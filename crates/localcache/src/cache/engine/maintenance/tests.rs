use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::params;
use tempfile::TempDir;

use crate::db::repository;
use crate::{CacheEngine, CacheOptions};

/// Page size used by every boundary test below — small enough that
/// "page_size - 1 / page_size / page_size + 1" costs a handful of files,
/// not `MAINTENANCE_CHUNK`'s 10 000.
const TEST_PAGE: usize = 3;

fn engine_in(dir: &TempDir, ttl: Option<Duration>) -> CacheEngine<Vec<f32>> {
    CacheEngine::open(CacheOptions {
        database_path: dir.path().join("test.sqlite3"),
        ttl,
        ..CacheOptions::default()
    })
    .unwrap()
}

fn write_file(dir: &TempDir, name: &str) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, b"x").unwrap();
    path
}

/// Create and `set` `n` entries, named so they sort in creation order — the
/// paged scan orders by `path`, and the shared tempdir prefix makes the
/// zero-padded suffix the only thing that varies.
fn set_n(engine: &CacheEngine<Vec<f32>>, dir: &TempDir, prefix: &str, n: usize) -> Vec<PathBuf> {
    (0..n)
        .map(|i| {
            let path = write_file(dir, &format!("{prefix}-{i:04}.txt"));
            engine.set(&path, &vec![i as f32]).unwrap();
            path
        })
        .collect()
}

fn set_updated_at(engine: &CacheEngine<Vec<f32>>, path: &Path, updated_at: i64) {
    engine
        .conn
        .execute(
            "UPDATE files SET updated_at = ?1 WHERE namespace = ?2 AND path = ?3",
            params![updated_at, engine.namespace, path.display().to_string()],
        )
        .unwrap();
}

// -----------------------------------------------------------------
// §7.1 — chunk boundaries
// -----------------------------------------------------------------

#[test]
fn cleanup_missing_files_paged_chunk_boundaries() {
    for n in [TEST_PAGE - 1, TEST_PAGE, TEST_PAGE + 1] {
        let dir = TempDir::new().unwrap();
        let engine = engine_in(&dir, None);
        let paths = set_n(&engine, &dir, "f", n);
        for path in &paths {
            std::fs::remove_file(path).unwrap();
        }
        let removed = engine.cleanup_missing_files_paged(TEST_PAGE).unwrap();
        assert_eq!(removed, n, "n={n}");
        assert_eq!(engine.entry_count().unwrap(), 0, "n={n}");
    }
}

#[test]
fn cleanup_expired_paged_chunk_boundaries() {
    for n in [TEST_PAGE - 1, TEST_PAGE, TEST_PAGE + 1] {
        let dir = TempDir::new().unwrap();
        let engine = engine_in(&dir, Some(Duration::from_secs(1000)));
        let paths = set_n(&engine, &dir, "f", n);
        let long_ago = repository::now_secs() - 2000;
        for path in &paths {
            set_updated_at(&engine, path, long_ago);
        }
        let removed = engine.cleanup_expired_paged(TEST_PAGE).unwrap();
        assert_eq!(removed, n, "n={n}");
        assert_eq!(engine.entry_count().unwrap(), 0, "n={n}");
    }
}

// -----------------------------------------------------------------
// §7.2 — a fully-absent page must still advance the cursor
// -----------------------------------------------------------------

#[test]
fn cleanup_missing_files_cursor_advances_through_a_fully_absent_page() {
    let dir = TempDir::new().unwrap();
    let engine = engine_in(&dir, None);
    // Three pages: [3 absent] [3 absent] [1 absent] — the entire first
    // two pages are absent, so if the cursor were taken from survivors
    // (there are none), it would never move.
    let paths = set_n(&engine, &dir, "f", 7);
    for path in &paths {
        std::fs::remove_file(path).unwrap();
    }

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _keep_dir_alive = dir;
        let result = engine.cleanup_missing_files_paged(TEST_PAGE);
        let _ = tx.send(result);
    });
    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(removed)) => assert_eq!(removed, 7),
        Ok(Err(error)) => panic!("cleanup_missing_files_paged returned an error: {error:?}"),
        Err(_) => panic!(
            "cleanup_missing_files_paged did not return within 10s — the cursor likely did \
             not advance past a fully-absent page (the exact regression this test exists to \
             catch)"
        ),
    }
}

#[test]
fn cleanup_expired_cursor_advances_through_a_fully_expired_page() {
    let dir = TempDir::new().unwrap();
    let engine = engine_in(&dir, Some(Duration::from_secs(1000)));
    let paths = set_n(&engine, &dir, "f", 7);
    let long_ago = repository::now_secs() - 2000;
    for path in &paths {
        set_updated_at(&engine, path, long_ago);
    }

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _keep_dir_alive = dir;
        let result = engine.cleanup_expired_paged(TEST_PAGE);
        let _ = tx.send(result);
    });
    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(removed)) => assert_eq!(removed, 7),
        Ok(Err(error)) => panic!("cleanup_expired_paged returned an error: {error:?}"),
        Err(_) => panic!(
            "cleanup_expired_paged did not return within 10s — the cursor likely did not \
             advance past a fully-expired page"
        ),
    }
}

// -----------------------------------------------------------------
// §7.3 — mixed present/absent (or expired/fresh) spanning pages
// -----------------------------------------------------------------

#[test]
fn cleanup_missing_files_mixed_absence_spans_pages() {
    let dir = TempDir::new().unwrap();
    let engine = engine_in(&dir, None);
    // 7 entries, page_size 3 -> pages of [3, 3, 1]. Even indices absent
    // (0, 2, 4, 6 -> 4 removed), odd indices present (1, 3, 5 -> 3 kept),
    // so every page has at least one of each.
    let paths = set_n(&engine, &dir, "f", 7);
    for (index, path) in paths.iter().enumerate() {
        if index % 2 == 0 {
            std::fs::remove_file(path).unwrap();
        }
    }
    let removed = engine.cleanup_missing_files_paged(TEST_PAGE).unwrap();
    assert_eq!(removed, 4);
    assert_eq!(engine.entry_count().unwrap(), 3);
    for (index, path) in paths.iter().enumerate() {
        let still_present = engine.get(path).unwrap().is_some();
        assert_eq!(still_present, index % 2 == 1, "index={index}");
    }
}

#[test]
fn cleanup_expired_mixed_expiry_spans_pages() {
    let dir = TempDir::new().unwrap();
    let engine = engine_in(&dir, Some(Duration::from_secs(1000)));
    let paths = set_n(&engine, &dir, "f", 7);
    let long_ago = repository::now_secs() - 2000;
    for (index, path) in paths.iter().enumerate() {
        if index % 2 == 0 {
            set_updated_at(&engine, path, long_ago);
        }
    }
    let removed = engine.cleanup_expired_paged(TEST_PAGE).unwrap();
    assert_eq!(removed, 4);
    assert_eq!(engine.entry_count().unwrap(), 3);
    for (index, path) in paths.iter().enumerate() {
        let still_present = engine.get(path).unwrap().is_some();
        assert_eq!(still_present, index % 2 == 1, "index={index}");
    }
}

// -----------------------------------------------------------------
// §7.4 — partial progress on error
// -----------------------------------------------------------------
//
// Tested at the `repository::delete_paths` layer — the exact
// transaction-scoped unit the paged loop calls once per page — rather
// than by driving a full `_paged` sweep through an induced mid-sweep
// failure. Forcing *that* deterministically would need either a
// test-only hook in production code (out of this RFC's two-file scope)
// or a racy multi-threaded pragma flip timed against page completion.
// This test exercises the identical `unchecked_transaction` +
// `prepare_cached` DELETE path the loop uses, so it validates the real
// atomicity mechanism, not a proxy for it.
#[test]
fn delete_paths_partial_progress_on_error_rolls_back_only_the_failing_page() {
    let dir = TempDir::new().unwrap();
    let engine = engine_in(&dir, None);
    let paths = set_n(&engine, &dir, "f", 6);
    let mut stored: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
    stored.sort();
    let (page_a, page_b) = stored.split_at(3);
    let page_a = page_a.to_vec();
    let page_b = page_b.to_vec();

    let removed_a = repository::delete_paths(&engine.conn, &engine.namespace, &page_a).unwrap();
    assert_eq!(removed_a, 3);
    assert_eq!(engine.entry_count().unwrap(), 3, "page A must be committed");

    // Force the next page's DELETE to fail deterministically, without
    // threading: SQLite's query_only pragma rejects any write with
    // SQLITE_READONLY.
    engine.conn.execute_batch("PRAGMA query_only = 1;").unwrap();
    let result = repository::delete_paths(&engine.conn, &engine.namespace, &page_b);
    assert!(
        result.is_err(),
        "expected the forced write failure to propagate"
    );
    engine.conn.execute_batch("PRAGMA query_only = 0;").unwrap();

    assert_eq!(
        engine.entry_count().unwrap(),
        3,
        "page A stays committed; page B must not be partially or fully committed"
    );
}

// query_only lets the SELECT scan succeed and the DELETE fail, so the
// failure propagates out of the paged loop itself, not just out of
// delete_paths — the loop-level counterpart to the repository-level test
// above.
#[test]
fn cleanup_missing_files_paged_propagates_a_page_delete_failure() {
    let dir = TempDir::new().unwrap();
    let engine = engine_in(&dir, None);
    let paths = set_n(&engine, &dir, "f", TEST_PAGE);
    for path in &paths {
        std::fs::remove_file(path).unwrap();
    }

    engine.conn.execute_batch("PRAGMA query_only = 1;").unwrap();
    assert!(engine.cleanup_missing_files_paged(TEST_PAGE).is_err());
    engine.conn.execute_batch("PRAGMA query_only = 0;").unwrap();

    assert_eq!(
        engine.entry_count().unwrap(),
        TEST_PAGE,
        "nothing removed — the single page's delete failed and rolled back"
    );
}

// -----------------------------------------------------------------
// §7.6 — empty and single-entry namespaces
// -----------------------------------------------------------------

#[test]
fn cleanup_missing_files_empty_and_single_entry_namespace() {
    let dir = TempDir::new().unwrap();
    let engine = engine_in(&dir, None);
    assert_eq!(engine.cleanup_missing_files_paged(TEST_PAGE).unwrap(), 0);

    let path = write_file(&dir, "solo.txt");
    engine.set(&path, &vec![1.0_f32]).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(engine.cleanup_missing_files_paged(TEST_PAGE).unwrap(), 1);
    assert_eq!(engine.entry_count().unwrap(), 0);
}

#[test]
fn cleanup_expired_empty_and_single_entry_namespace() {
    let dir = TempDir::new().unwrap();
    let engine = engine_in(&dir, Some(Duration::from_secs(1000)));
    assert_eq!(engine.cleanup_expired_paged(TEST_PAGE).unwrap(), 0);

    let path = write_file(&dir, "solo.txt");
    engine.set(&path, &vec![1.0_f32]).unwrap();
    set_updated_at(&engine, &path, repository::now_secs() - 2000);
    assert_eq!(engine.cleanup_expired_paged(TEST_PAGE).unwrap(), 1);
    assert_eq!(engine.entry_count().unwrap(), 0);
}

#[test]
fn cleanup_expired_ttl_none_returns_ok_zero() {
    let dir = TempDir::new().unwrap();
    let engine = engine_in(&dir, None);
    let path = write_file(&dir, "f.txt");
    engine.set(&path, &vec![1.0_f32]).unwrap();
    assert_eq!(engine.cleanup_expired_paged(TEST_PAGE).unwrap(), 0);
    assert_eq!(engine.entry_count().unwrap(), 1);
}

// -----------------------------------------------------------------
// §7.7 — the public method, through MAINTENANCE_CHUNK
// -----------------------------------------------------------------

#[test]
fn cleanup_missing_files_public_method_uses_the_real_constant() {
    let dir = TempDir::new().unwrap();
    let engine = engine_in(&dir, None);
    let paths = set_n(&engine, &dir, "f", 5);
    for path in &paths {
        std::fs::remove_file(path).unwrap();
    }
    // Calls the public `cleanup_missing_files`, not the `_paged` helper —
    // proves the MAINTENANCE_CHUNK wiring, not only the parameterised
    // implementation.
    assert_eq!(engine.cleanup_missing_files().unwrap(), 5);
    assert_eq!(engine.entry_count().unwrap(), 0);
}

#[test]
fn cleanup_expired_public_method_uses_the_real_constant() {
    let dir = TempDir::new().unwrap();
    let engine = engine_in(&dir, Some(Duration::from_secs(1000)));
    let paths = set_n(&engine, &dir, "f", 5);
    let long_ago = repository::now_secs() - 2000;
    for path in &paths {
        set_updated_at(&engine, path, long_ago);
    }
    assert_eq!(engine.cleanup_expired().unwrap(), 5);
    assert_eq!(engine.entry_count().unwrap(), 0);
}

// -----------------------------------------------------------------
// §7.8 — namespace isolation
// -----------------------------------------------------------------

#[test]
fn cleanup_missing_files_does_not_touch_other_namespaces() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("shared.sqlite3");
    let engine_a = CacheEngine::<Vec<f32>>::open(CacheOptions {
        database_path: db_path.clone(),
        namespace: "a".into(),
        ..CacheOptions::default()
    })
    .unwrap();
    let engine_b = CacheEngine::<Vec<f32>>::open(CacheOptions {
        database_path: db_path,
        namespace: "b".into(),
        ..CacheOptions::default()
    })
    .unwrap();

    let paths_a = set_n(&engine_a, &dir, "a", 3);
    let paths_b = set_n(&engine_b, &dir, "b", 3);
    for path in paths_a.iter().chain(paths_b.iter()) {
        std::fs::remove_file(path).unwrap();
    }

    assert_eq!(engine_a.cleanup_missing_files_paged(TEST_PAGE).unwrap(), 3);
    assert_eq!(engine_a.entry_count().unwrap(), 0);
    assert_eq!(
        engine_b.entry_count().unwrap(),
        3,
        "namespace b must be untouched by a's cleanup"
    );
}

#[test]
fn cleanup_expired_does_not_touch_other_namespaces() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("shared.sqlite3");
    let ttl = Some(Duration::from_secs(1000));
    let engine_a = CacheEngine::<Vec<f32>>::open(CacheOptions {
        database_path: db_path.clone(),
        namespace: "a".into(),
        ttl,
        ..CacheOptions::default()
    })
    .unwrap();
    let engine_b = CacheEngine::<Vec<f32>>::open(CacheOptions {
        database_path: db_path,
        namespace: "b".into(),
        ttl,
        ..CacheOptions::default()
    })
    .unwrap();

    let paths_a = set_n(&engine_a, &dir, "a", 3);
    // Namespace b's entries are deliberately left fresh (not backdated):
    // this test only calls cleanup on namespace a, so b's expiry state
    // is irrelevant to it — backdating them under a's namespace would
    // silently match zero rows (`set_updated_at` filters by the given
    // engine's own namespace) and be misleading dead code.
    let _paths_b = set_n(&engine_b, &dir, "b", 3);
    let long_ago = repository::now_secs() - 2000;
    for path in &paths_a {
        set_updated_at(&engine_a, path, long_ago);
    }

    assert_eq!(engine_a.cleanup_expired_paged(TEST_PAGE).unwrap(), 3);
    assert_eq!(engine_a.entry_count().unwrap(), 0);
    assert_eq!(
        engine_b.entry_count().unwrap(),
        3,
        "namespace b must be untouched by a's cleanup"
    );
}
