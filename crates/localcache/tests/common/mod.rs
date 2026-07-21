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
