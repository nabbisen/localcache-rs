//! Executable provenance and immutability gates for RFC 010 fixtures.

use std::fs;
use std::path::Path;

use rusqlite::Connection;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

use localcache::{CacheEngine, JournalMode, SynchronousMode};

#[path = "fixture_integrity/public_boundaries.rs"]
mod public_boundaries;

const V0_1_PATH: &str = "tests/fixtures/compat-v0_1.sqlite3";
const V0_1_SHA256: &str = "bd0bb9ffb9e07abafebde2c8a492618bf23ba8cf0e8c29cd8a9a76a4f5153aac";
const V0_19_INDEX_PATH: &str = "tests/fixtures/compat-v0_19-user-index.sqlite3";
const V0_19_INDEX_SHA256: &str = "585ea037ad94ef77696b3bb3c6d13d9778975057e2bdd7bdc5b01b299cfc86df";

fn assert_sha256(path: &str, expected: &str) {
    let bytes = fs::read(path).unwrap_or_else(|error| panic!("cannot read {path}: {error}"));
    let actual = format!("{:x}", Sha256::digest(bytes));
    assert_eq!(actual, expected, "immutable fixture digest changed: {path}");
}

#[test]
fn historical_v0_1_fixture_has_exact_digest_and_public_api_state() {
    assert_sha256(V0_1_PATH, V0_1_SHA256);
    let conn = Connection::open_with_flags(
        Path::new(V0_1_PATH),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        0
    );
    let ids = conn
        .prepare("SELECT id FROM files ORDER BY id")
        .unwrap()
        .query_map([], |row| row.get::<_, i64>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(ids, [1, 3]);
    assert_eq!(
        conn.query_row(
            "SELECT seq FROM sqlite_sequence WHERE name = 'files'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        3
    );
    let payloads = conn
        .prepare("SELECT content FROM payloads ORDER BY file_id")
        .unwrap()
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let decoded: Vec<Vec<f32>> = payloads
        .iter()
        .map(|bytes| {
            bincode::serde::decode_from_slice(bytes, bincode::config::legacy())
                .unwrap()
                .0
        })
        .collect();
    assert_eq!(decoded, [vec![1.25, -2.5, 3.75], vec![8.5, 13.0]]);
    assert_eq!(
        conn.query_row("SELECT count(*) FROM files WHERE hash IS NULL", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        2
    );
}

#[test]
fn released_v4_user_index_fixture_has_exact_digest_and_shape() {
    assert_sha256(V0_19_INDEX_PATH, V0_19_INDEX_SHA256);
    let conn = Connection::open_with_flags(
        Path::new(V0_19_INDEX_PATH),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        4
    );
    let index_sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE type = 'index' AND name = 'lc_user_rfc010'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        index_sql,
        "CREATE INDEX lc_user_rfc010 ON files(namespace, path)"
    );
    let query_plan: String = conn
        .query_row(
            "EXPLAIN QUERY PLAN SELECT id FROM files INDEXED BY lc_user_rfc010
             WHERE namespace = 'default' AND path = '/fixture/input-user-index.bin'",
            [],
            |row| row.get(3),
        )
        .unwrap();
    assert!(query_plan.contains("lc_user_rfc010"), "{query_plan}");
}

#[test]
fn released_v4_user_index_and_payload_survive_public_migration() {
    assert_sha256(V0_19_INDEX_PATH, V0_19_INDEX_SHA256);
    let directory = TempDir::new().unwrap();
    let copied = directory.path().join("compat-v0_19-user-index.sqlite3");
    fs::copy(V0_19_INDEX_PATH, &copied).unwrap();

    let before = Connection::open(&copied).unwrap();
    let index_sql: String = before
        .query_row(
            "SELECT sql FROM sqlite_schema
             WHERE type = 'index' AND name = 'lc_user_rfc010'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let (old_mtime, payload): (i64, Vec<u8>) = before
        .query_row(
            "SELECT files.mtime, payloads.content
             FROM files JOIN payloads ON payloads.file_id = files.id",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    drop(before);

    let engine = CacheEngine::<Vec<f32>>::builder()
        .database(&copied)
        .journal_mode(JournalMode::Delete)
        .build()
        .unwrap();
    let entries = engine.query().run().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].payload, vec![21.0, 34.0]);
    drop(engine);

    let after = Connection::open(&copied).unwrap();
    assert_eq!(
        after
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        5
    );
    assert_eq!(
        after
            .query_row("SELECT mtime FROM files", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        old_mtime * 1_000_000_000
    );
    assert_eq!(
        after
            .query_row("SELECT content FROM payloads", [], |row| {
                row.get::<_, Vec<u8>>(0)
            })
            .unwrap(),
        payload
    );
    assert_eq!(
        after
            .query_row(
                "SELECT sql FROM sqlite_schema
                 WHERE type = 'index' AND name = 'lc_user_rfc010'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        index_sql
    );
    let query_plan: String = after
        .query_row(
            "EXPLAIN QUERY PLAN SELECT id FROM files INDEXED BY lc_user_rfc010
             WHERE namespace = 'default' AND path = '/fixture/input-user-index.bin'",
            [],
            |row| row.get(3),
        )
        .unwrap();
    assert!(query_plan.contains("lc_user_rfc010"), "{query_plan}");
}

#[derive(Debug, PartialEq)]
struct V0_1Snapshot {
    version: i64,
    journal_mode: String,
    schema: Vec<(String, String, String, Option<String>)>,
    files: Vec<(i64, String, i64, i64, Option<String>, i64)>,
    payloads: Vec<(i64, Vec<u8>)>,
    sequence: Vec<(String, rusqlite::types::Value)>,
}

fn v0_1_snapshot(path: &Path) -> V0_1Snapshot {
    let conn = Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .unwrap();

    let schema = conn
        .prepare(
            "SELECT type, name, tbl_name, sql
             FROM sqlite_schema
             ORDER BY type, name",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let files = conn
        .prepare(
            "SELECT id, path, mtime, file_size, hash, updated_at
             FROM files ORDER BY id",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let payloads = conn
        .prepare("SELECT file_id, content FROM payloads ORDER BY file_id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let sequence = conn
        .prepare("SELECT name, seq FROM sqlite_sequence ORDER BY name")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    V0_1Snapshot {
        version: conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap(),
        journal_mode: conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap(),
        schema,
        files,
        payloads,
        sequence,
    }
}

#[test]
fn public_open_migrates_v0_1_atomically_for_every_runtime_option() {
    assert_sha256(V0_1_PATH, V0_1_SHA256);
    for journal_mode in [JournalMode::Wal, JournalMode::Delete, JournalMode::Memory] {
        for synchronous in [
            SynchronousMode::Off,
            SynchronousMode::Normal,
            SynchronousMode::Full,
            SynchronousMode::Extra,
        ] {
            let directory = TempDir::new().unwrap();
            let copied = directory.path().join("compat-v0_1.sqlite3");
            fs::copy(V0_1_PATH, &copied).unwrap();
            let before = v0_1_snapshot(&copied);

            let engine = CacheEngine::<Vec<f32>>::builder()
                .database(&copied)
                .journal_mode(journal_mode)
                .synchronous(synchronous)
                .build()
                .unwrap_or_else(|error| {
                    panic!("migration failed for {journal_mode:?}/{synchronous:?}: {error}")
                });
            let mut decoded = engine
                .query()
                .run()
                .unwrap()
                .into_iter()
                .map(|entry| entry.payload)
                .collect::<Vec<_>>();
            decoded.sort_by(|left, right| left.partial_cmp(right).unwrap());
            assert_eq!(
                decoded,
                [vec![1.25, -2.5, 3.75], vec![8.5, 13.0]],
                "decoded payload mismatch for {journal_mode:?}/{synchronous:?}"
            );
            drop(engine);

            let conn = Connection::open(&copied).unwrap();
            assert_eq!(
                conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                    .unwrap(),
                5
            );
            let files = conn
                .prepare(
                    "SELECT id, path, mtime, file_size, hash, updated_at
                     FROM files ORDER BY id",
                )
                .unwrap()
                .query_map([], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            let expected_files = before
                .files
                .iter()
                .map(|(id, path, mtime, size, hash, updated)| {
                    (
                        *id,
                        path.clone(),
                        mtime * 1_000_000_000,
                        *size,
                        hash.clone(),
                        *updated,
                    )
                })
                .collect::<Vec<_>>();
            assert_eq!(files, expected_files);
            let payloads = conn
                .prepare("SELECT file_id, content FROM payloads ORDER BY file_id")
                .unwrap()
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<Result<Vec<(i64, Vec<u8>)>, _>>()
                .unwrap();
            assert_eq!(payloads, before.payloads);
            assert_eq!(
                conn.query_row(
                    "SELECT seq FROM sqlite_sequence WHERE name = 'files'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
                3
            );
            assert_eq!(
                conn.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
                0
            );
            assert_eq!(
                conn.query_row(
                    "SELECT count(*) FROM sqlite_schema
                     WHERE name LIKE '__localcache_rfc010_%'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
                0
            );
            let observed_journal: String = conn
                .query_row("PRAGMA journal_mode", [], |row| row.get(0))
                .unwrap();
            let expected_journal = match journal_mode {
                JournalMode::Wal => "wal",
                JournalMode::Delete | JournalMode::Memory => "delete",
            };
            assert_eq!(observed_journal, expected_journal);
            drop(conn);
            assert!(!directory.path().join("compat-v0_1.sqlite3-wal").exists());
            assert!(!directory.path().join("compat-v0_1.sqlite3-shm").exists());
        }
    }
}
