//! Executable provenance and immutability gates for RFC 010 fixtures.

use std::fs;
use std::path::Path;

use rusqlite::Connection;
use sha2::{Digest, Sha256};

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
