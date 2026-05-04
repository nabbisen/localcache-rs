//! Integration tests — pool_observe.

mod common;
use common::write_file;

#[allow(unused_imports)]
use std::time::Duration;

use tempfile::TempDir;

use localcache::{CacheEngine, CacheOptions, CacheStatus};

// ====================================================================

#[test]
fn pool_basic_set_and_get() {
    let dir = TempDir::new().unwrap();
    let pool = localcache::ConnectionPool::<Vec<f32>>::open(CacheOptions {
        database_path: ":memory:".into(),
        ..CacheOptions::default()
    })
    .unwrap();

    let path = write_file(&dir, "pool.txt", b"content");
    pool.set(&path, &vec![1.0_f32, 2.0]).unwrap();
    let entry = pool.get(&path).unwrap().unwrap();
    assert_eq!(entry.payload, vec![1.0_f32, 2.0]);
}

#[test]
fn pool_clone_shares_engine() {
    let dir = TempDir::new().unwrap();
    let pool = localcache::ConnectionPool::<Vec<f32>>::open(CacheOptions {
        database_path: ":memory:".into(),
        ..CacheOptions::default()
    })
    .unwrap();

    let pool2 = pool.clone();
    let path = write_file(&dir, "shared.txt", b"x");
    pool.set(&path, &vec![9.0_f32]).unwrap();

    // Clone sees the entry.
    assert!(pool2.get(&path).unwrap().is_some());
}

#[test]
fn pool_multithreaded_access() {
    use std::sync::Arc;
    use std::thread;

    let dir = Arc::new(TempDir::new().unwrap());
    let pool = Arc::new(
        localcache::ConnectionPool::<Vec<f32>>::open(CacheOptions {
            database_path: ":memory:".into(),
            ..CacheOptions::default()
        })
        .unwrap(),
    );

    let handles: Vec<_> = (0..4)
        .map(|tid| {
            let pool = Arc::clone(&pool);
            let dir = Arc::clone(&dir);
            thread::spawn(move || {
                let p = write_file(&dir, &format!("t{tid}.txt"), b"data");
                pool.set(&p, &vec![tid as f32]).unwrap();
                let _ = pool.get(&p).unwrap();
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(pool.entry_count().unwrap(), 4);
}

#[test]
fn pool_get_if_fresh_and_check_status() {
    let dir = TempDir::new().unwrap();
    let pool = localcache::ConnectionPool::<Vec<f32>>::open(CacheOptions {
        database_path: ":memory:".into(),
        ..CacheOptions::default()
    })
    .unwrap();

    let path = write_file(&dir, "pf.txt", b"stable");
    pool.set(&path, &vec![1.0_f32]).unwrap();

    assert!(pool.get_if_fresh(&path).unwrap().is_some());
    assert_eq!(pool.check_status(&path).unwrap(), CacheStatus::Fresh);
}

#[test]
fn pool_entry_count_and_stats() {
    let dir = TempDir::new().unwrap();
    let pool = localcache::ConnectionPool::<Vec<f32>>::open(CacheOptions {
        database_path: ":memory:".into(),
        ..CacheOptions::default()
    })
    .unwrap();

    for i in 0..3u32 {
        let p = write_file(&dir, &format!("pec{i}.txt"), b"x");
        pool.set(&p, &vec![i as f32]).unwrap();
    }

    assert_eq!(pool.entry_count().unwrap(), 3);
    let stats = pool.cache_stats().unwrap();
    assert_eq!(stats.total_entries, 3);
    assert!(stats.total_payload_bytes > 0);
}

#[test]
fn pool_remove() {
    let dir = TempDir::new().unwrap();
    let pool = localcache::ConnectionPool::<Vec<f32>>::open(CacheOptions {
        database_path: ":memory:".into(),
        ..CacheOptions::default()
    })
    .unwrap();

    let path = write_file(&dir, "prm.txt", b"x");
    pool.set(&path, &vec![1.0_f32]).unwrap();
    assert!(pool.remove(&path).unwrap());
    assert!(pool.get(&path).unwrap().is_none());
}

#[test]
fn pool_query_run() {
    let dir = TempDir::new().unwrap();
    let pool = localcache::ConnectionPool::<Vec<f32>>::open(CacheOptions {
        database_path: ":memory:".into(),
        ..CacheOptions::default()
    })
    .unwrap();

    for i in 0..5u32 {
        let p = write_file(&dir, &format!("pq{i}.txt"), b"x");
        pool.set(&p, &vec![i as f32]).unwrap();
    }

    // Path-like filter (no json feature needed).
    let results = pool.query_run(|q| q.path_like("%pq%.txt")).unwrap();
    assert_eq!(results.len(), 5);
}

// ====================================================================
// Phase 12 — CacheOptionsExt
// ====================================================================

#[test]
fn cache_options_ext_ttl_helpers() {
    use localcache::CacheOptionsExt as _;

    let opts_secs = CacheOptions::default().with_ttl_secs(120);
    assert_eq!(opts_secs.ttl, Some(Duration::from_secs(120)));

    let opts_mins = CacheOptions::default().with_ttl_mins(5);
    assert_eq!(opts_mins.ttl, Some(Duration::from_secs(300)));

    let opts_hours = CacheOptions::default().with_ttl_hours(2);
    assert_eq!(opts_hours.ttl, Some(Duration::from_secs(7200)));
}

#[test]
fn shared_engine_helper() {
    let shared = localcache::shared_engine::<Vec<f32>>(CacheOptions {
        database_path: ":memory:".into(),
        ..CacheOptions::default()
    })
    .unwrap();

    let dir = TempDir::new().unwrap();
    let p = write_file(&dir, "se.txt", b"x");
    let _ = shared.lock().unwrap().set(&p, &vec![1.0_f32]);
    assert!(shared.lock().unwrap().get(&p).unwrap().is_some());
}

// ====================================================================
// Phase 13 — explain() /// ====================================================================

#[test]
fn explain_fresh_entry() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();

    let path = write_file(&dir, "diag_fresh.txt", b"content");
    engine.set(&path, &vec![1.0_f32]).unwrap();

    let diag = engine.explain(&path).unwrap();
    assert_eq!(diag.status, CacheStatus::Fresh);
    assert!(diag.entry_exists);
    assert!(diag.file_exists);
    assert!(diag.summary.contains("fresh") || diag.summary.contains("Fresh"));
}

#[test]
fn explain_missing_entry() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();

    let path = write_file(&dir, "diag_miss.txt", b"x");
    // Never set — no entry.
    let diag = engine.explain(&path).unwrap();
    assert_eq!(diag.status, CacheStatus::Missing);
    assert!(!diag.entry_exists);
    assert!(diag.file_exists);
}

#[test]
fn explain_missing_file_on_disk() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();

    let path = write_file(&dir, "diag_gone.txt", b"bye");
    engine.set(&path, &vec![1.0_f32]).unwrap();
    std::fs::remove_file(&path).unwrap();

    let diag = engine.explain(&path).unwrap();
    assert_eq!(diag.status, CacheStatus::Missing);
    assert!(!diag.file_exists);
}

#[test]
fn explain_stale_metadata() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();

    let path = write_file(&dir, "diag_stale.txt", b"original");
    engine.set(&path, &vec![1.0_f32]).unwrap();
    write_file(&dir, "diag_stale.txt", b"different content now");

    let diag = engine.explain(&path).unwrap();
    assert_eq!(diag.status, CacheStatus::Stale);
    assert!(diag.metadata_diff.is_some());
    let diff = diag.metadata_diff.unwrap();
    assert!(diff.mtime_changed || diff.size_changed);
}

#[test]
fn explain_ttl_expired() {
    #[allow(unused_imports)]
    use std::time::Duration;
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(":memory:")
        .ttl(Duration::from_secs(0))
        .build()
        .unwrap();

    let path = write_file(&dir, "diag_ttl.txt", b"content");
    engine.set(&path, &vec![1.0_f32]).unwrap();

    let diag = engine.explain(&path).unwrap();
    assert_eq!(diag.status, CacheStatus::Stale);
    assert_eq!(diag.ttl_remaining_secs, Some(0));
    assert!(diag.summary.to_lowercase().contains("ttl") || diag.summary.contains("expired"));
}

#[test]
fn explain_payload_version_mismatch() {
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("diag_pv.sqlite3");

    // Write with version 1.
    {
        let e: CacheEngine<Vec<f32>> = CacheEngine::builder()
            .database(db.clone())
            .payload_version(1)
            .build()
            .unwrap();
        let p = write_file(&dir, "diag_pv.txt", b"x");
        e.set(&p, &vec![1.0_f32]).unwrap();
    }

    // Explain with version 2.
    let engine: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(db)
        .payload_version(2)
        .build()
        .unwrap();

    let path = dir.path().join("diag_pv.txt");
    let diag = engine.explain(&path).unwrap();
    assert_eq!(diag.status, CacheStatus::Stale);
    assert!(diag.payload_version.is_some());
    let pv = diag.payload_version.unwrap();
    assert_eq!(pv.stored, 1);
    assert_eq!(pv.expected, 2);
    assert!(!pv.matches);
}

// ====================================================================
// Phase 13 — order_by_last_accessed + multi-column sort
// ====================================================================

#[test]
fn order_by_last_accessed_ascending() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();

    for i in 0..4u32 {
        let p = write_file(&dir, &format!("laa{i}.txt"), b"x");
        engine.set(&p, &vec![i as f32]).unwrap();
    }

    // Touch entries 0 and 2 (in that order) so they have non-zero last_accessed_at.
    let p0 = dir.path().join("laa0.txt");
    let p2 = dir.path().join("laa2.txt");
    engine.get(&p0).unwrap();
    // Small sleep to ensure distinct timestamps — use std::thread::sleep.
    std::thread::sleep(std::time::Duration::from_millis(10));
    engine.get(&p2).unwrap();

    // Ascending: never-accessed (0) → p0 (older access) → p2 (newer access)
    let results = engine.query().order_by_last_accessed(true).run().unwrap();
    assert_eq!(results.len(), 4);

    // Entries 1 and 3 were never read, so last_accessed_at == 0 (they come first).
    let first_two_laas: std::collections::HashSet<_> = results[0..2]
        .iter()
        .map(|e| e.path.file_name().unwrap().to_str().unwrap().to_owned())
        .collect();
    assert!(first_two_laas.contains("laa1.txt"));
    assert!(first_two_laas.contains("laa3.txt"));
}

#[cfg(feature = "json")]
#[test]
fn multi_column_sort_field_then_path() {
    use localcache::Codec;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Clone)]
    struct Item {
        group: u32,
    }

    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Item> = CacheEngine::builder()
        .database(":memory:")
        .codec(Codec::Json)
        .build()
        .unwrap();

    // Two items with group=1, two with group=2.
    for (name, group) in [("a1", 1u32), ("b1", 1), ("a2", 2), ("b2", 2)] {
        let p = write_file(&dir, &format!("{name}.txt"), b"x");
        engine.set(&p, &Item { group }).unwrap();
    }

    // Sort: group ASC, then path DESC within same group.
    let results = engine
        .query()
        .order_by_field("group", true)
        .then_by_path(false)
        .run()
        .unwrap();

    assert_eq!(results.len(), 4);

    // First two should be group=1, sorted by path descending → b1, a1
    assert_eq!(results[0].payload.group, 1);
    assert_eq!(results[1].payload.group, 1);
    // Path desc: b1 before a1 (b > a).
    let p0 = results[0].path.file_stem().unwrap().to_str().unwrap();
    let p1 = results[1].path.file_stem().unwrap().to_str().unwrap();
    assert!(p0 > p1, "expected descending path within group 1");
}

#[test]
fn then_by_methods_append_sort_keys() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();

    for i in 0..3u32 {
        let p = write_file(&dir, &format!("tb{i}.txt"), b"x");
        engine.set(&p, &vec![i as f32]).unwrap();
    }

    // order_by_path ASC then then_by_last_accessed DESC — just verify no panic.
    let results = engine
        .query()
        .order_by_path(true)
        .then_by_last_accessed(false)
        .then_by_updated_at(true)
        .run()
        .unwrap();

    assert_eq!(results.len(), 3);
    // Should be sorted by path ascending.
    for w in results.windows(2) {
        assert!(w[0].path <= w[1].path);
    }
}

// ====================================================================
// Phase 13 — tracing feature (smoke test: no panic)
// ====================================================================

#[cfg(feature = "tracing")]
#[test]
fn tracing_feature_no_panic() {
    let dir = TempDir::new().unwrap();
    // Just verify that with tracing enabled, operations complete without error.
    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();

    let path = write_file(&dir, "tr.txt", b"data");
    engine.set(&path, &vec![1.0_f32]).unwrap();
    let _ = engine.get(&path).unwrap();
    let _ = engine.get_if_fresh(&path).unwrap();
    let _ = engine.check_status(&path).unwrap();
}

// ====================================================================
// Phase 13 — async explain
// ====================================================================

#[cfg(feature = "async")]
mod async_phase13_tests {
    use super::*;
    use localcache::AsyncCacheEngine;

    #[tokio::test]
    async fn async_explain_fresh() {
        let dir = TempDir::new().unwrap();
        let engine: AsyncCacheEngine<Vec<f32>> = AsyncCacheEngine::open(CacheOptions {
            database_path: ":memory:".into(),
            ..CacheOptions::default()
        })
        .await
        .unwrap();

        let path = write_file(&dir, "ae_diag.txt", b"content");
        engine.set(path.clone(), vec![1.0_f32]).await.unwrap();

        let diag = engine.explain(path).await.unwrap();
        assert_eq!(diag.status, CacheStatus::Fresh);
        assert!(diag.entry_exists);
    }
}

// ====================================================================
// Phase 14 — preload()
