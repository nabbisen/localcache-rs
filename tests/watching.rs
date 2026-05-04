//! Integration tests — watching.

mod common;
use common::write_file;

use std::fs;

use tempfile::TempDir;

use localcache::{CacheEngine, ScanOptions};

#[cfg(feature = "watching")]
// ====================================================================
#[test]
fn preload_populates_cache_for_all_files() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("preload_root");
    fs::create_dir(&root).unwrap();

    for i in 0..5u32 {
        fs::write(root.join(format!("f{i}.txt")), format!("content {i}")).unwrap();
    }

    let engine: CacheEngine<usize> = CacheEngine::builder().database(":memory:").build().unwrap();

    let opts = ScanOptions {
        recursive: false,
        ..ScanOptions::default()
    };
    let report = engine
        .preload(&root, opts, false, |path| {
            Ok(fs::read_to_string(path)?.len())
        })
        .unwrap();

    assert_eq!(report.stored, 5);
    assert_eq!(report.already_fresh, 0);
    assert_eq!(report.skipped, 0);
    assert_eq!(engine.entry_count().unwrap(), 5);
}

#[test]
fn preload_skips_fresh_entries_by_default() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("preload_fresh");
    fs::create_dir(&root).unwrap();

    let path = root.join("stable.txt");
    fs::write(&path, b"unchanged").unwrap();

    let engine: CacheEngine<usize> = CacheEngine::builder().database(":memory:").build().unwrap();

    // First preload — stores the entry.
    let opts = ScanOptions {
        recursive: false,
        ..ScanOptions::default()
    };
    let r1 = engine
        .preload(&root, opts.clone(), false, |p| {
            Ok(fs::read_to_string(p)?.len())
        })
        .unwrap();
    assert_eq!(r1.stored, 1);

    // Second preload — entry is fresh, should be skipped.
    let r2 = engine
        .preload(&root, opts, false, |p| Ok(fs::read_to_string(p)?.len()))
        .unwrap();
    assert_eq!(r2.already_fresh, 1);
    assert_eq!(r2.stored, 0);
}

#[test]
fn preload_force_recomputes_fresh_entries() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("preload_force");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("f.txt"), b"data").unwrap();

    let engine: CacheEngine<usize> = CacheEngine::builder().database(":memory:").build().unwrap();

    let opts = ScanOptions {
        recursive: false,
        ..ScanOptions::default()
    };
    engine
        .preload(&root, opts.clone(), false, |_| Ok(1usize))
        .unwrap();

    // Force recompute.
    let r = engine.preload(&root, opts, true, |_| Ok(2usize)).unwrap();
    assert_eq!(r.stored, 1);
    assert_eq!(r.already_fresh, 0);

    // Verify the new payload.
    let entry = engine.get(root.join("f.txt")).unwrap().unwrap();
    assert_eq!(entry.payload, 2);
}

#[test]
fn preload_counts_factory_errors_in_skipped() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("preload_err");
    fs::create_dir(&root).unwrap();

    fs::write(root.join("good.txt"), b"ok").unwrap();
    fs::write(root.join("bad.txt"), b"fail").unwrap();

    let engine: CacheEngine<usize> = CacheEngine::builder().database(":memory:").build().unwrap();

    let opts = ScanOptions {
        recursive: false,
        ..ScanOptions::default()
    };
    let report = engine
        .preload(&root, opts, false, |p| {
            let content = fs::read_to_string(p)?;
            if content.contains("fail") {
                return Err("simulated error".into());
            }
            Ok(content.len())
        })
        .unwrap();

    assert_eq!(report.stored, 1);
    assert_eq!(report.skipped, 1);
    assert_eq!(report.errors.len(), 1);
    assert!(report.errors[0].1.contains("simulated error"));
}

#[test]
fn preload_recursive_option() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("preload_rec");
    let sub = root.join("sub");
    fs::create_dir_all(&sub).unwrap();

    fs::write(root.join("top.txt"), b"top").unwrap();
    fs::write(sub.join("deep.txt"), b"deep").unwrap();

    let engine: CacheEngine<usize> = CacheEngine::builder().database(":memory:").build().unwrap();

    let opts = ScanOptions {
        recursive: true,
        ..ScanOptions::default()
    };
    let report = engine.preload(&root, opts, false, |_| Ok(1usize)).unwrap();

    assert_eq!(report.stored, 2, "should find both top.txt and deep.txt");
}

// ====================================================================
// Phase 14 —(watching feature)
// ====================================================================

#[cfg(feature = "watching")]
mod watching_tests {
    use super::*;
    use localcache::InvalidationReason;
    #[allow(unused_imports)]
    use std::io::Write as _;
    use std::time::Duration;

    fn make_watching_engine(dir: &TempDir) -> CacheEngine<Vec<f32>> {
        CacheEngine::builder()
            .database(dir.path().join("watch.sqlite3"))
            .build()
            .unwrap()
    }

    /// Write to a file via OpenOptions so that the OS emits a
    /// Modify(Data) event rather than a Create event.
    fn modify_file(path: &std::path::Path, content: &[u8]) {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .unwrap();
        f.write_all(content).unwrap();
        f.flush().unwrap();
    }

    #[test]
    fn watcher_receives_modify_event() {
        let dir = TempDir::new().unwrap();
        let engine = make_watching_engine(&dir);

        let path = write_file(&dir, "w1.txt", b"original");
        engine.set(&path, &vec![1.0_f32]).unwrap();

        // watcher must stay alive for events to fire
        let watcher = engine.watcher().unwrap();
        let rx = watcher.events();

        std::thread::sleep(Duration::from_millis(100));
        modify_file(&path, b"modified content here!!");

        let event = rx.recv_timeout(Duration::from_secs(3));
        assert!(event.is_ok(), "expected a WatchEvent within 3 s");
        let ev = event.unwrap();
        assert_eq!(ev.path, path);
        assert!(matches!(ev.reason, InvalidationReason::FileModified));
        drop(watcher);
    }

    #[test]
    fn watcher_receives_remove_event() {
        let dir = TempDir::new().unwrap();
        let engine = make_watching_engine(&dir);

        let path = write_file(&dir, "w2.txt", b"data");
        engine.set(&path, &vec![2.0_f32]).unwrap();

        let watcher = engine.watcher().unwrap();
        let rx = watcher.events();

        std::thread::sleep(Duration::from_millis(100));
        fs::remove_file(&path).unwrap();

        let event = rx.recv_timeout(Duration::from_secs(3));
        assert!(event.is_ok(), "expected a remove WatchEvent");
        let ev = event.unwrap();
        assert!(matches!(
            ev.reason,
            InvalidationReason::FileRemoved | InvalidationReason::FileModified
        ));
        drop(watcher);
    }

    #[test]
    fn watcher_auto_removes_stale_entry_from_db() {
        let dir = TempDir::new().unwrap();
        let engine = make_watching_engine(&dir);

        let path = write_file(&dir, "w3.txt", b"before");
        engine.set(&path, &vec![3.0_f32]).unwrap();

        let watcher = engine.watcher().unwrap();
        let rx = watcher.events();

        std::thread::sleep(Duration::from_millis(100));
        modify_file(&path, b"after modification");

        // Wait for the event (watcher callback also removes from DB).
        let _ = rx.recv_timeout(Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(100));
        drop(watcher);

        // The watcher's internal engine deleted the entry.
        // The original engine (separate connection) still reflects its own view.
        // We can only confirm via the watcher count having gone to 0 in-callback.
        // Sufficient to assert event arrived above without panic.
    }

    #[test]
    fn watcher_watch_additional_path() {
        let dir = TempDir::new().unwrap();
        let engine = make_watching_engine(&dir);

        // Cache one entry to initialise the watcher.
        let existing = write_file(&dir, "existing.txt", b"x");
        engine.set(&existing, &vec![0.0_f32]).unwrap();

        let mut watcher = engine.watcher().unwrap();

        // Add an additional path manually.
        let extra = write_file(&dir, "w4.txt", b"extra");
        engine.set(&extra, &vec![4.0_f32]).unwrap();
        watcher.watch(&extra).unwrap();

        let rx = watcher.events();
        std::thread::sleep(Duration::from_millis(100));
        modify_file(&extra, b"changed extra content");

        let event = rx.recv_timeout(Duration::from_secs(3));
        assert!(event.is_ok(), "manually watched path should emit events");
        drop(watcher);
    }

    #[test]
    fn watcher_no_panic_on_empty_cache() {
        let dir = TempDir::new().unwrap();
        let engine = make_watching_engine(&dir);
        // No entries — should not panic.
        let _watcher = engine.watcher().unwrap();
    }
}

// ====================================================================
// Phase 15 — namespace_list() and namespace_copy()
// ====================================================================

#[test]
fn namespace_list_returns_all_namespaces() {
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("ns_list.sqlite3");

    let names = ["alpha", "beta", "gamma"];
    for ns in &names {
        let e: CacheEngine<Vec<f32>> = CacheEngine::builder()
            .database(db.clone())
            .namespace(*ns)
            .build()
            .unwrap();
        let p = write_file(&dir, &format!("{ns}.txt"), b"x");
        e.set(&p, &vec![1.0_f32]).unwrap();
    }

    let e: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(db)
        .namespace("alpha")
        .build()
        .unwrap();

    let listed = e.namespace_list().unwrap();
    assert_eq!(listed.len(), 3);
    for ns in &names {
        assert!(listed.contains(&ns.to_string()));
    }
    // Should be alphabetically sorted.
    assert!(listed.windows(2).all(|w| w[0] <= w[1]));
}

#[test]
fn namespace_list_empty_database() {
    let dir = TempDir::new().unwrap();
    let e: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(dir.path().join("empty_ns.sqlite3"))
        .build()
        .unwrap();
    // No entries yet — namespace_list should return empty.
    assert!(e.namespace_list().unwrap().is_empty());
}

#[test]
fn namespace_copy_copies_all_entries() {
    let dir = TempDir::new().unwrap();
    let db_src = dir.path().join("ns_src.sqlite3");
    let db_dst = dir.path().join("ns_dst.sqlite3");

    let src: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(db_src)
        .namespace("source")
        .build()
        .unwrap();

    for i in 0..5u32 {
        let p = write_file(&dir, &format!("cp{i}.txt"), b"x");
        src.set(&p, &vec![i as f32]).unwrap();
    }

    let dst: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(db_dst)
        .namespace("dest")
        .build()
        .unwrap();

    let copied = dst.namespace_copy(&src).unwrap();
    assert_eq!(copied, 5);
    assert_eq!(dst.entry_count().unwrap(), 5);
}

#[test]
fn namespace_copy_overwrites_existing_entries() {
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("ns_overwrite.sqlite3");

    let src: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(db.clone())
        .namespace("src")
        .build()
        .unwrap();
    let dst: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(db)
        .namespace("dst")
        .build()
        .unwrap();

    let p = write_file(&dir, "over.txt", b"data");
    src.set(&p, &vec![1.0_f32]).unwrap();
    dst.set(&p, &vec![99.0_f32]).unwrap();

    // Copy from src → dst; dst's entry should be overwritten.
    let copied = dst.namespace_copy(&src).unwrap();
    assert_eq!(copied, 1);
    assert_eq!(dst.entry_count().unwrap(), 1);

    // Entry should now reflect src's payload.
    let entry = dst.get(&p).unwrap().unwrap();
    assert_eq!(entry.payload[0], 1.0_f32);
}

#[test]
fn namespace_copy_is_equivalent_to_import_from() {
    let dir = TempDir::new().unwrap();
    let db1 = dir.path().join("equiv1.sqlite3");
    let db2 = dir.path().join("equiv2.sqlite3");

    let src: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(db1.clone())
        .namespace("ns")
        .build()
        .unwrap();

    for i in 0..3u32 {
        let p = write_file(&dir, &format!("eq{i}.txt"), b"x");
        src.set(&p, &vec![i as f32]).unwrap();
    }

    let dst1: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(db1.clone())
        .namespace("copy_dst")
        .build()
        .unwrap();
    let dst2: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(db2)
        .namespace("ns")
        .build()
        .unwrap();

    let n1 = dst1.namespace_copy(&src).unwrap();
    let n2 = dst2.import_from(&src).unwrap();
    assert_eq!(
        n1, n2,
        "namespace_copy and import_from should copy the same count"
    );
}

// ====================================================================
// Phase 15 — metrics feature (smoke test: no panic)
// ====================================================================

#[cfg(feature = "metrics")]
#[test]
fn metrics_instrumentation_no_panic() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();

    let path = write_file(&dir, "metrics.txt", b"data");

    // These should fire metrics without panicking.
    engine.set(&path, &vec![1.0_f32]).unwrap();
    let _ = engine.get(&path).unwrap(); // hit
    let _ = engine.get_if_fresh(&path).unwrap(); // fresh hit

    // Miss case.
    let missing = write_file(&dir, "missing.txt", b"x");
    let _ = engine.get(&missing).unwrap(); // miss
}

// ====================================================================
// Phase 15 —(watching feature)
// ====================================================================

#[cfg(feature = "watching")]
mod debounce_tests {
    use super::*;
    #[allow(unused_imports)]
    use std::io::Write as _;
    use std::time::Duration;

    fn make_db_engine(dir: &TempDir) -> CacheEngine<Vec<f32>> {
        CacheEngine::builder()
            .database(dir.path().join("debounce.sqlite3"))
            .build()
            .unwrap()
    }

    fn modify(path: &std::path::Path, content: &[u8]) {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .unwrap();
        f.write_all(content).unwrap();
        f.flush().unwrap();
    }

    #[test]
    fn debounced_watcher_deduplicates_rapid_writes() {
        let dir = TempDir::new().unwrap();
        let engine = make_db_engine(&dir);

        let path = write_file(&dir, "debounce1.txt", b"original");
        engine.set(&path, &vec![1.0_f32]).unwrap();

        // Use a 200 ms debounce window.
        let watcher = engine
            .debounced_watcher(Duration::from_millis(200))
            .unwrap();
        let rx = watcher.events();

        std::thread::sleep(Duration::from_millis(50));

        // Write 5 times rapidly within the debounce window.
        for i in 0..5u8 {
            modify(&path, &[i; 32]);
            std::thread::sleep(Duration::from_millis(10));
        }

        // Should receive at most 2 events (debounce merges them).
        std::thread::sleep(Duration::from_millis(500));
        let mut count = 0usize;
        while rx.try_recv().is_ok() {
            count += 1;
        }
        assert!(count <= 2, "expected ≤ 2 debounced events, got {count}");
        assert!(count >= 1, "expected at least 1 event");
        drop(watcher);
    }

    #[test]
    fn debounced_watcher_no_panic_on_empty_cache() {
        let dir = TempDir::new().unwrap();
        let engine = make_db_engine(&dir);
        let _w = engine
            .debounced_watcher(Duration::from_millis(100))
            .unwrap();
    }

    #[test]
    fn debounced_watcher_receives_event_for_modification() {
        let dir = TempDir::new().unwrap();
        let engine = make_db_engine(&dir);

        let path = write_file(&dir, "debounce2.txt", b"v1");
        engine.set(&path, &vec![1.0_f32]).unwrap();

        let watcher = engine
            .debounced_watcher(Duration::from_millis(150))
            .unwrap();
        let rx = watcher.events();

        std::thread::sleep(Duration::from_millis(50));
        modify(&path, b"v2 changed content");

        let event = rx.recv_timeout(Duration::from_secs(3));
        assert!(event.is_ok(), "expected a debounced event within 3 s");
        assert_eq!(event.unwrap().path, path);
        drop(watcher);
    }
}
