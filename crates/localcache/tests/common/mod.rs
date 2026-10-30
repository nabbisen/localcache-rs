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

/// Replace the stored bytes **and** encoding tag of the entry at
/// `stored_path` (the path as `keys()` returns it), straight in the database.
/// Reaches a stored payload that no public call writes, such as a hostile one.
#[allow(dead_code)]
pub fn store_raw_payload(
    database: &std::path::Path,
    stored_path: &std::path::Path,
    content: &[u8],
    encoding: &str,
) {
    let conn = rusqlite::Connection::open(database).unwrap();
    let changed = conn
        .execute(
            "UPDATE payloads SET content = ?1, encoding = ?2
             WHERE file_id = (SELECT id FROM files WHERE path = ?3)",
            rusqlite::params![content, encoding, stored_path.to_str().unwrap()],
        )
        .unwrap();
    assert_eq!(changed, 1, "expected exactly one payload row to replace");
}

/// Run the test `test` of this same test binary in a child process whose
/// address space is limited to `address_space_kib` (`ulimit -v`), with `envs`
/// set, and return its output. A child that aborts or fails to allocate shows
/// up as a failed exit status here instead of taking the parent down.
#[cfg(unix)]
#[allow(dead_code)]
pub fn run_self_with_address_limit(
    test: &str,
    address_space_kib: u64,
    envs: &[(&str, &str)],
) -> std::process::Output {
    std::process::Command::new("sh")
        .arg("-c")
        .arg(format!(
            "ulimit -v {address_space_kib} && exec \"$0\" --exact {test} --include-ignored --nocapture --test-threads=1"
        ))
        .arg(std::env::current_exe().unwrap())
        .envs(envs.iter().copied())
        .output()
        .unwrap()
}

/// One field of `/proc/self/status` in kibibytes (`VmHWM`: peak resident set,
/// `VmPeak`: peak address space), or `None` where there is no such file.
#[allow(dead_code)]
pub fn proc_status_kib(field: &str) -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with(field))?;
    line.split_whitespace().nth(1)?.parse().ok()
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
