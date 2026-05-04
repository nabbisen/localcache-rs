//! Integration tests — builder_ops.

mod common;
use common::write_file;

use std::fs;

use tempfile::TempDir;

use localcache::{
    CacheEngine, CacheOptions, CacheStatus, ChangeDetectionMode, LocalFileCacheError, ScanOptions,
};

#[test]
fn builder_creates_working_engine() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(":memory:")
        .namespace("builder_test")
        .change_detection(ChangeDetectionMode::MetadataOnly)
        .max_entries(100)
        .payload_version(1)
        .build()
        .unwrap();

    let path = write_file(&dir, "built.txt", b"hello");
    engine.set(&path, &vec![1.0_f32, 2.0]).unwrap();
    let entry = engine.get(&path).unwrap().unwrap();
    assert_eq!(entry.payload, vec![1.0_f32, 2.0]);
}

#[test]
fn builder_with_ttl() {
    use std::time::Duration;
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(":memory:")
        .ttl(Duration::from_secs(0))
        .build()
        .unwrap();

    let path = write_file(&dir, "ttl_built.txt", b"x");
    engine.set(&path, &vec![1.0_f32]).unwrap();
    // TTL=0 → immediately stale.
    assert!(engine.get_if_fresh(&path).unwrap().is_none());
}

#[test]
fn builder_read_only() {
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("ro_builder.sqlite3");

    // Create DB first.
    CacheEngine::<Vec<f32>>::open(CacheOptions {
        database_path: db.clone(),
        ..CacheOptions::default()
    })
    .unwrap();

    let ro: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(db)
        .read_only()
        .build()
        .unwrap();

    let path = write_file(&dir, "ro_b.txt", b"x");
    assert!(matches!(
        ro.set(&path, &vec![1.0_f32]),
        Err(LocalFileCacheError::ReadOnly)
    ));
}

#[cfg(feature = "compression")]
#[test]
fn builder_compress() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(":memory:")
        .compress()
        .build()
        .unwrap();

    let path = write_file(&dir, "compress_b.txt", b"content");
    let payload: Vec<f32> = (0..100).map(|i| i as f32).collect();
    engine.set(&path, &payload).unwrap();
    assert_eq!(engine.get(&path).unwrap().unwrap().payload, payload);
}

// ====================================================================
// Phase 7 — CacheStats
// ====================================================================

#[test]
fn cache_stats_empty() {
    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();

    let stats = engine.cache_stats().unwrap();
    assert_eq!(stats.total_entries, 0);
    assert_eq!(stats.total_payload_bytes, 0);
    assert!(stats.oldest_updated_at.is_none());
    assert!(stats.newest_updated_at.is_none());
    assert!(stats.entries_by_encoding.is_empty());
}

#[test]
fn cache_stats_with_entries() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();

    let p1 = write_file(&dir, "s1.txt", b"a");
    let p2 = write_file(&dir, "s2.txt", b"b");
    engine.set(&p1, &vec![1.0_f32]).unwrap();
    engine.set(&p2, &vec![2.0_f32, 3.0]).unwrap();

    let stats = engine.cache_stats().unwrap();
    assert_eq!(stats.total_entries, 2);
    assert!(stats.total_payload_bytes > 0);
    assert!(stats.oldest_updated_at.is_some());
    assert!(stats.newest_updated_at.is_some());
    assert_eq!(stats.entries_by_encoding.len(), 1); // all "raw"
    assert_eq!(stats.entries_by_encoding[0].0, "raw");
    assert_eq!(stats.entries_by_encoding[0].1, 2);
}

#[test]
fn cache_stats_namespace_scoped() {
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("stats_ns.sqlite3");

    let e1: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(db.clone())
        .namespace("ns1")
        .build()
        .unwrap();
    let e2: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(db)
        .namespace("ns2")
        .build()
        .unwrap();

    let p = write_file(&dir, "sns.txt", b"x");
    e1.set(&p, &vec![1.0_f32]).unwrap();
    e1.set(&p, &vec![2.0_f32]).unwrap(); // upsert

    // ns1 has 1 entry; ns2 has 0.
    assert_eq!(e1.cache_stats().unwrap().total_entries, 1);
    assert_eq!(e2.cache_stats().unwrap().total_entries, 0);
}

// ====================================================================
// Phase 7 — check_status_batch
// ====================================================================

#[test]
fn check_status_batch_mixed() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();

    let p_fresh = write_file(&dir, "csb_fresh.txt", b"stable");
    let p_stale = write_file(&dir, "csb_stale.txt", b"original");
    let p_miss = write_file(&dir, "csb_miss.txt", b"x");

    engine.set(&p_fresh, &vec![1.0_f32]).unwrap();
    engine.set(&p_stale, &vec![2.0_f32]).unwrap();
    // p_miss intentionally not cached

    // Make p_stale stale.
    write_file(&dir, "csb_stale.txt", b"modified content!!");

    let statuses = engine.check_status_batch(&[p_fresh.clone(), p_stale.clone(), p_miss.clone()]);
    assert_eq!(statuses.len(), 3);
    assert_eq!(statuses[0].as_ref().unwrap(), &CacheStatus::Fresh);
    assert_eq!(statuses[1].as_ref().unwrap(), &CacheStatus::Stale);
    assert_eq!(statuses[2].as_ref().unwrap(), &CacheStatus::Missing);
}

#[test]
fn check_status_batch_empty_input() {
    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();
    let results = engine.check_status_batch::<std::path::PathBuf>(&[]);
    assert!(results.is_empty());
}

// ====================================================================
// Phase 7 — Key rotation
// ====================================================================

#[cfg(feature = "encryption")]
mod rotation_tests {
    use super::*;

    fn key(seed: u8) -> Vec<u8> {
        vec![seed; 32]
    }

    #[test]
    fn rotate_key_re_encrypts_all_entries() {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("rot.sqlite3");

        // Write with key A.
        {
            let e: CacheEngine<Vec<f32>> = CacheEngine::builder()
                .database(db.clone())
                .encryption_key(key(0xAA))
                .build()
                .unwrap();

            for i in 0..3u32 {
                let p = write_file(&dir, &format!("rot{i}.txt"), b"data");
                e.set(&p, &vec![i as f32]).unwrap();
            }
        }

        // Rotate key A → key B.
        {
            let e: CacheEngine<Vec<f32>> = CacheEngine::builder()
                .database(db.clone())
                .encryption_key(key(0xAA))
                .build()
                .unwrap();

            let rotated = e.rotate_encryption_key(&key(0xBB)).unwrap();
            assert_eq!(rotated, 3);
        }

        // Re-open with key B — must read successfully.
        let e_b: CacheEngine<Vec<f32>> = CacheEngine::builder()
            .database(db)
            .encryption_key(key(0xBB))
            .build()
            .unwrap();

        for i in 0..3u32 {
            let p = dir.path().join(format!("rot{i}.txt"));
            let entry = e_b.get(&p).unwrap().expect("entry must exist");
            assert_eq!(entry.payload, vec![i as f32]);
        }
    }

    #[test]
    fn rotate_key_old_key_no_longer_decrypts() {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("rot2.sqlite3");

        let p = write_file(&dir, "rot2.txt", b"secret");

        {
            let e: CacheEngine<Vec<f32>> = CacheEngine::builder()
                .database(db.clone())
                .encryption_key(key(0x11))
                .build()
                .unwrap();
            e.set(&p, &vec![1.0_f32]).unwrap();
            e.rotate_encryption_key(&key(0x22)).unwrap();
        }

        // Old key (0x11) must fail.
        let old: CacheEngine<Vec<f32>> = CacheEngine::builder()
            .database(db)
            .encryption_key(key(0x11))
            .build()
            .unwrap();

        assert!(old.get(&p).is_err(), "old key should no longer work");
    }

    #[test]
    fn rotate_returns_zero_when_no_encrypted_entries() {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("rot3.sqlite3");

        // Write unencrypted entries.
        {
            let e: CacheEngine<Vec<f32>> =
                CacheEngine::builder().database(db.clone()).build().unwrap();
            let p = write_file(&dir, "rot3.txt", b"plain");
            e.set(&p, &vec![1.0_f32]).unwrap();
        }

        // Rotating with key = Some but no encrypted entries → 0 rotated.
        let e: CacheEngine<Vec<f32>> = CacheEngine::builder()
            .database(db)
            .encryption_key(key(0x33))
            .build()
            .unwrap();

        let rotated = e.rotate_encryption_key(&key(0x44)).unwrap();
        assert_eq!(rotated, 0);
    }
}

// ====================================================================
// Phase 7 — Glob brace expansion
// ====================================================================

#[test]
fn glob_brace_expansion_basic() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("brace");
    fs::create_dir(&root).unwrap();

    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();

    let txt = {
        let p = root.join("doc.txt");
        fs::write(&p, b"t").unwrap();
        p
    };
    let md = {
        let p = root.join("doc.md");
        fs::write(&p, b"m").unwrap();
        p
    };
    let rs = {
        let p = root.join("doc.rs");
        fs::write(&p, b"r").unwrap();
        p
    };

    // Only .txt and .md should match.
    let opts = ScanOptions {
        recursive: false,
        glob_pattern: Some("*.{txt,md}".into()),
        ..ScanOptions::default()
    };
    let results = engine.scan_dir_filtered(&root, opts).unwrap();
    let paths: Vec<_> = results.iter().map(|(p, _)| p.clone()).collect();

    assert_eq!(paths.len(), 2);
    assert!(paths.contains(&txt));
    assert!(paths.contains(&md));
    assert!(!paths.contains(&rs));
}

#[test]
fn glob_brace_three_alternatives() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("brace3");
    fs::create_dir(&root).unwrap();

    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();

    let exts = ["txt", "md", "rst", "py"];
    for ext in exts {
        fs::write(root.join(format!("file.{ext}")), b"x").unwrap();
    }

    let opts = ScanOptions {
        recursive: false,
        glob_pattern: Some("*.{txt,md,rst}".into()),
        ..ScanOptions::default()
    };
    let results = engine.scan_dir_filtered(&root, opts).unwrap();
    // Should match txt, md, rst but not py.
    assert_eq!(results.len(), 3);
}

#[test]
fn glob_no_braces_unchanged() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("nobrace");
    fs::create_dir(&root).unwrap();

    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();

    fs::write(root.join("a.txt"), b"a").unwrap();
    fs::write(root.join("b.txt"), b"b").unwrap();
    fs::write(root.join("c.md"), b"c").unwrap();

    let opts = ScanOptions {
        recursive: false,
        glob_pattern: Some("*.txt".into()),
        ..ScanOptions::default()
    };
    let results = engine.scan_dir_filtered(&root, opts).unwrap();
    assert_eq!(results.len(), 2);
}

// ====================================================================
// Phase 7 — Async builder / cache_stats / check_status_batch
// ====================================================================

#[cfg(feature = "async")]
mod async_phase7_tests {
    use super::*;
    use localcache::{AsyncCacheEngine, CacheStats};

    #[tokio::test]
    async fn async_builder_opens_engine() {
        let dir = TempDir::new().unwrap();
        // AsyncCacheEngine::open uses CacheOptions directly.
        let engine: AsyncCacheEngine<Vec<f32>> = AsyncCacheEngine::open(CacheOptions {
            database_path: ":memory:".into(),
            ..CacheOptions::default()
        })
        .await
        .unwrap();

        let path = write_file(&dir, "ab.txt", b"hello");
        engine.set(path.clone(), vec![1.0_f32]).await.unwrap();
        assert!(engine.get(path).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn async_cache_stats() {
        let dir = TempDir::new().unwrap();
        let engine: AsyncCacheEngine<Vec<f32>> = AsyncCacheEngine::open(CacheOptions {
            database_path: ":memory:".into(),
            ..CacheOptions::default()
        })
        .await
        .unwrap();

        let p = write_file(&dir, "as.txt", b"x");
        engine.set(p, vec![1.0_f32, 2.0]).await.unwrap();

        let stats: CacheStats = engine.cache_stats().await.unwrap();
        assert_eq!(stats.total_entries, 1);
        assert!(stats.total_payload_bytes > 0);
    }

    #[tokio::test]
    async fn async_check_status_batch() {
        let dir = TempDir::new().unwrap();
        let engine: AsyncCacheEngine<Vec<f32>> = AsyncCacheEngine::open(CacheOptions {
            database_path: ":memory:".into(),
            ..CacheOptions::default()
        })
        .await
        .unwrap();

        let p = write_file(&dir, "acsbat.txt", b"x");
        engine.set(p.clone(), vec![1.0_f32]).await.unwrap();

        let results = engine.check_status_batch(vec![p]).await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].as_ref().unwrap(), &CacheStatus::Fresh);
    }
}

// ====================================================================
// Phase 8 — on_evict callback
// ====================================================================

#[test]
fn on_evict_called_when_entry_removed() {
    use std::sync::{Arc, Mutex};
    let dir = TempDir::new().unwrap();

    let evicted: Arc<Mutex<Vec<std::path::PathBuf>>> = Arc::new(Mutex::new(Vec::new()));
    let evicted_clone = Arc::clone(&evicted);

    let engine: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(":memory:")
        .max_entries(2)
        .on_evict(move |p| {
            evicted_clone.lock().unwrap().push(p.to_path_buf());
        })
        .build()
        .unwrap();

    let p1 = write_file(&dir, "ev1.txt", b"a");
    let p2 = write_file(&dir, "ev2.txt", b"b");
    let p3 = write_file(&dir, "ev3.txt", b"c");

    engine.set(&p1, &vec![1.0_f32]).unwrap();
    engine.set(&p2, &vec![2.0_f32]).unwrap();
    engine.set(&p3, &vec![3.0_f32]).unwrap(); // evicts p1

    let evicted_list = evicted.lock().unwrap().clone();
    assert_eq!(evicted_list.len(), 1, "one entry should have been evicted");
}

#[test]
fn on_evict_not_called_when_under_limit() {
    use std::sync::{Arc, Mutex};
    let dir = TempDir::new().unwrap();

    let evicted: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
    let evicted_clone = Arc::clone(&evicted);

    let engine: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(":memory:")
        .max_entries(10)
        .on_evict(move |_| {
            *evicted_clone.lock().unwrap() += 1;
        })
        .build()
        .unwrap();

    for i in 0..3u32 {
        let p = write_file(&dir, &format!("nev{i}.txt"), b"x");
        engine.set(&p, &vec![i as f32]).unwrap();
    }

    assert_eq!(*evicted.lock().unwrap(), 0);
}

#[test]
fn on_evict_callback_via_builder_without_max_entries_never_fires() {
    use std::sync::{Arc, Mutex};
    let dir = TempDir::new().unwrap();
    let fired = Arc::new(Mutex::new(false));
    let fired_clone = Arc::clone(&fired);

    let engine: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(":memory:")
        // no max_entries — eviction never happens
        .on_evict(move |_| {
            *fired_clone.lock().unwrap() = true;
        })
        .build()
        .unwrap();

    for i in 0..20u32 {
        let p = write_file(&dir, &format!("nf{i}.txt"), b"x");
        engine.set(&p, &vec![i as f32]).unwrap();
    }

    assert!(!*fired.lock().unwrap());
}

// ====================================================================
// Phase 8 — Multi-group glob expansion
// ====================================================================

#[test]
fn glob_two_brace_groups_cartesian_product() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("multi_brace");
    fs::create_dir(&root).unwrap();

    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();

    // Create pre_a.txt, pre_b.txt, post_a.txt, post_b.txt, other.txt
    let pre_a = {
        let p = root.join("pre_a.txt");
        fs::write(&p, b"").unwrap();
        p
    };
    let pre_b = {
        let p = root.join("pre_b.txt");
        fs::write(&p, b"").unwrap();
        p
    };
    let post_a = {
        let p = root.join("post_a.txt");
        fs::write(&p, b"").unwrap();
        p
    };
    let post_b = {
        let p = root.join("post_b.txt");
        fs::write(&p, b"").unwrap();
        p
    };
    let other = {
        let p = root.join("other.txt");
        fs::write(&p, b"").unwrap();
        p
    };

    // "{pre,post}_{a,b}.txt" → 4 combinations
    let opts = ScanOptions {
        recursive: false,
        glob_pattern: Some("{pre,post}_{a,b}.txt".into()),
        ..ScanOptions::default()
    };
    let results = engine.scan_dir_filtered(&root, opts).unwrap();
    let paths: Vec<_> = results.iter().map(|(p, _)| p.clone()).collect();

    assert_eq!(paths.len(), 4, "should match exactly 4 files");
    assert!(paths.contains(&pre_a));
    assert!(paths.contains(&pre_b));
    assert!(paths.contains(&post_a));
    assert!(paths.contains(&post_b));
    assert!(!paths.contains(&other));
}

#[test]
fn glob_three_alternatives_multi_group() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("three_groups");
    fs::create_dir(&root).unwrap();

    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();

    // "data_{a,b,c}.{txt,csv}" → 6 combinations
    for name in &[
        "data_a.txt",
        "data_b.txt",
        "data_c.txt",
        "data_a.csv",
        "data_b.csv",
        "data_c.csv",
        "info.txt",
    ] {
        fs::write(root.join(name), b"").unwrap();
    }

    let opts = ScanOptions {
        recursive: false,
        glob_pattern: Some("data_{a,b,c}.{txt,csv}".into()),
        ..ScanOptions::default()
    };
    let results = engine.scan_dir_filtered(&root, opts).unwrap();
    assert_eq!(results.len(), 6, "should match 6 data files");
}

#[test]
fn glob_nested_single_still_works() {
    // Single brace group should still function correctly.
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("single_brace2");
    fs::create_dir(&root).unwrap();

    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();

    fs::write(root.join("a.txt"), b"").unwrap();
    fs::write(root.join("a.md"), b"").unwrap();
    fs::write(root.join("a.rs"), b"").unwrap();

    let opts = ScanOptions {
        recursive: false,
        glob_pattern: Some("*.{txt,md}".into()),
        ..ScanOptions::default()
    };
    let results = engine.scan_dir_filtered(&root, opts).unwrap();
    assert_eq!(results.len(), 2);
}

// ====================================================================
// Phase 9 — Nested brace expansion
// ====================================================================
