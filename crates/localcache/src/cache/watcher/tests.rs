//! `CacheWatcher` unit tests that need the watcher's private engine lock
//! (RFC 024 R7).

use std::sync::Arc;

use tempfile::TempDir;

use super::*;
use crate::cache::engine::CacheEngine;

/// A watcher over a file-backed database holding exactly one entry.
fn watcher_with_one_entry(dir: &TempDir) -> CacheWatcher<Vec<f32>> {
    let engine: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(dir.path().join("watcher.sqlite3"))
        .build()
        .unwrap();
    let source = dir.path().join("a.txt");
    std::fs::write(&source, b"x").unwrap();
    engine.set(&source, &vec![1.0]).unwrap();
    engine.watcher().unwrap()
}

/// Poison the watcher's engine lock the way a real fault would: a thread
/// panics while holding it. No `unsafe` and no test hook are needed, because
/// this module can reach the private lock.
fn poison_engine_lock(watcher: &CacheWatcher<Vec<f32>>) {
    let inner = Arc::clone(&watcher.inner);
    let joined = std::thread::spawn(move || {
        let _guard = inner.engine.lock().unwrap();
        panic!("deliberately poison the watcher's engine lock");
    })
    .join();
    assert!(joined.is_err());
    assert!(watcher.inner.engine.is_poisoned());
}

#[test]
fn entry_count_counts_the_engines_entries() {
    let dir = TempDir::new().unwrap();
    let watcher = watcher_with_one_entry(&dir);
    assert_eq!(watcher.entry_count().unwrap(), 1);
}

/// RFC 024 R7: a poisoned lock is an error from `entry_count`, where the
/// deprecated `watched_count` reports the same fault as a plain `0` — a
/// count that looks like "nothing is cached".
#[test]
#[allow(deprecated)]
fn poisoned_lock_is_an_error_from_entry_count_and_a_silent_zero_from_watched_count() {
    let dir = TempDir::new().unwrap();
    let watcher = watcher_with_one_entry(&dir);
    assert_eq!(
        watcher.entry_count().unwrap(),
        1,
        "the entry is really there"
    );

    poison_engine_lock(&watcher);

    match watcher.entry_count() {
        Err(LocalFileCacheError::Poisoned { resource }) => assert_eq!(resource, "CacheWatcher"),
        other => panic!("expected Poisoned {{ resource: \"CacheWatcher\" }}, got {other:?}"),
    }
    assert_eq!(
        watcher.watched_count(),
        0,
        "the deprecated method hides the error as 0 (its documented behaviour)"
    );
}
