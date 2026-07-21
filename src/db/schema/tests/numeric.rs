use std::path::Path;

use rusqlite::{Connection, params, types::Value};
use tempfile::TempDir;

use super::super::initialize;
use super::helpers::{assert_no_migration_objects, open_v1};

const MIN_SECONDS: i64 = -9_223_372_036;
const MAX_SECONDS: i64 = 9_223_372_036;

#[derive(Debug, PartialEq)]
struct V4Snapshot {
    version: i64,
    journal_mode: String,
    schema: Vec<(String, String, Option<String>)>,
    files: Vec<(i64, Value)>,
    payloads: Vec<(i64, Vec<u8>, String)>,
    sequence: Vec<(String, Value)>,
}

fn open_v4(path: &Path, mtimes: &[Value]) -> Connection {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         CREATE TABLE files (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            namespace TEXT NOT NULL DEFAULT 'default',
            path TEXT NOT NULL,
            mtime INTEGER NOT NULL,
            file_size INTEGER NOT NULL,
            hash TEXT,
            updated_at INTEGER NOT NULL,
            payload_version INTEGER NOT NULL DEFAULT 0,
            last_accessed_at INTEGER NOT NULL DEFAULT 0,
            UNIQUE(namespace, path)
         );
         CREATE TABLE payloads (
            file_id INTEGER PRIMARY KEY,
            content BLOB NOT NULL,
            encoding TEXT NOT NULL DEFAULT 'raw',
            FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
         );
         CREATE INDEX idx_files_namespace_path ON files(namespace, path);
         CREATE INDEX idx_files_lru
            ON files(namespace, last_accessed_at, updated_at);
         PRAGMA user_version = 4;",
    )
    .unwrap();
    for (offset, mtime) in mtimes.iter().enumerate() {
        let id = i64::try_from(offset).unwrap() + 1;
        conn.execute(
            "INSERT INTO files
                (id, namespace, path, mtime, file_size, hash, updated_at,
                 payload_version, last_accessed_at)
             VALUES (?1, 'default', ?2, ?3, ?4, NULL, ?5, 0, 0)",
            params![id, format!("/fixture/numeric-{id}"), mtime, id * 2, id * 3],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO payloads(file_id, content, encoding)
             VALUES (?1, ?2, 'raw')",
            params![id, vec![0_u8, id as u8, 255]],
        )
        .unwrap();
    }
    conn
}

fn snapshot_v4(conn: &Connection) -> V4Snapshot {
    let schema = conn
        .prepare(
            "SELECT type, name, sql FROM sqlite_schema
             WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
        )
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let files = conn
        .prepare("SELECT id, mtime FROM files ORDER BY id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let payloads = conn
        .prepare("SELECT file_id, content, encoding FROM payloads ORDER BY file_id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
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
    V4Snapshot {
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
fn v4_numeric_inclusive_bounds_and_ordinary_seconds_convert_exactly() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("numeric-success.sqlite3");
    let mut conn = open_v4(
        &path,
        &[
            Value::Integer(MIN_SECONDS),
            Value::Integer(0),
            Value::Integer(1_784_600_942),
            Value::Integer(MAX_SECONDS),
        ],
    );
    let before_payloads = snapshot_v4(&conn).payloads;

    assert!(
        initialize(&mut conn, false)
            .unwrap()
            .schema_migration_committed
    );
    let converted = conn
        .prepare("SELECT mtime FROM files ORDER BY id")
        .unwrap()
        .query_map([], |row| row.get::<_, i64>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        converted,
        [
            MIN_SECONDS * 1_000_000_000,
            0,
            1_784_600_942_000_000_000,
            MAX_SECONDS * 1_000_000_000,
        ]
    );
    assert_eq!(snapshot_v4_payloads_after_migration(&conn), before_payloads);
}

#[test]
fn invalid_v4_numeric_storage_and_ranges_reject_unchanged() {
    let invalid = [
        Value::Integer(MIN_SECONDS - 1),
        Value::Integer(MAX_SECONDS + 1),
        Value::Integer(1_784_600_942_123_456_789),
        Value::Real(1.5),
        Value::Text("not-an-integer".into()),
        Value::Blob(vec![0, 1, 2]),
    ];
    for (case, value) in invalid.into_iter().enumerate() {
        let directory = TempDir::new().unwrap();
        let path = directory
            .path()
            .join(format!("numeric-invalid-{case}.sqlite3"));
        let mut conn = open_v4(&path, &[value]);
        let before = snapshot_v4(&conn);

        let error = initialize(&mut conn, false).unwrap_err().to_string();
        assert!(
            error.contains("v4 mtime") || error.contains("starting row"),
            "{error}"
        );
        assert_eq!(snapshot_v4(&conn), before, "case {case} changed");
        assert_no_migration_objects(&conn);
    }
}

#[test]
fn corrupt_null_v4_mtime_rejects_unchanged() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("numeric-null.sqlite3");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE files (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            namespace TEXT NOT NULL DEFAULT 'default',
            path TEXT NOT NULL,
            mtime INTEGER,
            file_size INTEGER NOT NULL,
            hash TEXT,
            updated_at INTEGER NOT NULL,
            payload_version INTEGER NOT NULL DEFAULT 0,
            last_accessed_at INTEGER NOT NULL DEFAULT 0,
            UNIQUE(namespace, path)
         );
         CREATE TABLE payloads (
            file_id INTEGER PRIMARY KEY,
            content BLOB NOT NULL,
            encoding TEXT NOT NULL DEFAULT 'raw',
            FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
         );
         CREATE INDEX idx_files_namespace_path ON files(namespace, path);
         CREATE INDEX idx_files_lru
            ON files(namespace, last_accessed_at, updated_at);
         INSERT INTO files
            (id, namespace, path, mtime, file_size, hash, updated_at,
             payload_version, last_accessed_at)
         VALUES (1, 'default', '/fixture/null', NULL, 1, NULL, 2, 0, 0);
         PRAGMA user_version = 4;",
    )
    .unwrap();
    let schema_version: i64 = conn
        .query_row("PRAGMA schema_version", [], |row| row.get(0))
        .unwrap();
    conn.execute_batch("PRAGMA writable_schema = ON;").unwrap();
    conn.execute(
        "UPDATE sqlite_schema
         SET sql = replace(sql, 'mtime INTEGER,', 'mtime INTEGER NOT NULL,')
         WHERE type = 'table' AND name = 'files'",
        [],
    )
    .unwrap();
    conn.execute_batch(&format!(
        "PRAGMA schema_version = {}; PRAGMA writable_schema = OFF;",
        schema_version + 1
    ))
    .unwrap();
    drop(conn);

    let mut conn = Connection::open(&path).unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT \"notnull\" FROM pragma_table_xinfo('files') WHERE name = 'mtime'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row("SELECT typeof(mtime) FROM files", [], |row| {
            row.get::<_, String>(0)
        })
        .unwrap(),
        "null"
    );
    let before = snapshot_v4(&conn);
    assert!(initialize(&mut conn, false).is_err());
    assert_eq!(snapshot_v4(&conn), before);
    assert_no_migration_objects(&conn);
}

#[test]
fn maximum_sequence_is_preserved_and_next_insert_exhausts() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sequence-max.sqlite3");
    let mut conn = open_v1(&path);
    conn.execute(
        "UPDATE sqlite_sequence SET seq = ?1 WHERE name = 'files'",
        [i64::MAX],
    )
    .unwrap();

    assert!(
        initialize(&mut conn, false)
            .unwrap()
            .schema_migration_committed
    );
    assert_eq!(
        conn.query_row(
            "SELECT seq FROM sqlite_sequence WHERE name = 'files'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        i64::MAX
    );
    let before_count: i64 = conn
        .query_row("SELECT count(*) FROM files", [], |row| row.get(0))
        .unwrap();
    assert!(
        conn.execute(
            "INSERT INTO files
                (namespace, path, mtime, file_size, hash, updated_at,
                 payload_version, last_accessed_at)
             VALUES ('default', '/fixture/exhausted', 0, 0, NULL, 0, 0, 0)",
            [],
        )
        .is_err()
    );
    assert_eq!(
        conn.query_row("SELECT count(*) FROM files", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        before_count
    );
}

#[test]
fn stored_sequence_below_live_id_restores_effective_live_high_water() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sequence-below-live.sqlite3");
    let mut conn = open_v1(&path);
    conn.execute(
        "UPDATE sqlite_sequence SET seq = 3 WHERE name = 'files'",
        [],
    )
    .unwrap();

    assert!(
        initialize(&mut conn, false)
            .unwrap()
            .schema_migration_committed
    );
    assert_eq!(
        conn.query_row(
            "SELECT seq FROM sqlite_sequence WHERE name = 'files'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        5
    );
    conn.execute(
        "INSERT INTO files
            (namespace, path, mtime, file_size, hash, updated_at,
             payload_version, last_accessed_at)
         VALUES ('default', '/fixture/next-id', 0, 0, NULL, 0, 0, 0)",
        [],
    )
    .unwrap();
    assert_eq!(conn.last_insert_rowid(), 6);
}

fn snapshot_v4_payloads_after_migration(conn: &Connection) -> Vec<(i64, Vec<u8>, String)> {
    conn.prepare("SELECT file_id, content, encoding FROM payloads ORDER BY file_id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}
