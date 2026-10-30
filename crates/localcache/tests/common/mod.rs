//! Shared test helpers for integration tests.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use tempfile::TempDir;

use localcache::{CacheEngine, CacheOptions, ChangeDetectionMode};

/// Create a file inside `dir` with `name` and `content`, returning its path.
pub fn write_file(dir: &TempDir, name: &str, content: &[u8]) -> PathBuf {
    let path = dir.path().join(name);
    let mut f = fs::File::create(&path).unwrap();
    f.write_all(content).unwrap();
    path
}

/// Open a `CacheEngine<Vec<f32>>` backed by `dir/test.sqlite3` with the
/// given change-detection mode.
#[allow(dead_code)]
pub fn make_engine(dir: &TempDir, mode: ChangeDetectionMode) -> CacheEngine<Vec<f32>> {
    CacheEngine::open(CacheOptions {
        database_path: dir.path().join("test.sqlite3"),
        change_detection_mode: mode,
        ..CacheOptions::default()
    })
    .unwrap()
}

/// Make every entry of `database` `seconds` older, by moving its `updated_at`
/// back, so a test can expire entries without sleeping.
#[allow(dead_code)]
pub fn age_entries(database: &std::path::Path, seconds: i64) {
    rusqlite::Connection::open(database)
        .unwrap()
        .execute("UPDATE files SET updated_at = updated_at - ?1", [seconds])
        .unwrap();
}

/// Overwrite the stored payload of the entry at `stored_path` (the path as
/// `keys()` returns it) with bytes that no codec can decode, straight in the
/// database. Reaches a decode failure that no public call produces.
#[allow(dead_code)]
pub fn corrupt_payload(database: &std::path::Path, stored_path: &std::path::Path) {
    let conn = rusqlite::Connection::open(database).unwrap();
    let changed = conn
        .execute(
            "UPDATE payloads SET content = ?1
             WHERE file_id = (SELECT id FROM files WHERE path = ?2)",
            rusqlite::params![vec![0xFF_u8], stored_path.to_str().unwrap()],
        )
        .unwrap();
    assert_eq!(changed, 1, "expected exactly one payload row to corrupt");
}

/// Change the stored encoding tag of the entry at `stored_path` (the path as
/// `keys()` returns it) to `encoding`, straight in the database. Reaches an
/// `UnknownEncoding` failure that no public call produces.
#[allow(dead_code)]
pub fn set_payload_encoding(
    database: &std::path::Path,
    stored_path: &std::path::Path,
    encoding: &str,
) {
    let conn = rusqlite::Connection::open(database).unwrap();
    let changed = conn
        .execute(
            "UPDATE payloads SET encoding = ?1
             WHERE file_id = (SELECT id FROM files WHERE path = ?2)",
            rusqlite::params![encoding, stored_path.to_str().unwrap()],
        )
        .unwrap();
    assert_eq!(changed, 1, "expected exactly one payload row to retag");
}

/// A file-backed database holding three `Vec<f32>` entries (`a.txt`, `b.txt`,
/// `c.txt`) whose second entry in path order is corrupted. Returns the
/// database path and the stored paths in path order.
#[allow(dead_code)]
pub fn database_with_one_corrupt_entry(dir: &TempDir, name: &str) -> (PathBuf, Vec<PathBuf>) {
    let database = dir.path().join(name);
    let engine: CacheEngine<Vec<f32>> = CacheEngine::builder().database(&database).build().unwrap();
    for file in ["a.txt", "b.txt", "c.txt"] {
        let p = write_file(dir, file, b"x");
        engine.set(&p, &vec![1.0_f32]).unwrap();
    }
    let mut stored = engine.keys(None).unwrap();
    stored.sort();
    corrupt_payload(&database, &stored[1]);
    (database, stored)
}
