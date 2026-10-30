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

/// RFC 025 R4: every failure of the operating-system watcher is a `Watcher`
/// error whose text reads once ("file watching failed: could not …").
fn assert_watcher_error(error: LocalFileCacheError, expected_start: &str) {
    match &error {
        LocalFileCacheError::Watcher(message) => {
            assert!(message.starts_with(expected_start), "{message}");
            assert!(!message.contains("failed"), "reads twice: {message}");
        }
        other => panic!("expected a Watcher error, got {other:?}"),
    }
    assert!(
        error
            .to_string()
            .starts_with("file watching failed: could not ")
    );
}

#[test]
fn watch_and_unwatch_failures_are_watcher_errors() {
    let dir = TempDir::new().unwrap();
    let mut watcher = watcher_with_one_entry(&dir);
    let missing = dir.path().join("no-such-file");

    assert_watcher_error(watcher.watch(&missing).unwrap_err(), "could not watch '");
    assert_watcher_error(
        watcher.unwatch(&missing).unwrap_err(),
        "could not unwatch '",
    );
    assert_watcher_error(
        watcher.watch_dir(&missing).unwrap_err(),
        "could not watch directory '",
    );
    assert_watcher_error(
        watcher.unwatch_dir(&missing).unwrap_err(),
        "could not unwatch directory '",
    );
}

#[test]
fn debounced_watch_dir_and_unwatch_dir_failures_are_watcher_errors() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(dir.path().join("debounced.sqlite3"))
        .build()
        .unwrap();
    let mut watcher = engine
        .debounced_watcher(std::time::Duration::from_millis(50))
        .unwrap();
    let missing = dir.path().join("no-such-directory");

    assert_watcher_error(
        watcher.watch_dir(&missing).unwrap_err(),
        "could not watch directory '",
    );
    assert_watcher_error(
        watcher.unwatch_dir(&missing).unwrap_err(),
        "could not unwatch directory '",
    );
}
