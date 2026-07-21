use std::path::Path;

use rusqlite::{Connection, types::Value};

#[derive(Debug, PartialEq)]
pub(super) struct V1Snapshot {
    version: i64,
    journal_mode: String,
    schema: Vec<(String, String, String, Option<String>)>,
    files: Vec<(i64, String, Value, Value, Value, Value)>,
    payloads: Vec<(i64, Vec<u8>)>,
    sequence: Vec<(String, Value)>,
}

pub(super) fn open_v1(path: &Path) -> Connection {
    let conn = Connection::open(path).unwrap();
    create_v1(&conn);
    conn
}

pub(super) fn create_v1(conn: &Connection) {
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         CREATE TABLE files (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            path TEXT NOT NULL UNIQUE,
            mtime INTEGER NOT NULL,
            file_size INTEGER NOT NULL,
            hash TEXT,
            updated_at INTEGER NOT NULL
         );
         CREATE TABLE payloads (
            file_id INTEGER PRIMARY KEY,
            content BLOB NOT NULL,
            FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
         );
         CREATE INDEX idx_files_path ON files(path);
         INSERT INTO files(id, path, mtime, file_size, hash, updated_at) VALUES
            (1, '/fixture/a.bin', 2, 3, NULL, 4),
            (3, '/fixture/b.bin', -3, 0, 'hash-b', 5),
            (5, '/fixture/no-payload.bin', 4, 9, NULL, 6);
         INSERT INTO payloads(file_id, content) VALUES
            (1, X'0001FF'),
            (3, X'');
         UPDATE sqlite_sequence SET seq = 17 WHERE name = 'files';
         PRAGMA user_version = 1;",
    )
    .unwrap();
}

pub(super) fn snapshot_v1(conn: &Connection) -> V1Snapshot {
    let schema = conn
        .prepare(
            "SELECT type, name, tbl_name, sql
             FROM sqlite_schema ORDER BY type, name",
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

    V1Snapshot {
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

pub(super) fn assert_no_migration_objects(conn: &Connection) {
    let main_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_schema
             WHERE name LIKE '__localcache_rfc010_%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let temp_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_temp_schema
             WHERE name LIKE '__localcache_rfc010_%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!((main_count, temp_count), (0, 0));
}
