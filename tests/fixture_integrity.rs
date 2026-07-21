//! Executable provenance and immutability gates for RFC 010 fixtures.

use std::fs;
use std::path::Path;

use rusqlite::Connection;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

use localcache::{CacheEngine, JournalMode, SynchronousMode};

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
fn public_open_refuses_v0_1_before_destructive_migration_and_preserves_state() {
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

            let error = match CacheEngine::<Vec<f32>>::builder()
                .database(&copied)
                .journal_mode(journal_mode)
                .synchronous(synchronous)
                .build()
            {
                Ok(_) => panic!("historical v0.1 database unexpectedly entered legacy migration"),
                Err(error) => error,
            };

            let message = error.to_string();
            assert!(message.contains("recognized historical unversioned v0.1 database"));
            assert!(message.contains("database was not modified"));
            assert_eq!(
                v0_1_snapshot(&copied),
                before,
                "state changed for {journal_mode:?}/{synchronous:?}"
            );
            assert!(!directory.path().join("compat-v0_1.sqlite3-wal").exists());
            assert!(!directory.path().join("compat-v0_1.sqlite3-shm").exists());
        }
    }
}

#[test]
fn released_public_mixed_case_user_index_reopens_successfully() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("mixed-case-index.sqlite3");
    let engine = CacheEngine::<Vec<f32>>::builder()
        .database(&database)
        .build()
        .unwrap();
    for suffix in ["MixedCase_9", "dollar$sign", "éclair"] {
        assert_eq!(
            engine.create_path_index(suffix).unwrap(),
            format!("lc_user_{suffix}")
        );
    }
    drop(engine);

    let reopened = CacheEngine::<Vec<f32>>::builder()
        .database(&database)
        .build();
    assert!(
        reopened.is_ok(),
        "valid released mixed-case index was rejected"
    );
    drop(reopened);

    let conn = Connection::open_with_flags(
        &database,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .unwrap();
    let indexes = conn
        .prepare(
            "SELECT name FROM sqlite_schema
             WHERE type = 'index' AND name LIKE 'lc_user_%'
             ORDER BY name",
        )
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        indexes,
        [
            "lc_user_MixedCase_9",
            "lc_user_dollar$sign",
            "lc_user_éclair"
        ]
    );
}
