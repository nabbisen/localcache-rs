//! Integration tests — codec_lru.

mod common;
use common::write_file;

use std::fs;

use tempfile::TempDir;

use localcache::{CacheEngine, CacheOptions, CacheStatus, ScanOptions};

#[cfg(feature = "json")]
mod json_tests {
    use super::*;
    use localcache::Codec;

    #[test]
    fn json_codec_roundtrip() {
        let dir = TempDir::new().unwrap();
        let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: ":memory:".into(),
            codec: Codec::Json,
            ..CacheOptions::default()
        })
        .unwrap();

        let path = write_file(&dir, "json.txt", b"content");
        let payload = vec![1.1_f32, 2.2, 3.3];
        engine.set(&path, &payload).unwrap();
        let entry = engine.get(&path).unwrap().unwrap();
        assert!((entry.payload[0] - 1.1_f32).abs() < 1e-5);
    }

    #[test]
    fn json_entry_is_fresh() {
        let dir = TempDir::new().unwrap();
        let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: ":memory:".into(),
            codec: Codec::Json,
            ..CacheOptions::default()
        })
        .unwrap();

        let path = write_file(&dir, "json_fresh.txt", b"stable");
        engine.set(&path, &vec![1.0_f32]).unwrap();
        assert_eq!(engine.check_status(&path).unwrap(), CacheStatus::Fresh);
        assert!(engine.get_if_fresh(&path).unwrap().is_some());
    }

    #[test]
    fn json_and_bincode_coexist_in_same_db() {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("mixed_codec.sqlite3");

        let engine_bincode: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: db.clone(),
            codec: Codec::Bincode,
            namespace: "bin".to_owned(),
            ..CacheOptions::default()
        })
        .unwrap();

        let engine_json: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: db,
            codec: Codec::Json,
            namespace: "json".to_owned(),
            ..CacheOptions::default()
        })
        .unwrap();

        let path = write_file(&dir, "mixed_codec.txt", b"hello");
        let payload = vec![5.0_f32, 6.0, 7.0];

        engine_bincode.set(&path, &payload).unwrap();
        engine_json.set(&path, &payload).unwrap();

        let e1 = engine_bincode.get(&path).unwrap().unwrap();
        let e2 = engine_json.get(&path).unwrap().unwrap();
        assert_eq!(e1.payload, payload);
        assert_eq!(e2.payload, payload);
    }

    #[cfg(feature = "compression")]
    #[test]
    fn json_zstd_roundtrip() {
        let dir = TempDir::new().unwrap();
        let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: ":memory:".into(),
            codec: Codec::Json,
            compress_payloads: true,
            ..CacheOptions::default()
        })
        .unwrap();

        let path = write_file(&dir, "jz.txt", b"content");
        let payload: Vec<f32> = (0..200).map(|i| i as f32).collect();
        engine.set(&path, &payload).unwrap();
        let entry = engine.get(&path).unwrap().unwrap();
        assert_eq!(entry.payload.len(), 200);
    }
}

// ====================================================================
// Phase 5 — max_entries (LRU eviction)
// ====================================================================

#[test]
fn max_entries_evicts_oldest() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: ":memory:".into(),
        max_entries: Some(2),
        ..CacheOptions::default()
    })
    .unwrap();

    let p1 = write_file(&dir, "me1.txt", b"a");
    let p2 = write_file(&dir, "me2.txt", b"b");
    let p3 = write_file(&dir, "me3.txt", b"c");

    engine.set(&p1, &vec![1.0_f32]).unwrap();
    engine.set(&p2, &vec![2.0_f32]).unwrap();
    engine.set(&p3, &vec![3.0_f32]).unwrap(); // should evict p1

    // Total entries must be ≤ max_entries.
    assert!(engine.entry_count().unwrap() <= 2);

    // p3 (most recently written) must survive.
    assert!(engine.get(&p3).unwrap().is_some());
}

#[test]
fn max_entries_does_not_evict_when_within_limit() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: ":memory:".into(),
        max_entries: Some(5),
        ..CacheOptions::default()
    })
    .unwrap();

    for i in 0..4u32 {
        let p = write_file(&dir, &format!("ne{i}.txt"), b"x");
        engine.set(&p, &vec![i as f32]).unwrap();
    }

    assert_eq!(engine.entry_count().unwrap(), 4);
}

#[test]
fn batch_set_respects_max_entries() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: ":memory:".into(),
        max_entries: Some(2),
        ..CacheOptions::default()
    })
    .unwrap();

    let items: Vec<_> = (0..5u32)
        .map(|i| {
            let p = write_file(&dir, &format!("bs{i}.txt"), b"x");
            (p, vec![i as f32])
        })
        .collect();

    engine.batch_set(&items).unwrap();
    assert!(engine.entry_count().unwrap() <= 2);
}

// ====================================================================
// Phase 5 — scan_dir_filtered (extension filter + max_depth)
// ====================================================================

#[test]
fn scan_dir_filtered_by_extension() {
    let dir = TempDir::new().unwrap();
    let scan_root = dir.path().join("ext_filter");
    fs::create_dir(&scan_root).unwrap();

    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: ":memory:".into(),
        ..CacheOptions::default()
    })
    .unwrap();

    let txt = {
        let p = scan_root.join("a.txt");
        fs::write(&p, b"text").unwrap();
        p
    };
    let md = {
        let p = scan_root.join("b.md");
        fs::write(&p, b"markdown").unwrap();
        p
    };
    let rs = {
        let p = scan_root.join("c.rs");
        fs::write(&p, b"rust").unwrap();
        p
    };

    engine.set(&txt, &vec![1.0_f32]).unwrap();
    engine.set(&md, &vec![2.0_f32]).unwrap();
    engine.set(&rs, &vec![3.0_f32]).unwrap();

    let opts = ScanOptions {
        recursive: false,
        extensions: vec!["txt".into(), "md".into()],
        ..ScanOptions::default()
    };
    let results = engine.scan_dir_filtered(&scan_root, opts).unwrap();

    // Only txt and md should appear.
    assert_eq!(results.len(), 2);
    let paths: Vec<_> = results.iter().map(|(p, _)| p.clone()).collect();
    assert!(paths.iter().any(|p| p == &txt));
    assert!(paths.iter().any(|p| p == &md));
    assert!(!paths.iter().any(|p| p == &rs));
}

#[test]
fn scan_dir_filtered_max_depth() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("depth_root");
    let level1 = root.join("l1");
    let level2 = level1.join("l2");
    fs::create_dir_all(&level2).unwrap();

    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: ":memory:".into(),
        ..CacheOptions::default()
    })
    .unwrap();

    let f0 = {
        let p = root.join("f0.txt");
        fs::write(&p, b"0").unwrap();
        p
    };
    let f1 = {
        let p = level1.join("f1.txt");
        fs::write(&p, b"1").unwrap();
        p
    };
    let f2 = {
        let p = level2.join("f2.txt");
        fs::write(&p, b"2").unwrap();
        p
    };

    // max_depth=1 means root + one level → f0 and f1 only.
    let opts = ScanOptions {
        recursive: true,
        max_depth: Some(1),
        ..ScanOptions::default()
    };
    let results = engine.scan_dir_filtered(&root, opts).unwrap();
    assert_eq!(results.len(), 2);
    let paths: Vec<_> = results.iter().map(|(p, _)| p.clone()).collect();
    assert!(paths.contains(&f0));
    assert!(paths.contains(&f1));
    assert!(!paths.contains(&f2));
}

#[test]
fn scan_dir_filtered_extension_case_insensitive() {
    let dir = TempDir::new().unwrap();
    let scan_root = dir.path().join("case_ext");
    fs::create_dir(&scan_root).unwrap();

    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: ":memory:".into(),
        ..CacheOptions::default()
    })
    .unwrap();

    let upper = {
        let p = scan_root.join("IMAGE.PNG");
        fs::write(&p, b"img").unwrap();
        p
    };
    let lower = {
        let p = scan_root.join("photo.png");
        fs::write(&p, b"img2").unwrap();
        p
    };

    let opts = ScanOptions {
        recursive: false,
        extensions: vec!["PNG".into()], // uppercase filter
        ..ScanOptions::default()
    };
    let results = engine.scan_dir_filtered(&scan_root, opts).unwrap();
    let paths: Vec<_> = results.iter().map(|(p, _)| p.clone()).collect();
    assert!(paths.contains(&upper));
    assert!(paths.contains(&lower)); // both match case-insensitively
}

// ====================================================================
// Phase 5 — purge_stale_versions / entry_count_by_version
// ====================================================================

#[test]
fn purge_stale_versions_removes_old_entries() {
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("purge.sqlite3");

    // Write 3 entries with version 1.
    {
        let e: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: db.clone(),
            payload_version: 1,
            ..CacheOptions::default()
        })
        .unwrap();
        for i in 0..3u32 {
            let p = write_file(&dir, &format!("pv{i}.txt"), b"x");
            e.set(&p, &vec![i as f32]).unwrap();
        }
    }

    // Open with version 2 and write 1 new entry.
    let e2: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: db.clone(),
        payload_version: 2,
        ..CacheOptions::default()
    })
    .unwrap();
    let new_p = write_file(&dir, "pv_new.txt", b"new");
    e2.set(&new_p, &vec![99.0_f32]).unwrap();

    assert_eq!(e2.entry_count().unwrap(), 4); // 3 old + 1 new

    let purged = e2.purge_stale_versions().unwrap();
    assert_eq!(purged, 3); // only v1 entries removed
    assert_eq!(e2.entry_count().unwrap(), 1);
}

#[test]
fn entry_count_by_version_groups_correctly() {
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("cnt_ver.sqlite3");

    // Write 2 v1 entries.
    {
        let e: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: db.clone(),
            payload_version: 1,
            ..CacheOptions::default()
        })
        .unwrap();
        for i in 0..2u32 {
            let p = write_file(&dir, &format!("cv{i}.txt"), b"x");
            e.set(&p, &vec![i as f32]).unwrap();
        }
    }

    // Write 3 v2 entries.
    {
        let e: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: db.clone(),
            payload_version: 2,
            ..CacheOptions::default()
        })
        .unwrap();
        for i in 2..5u32 {
            let p = write_file(&dir, &format!("cv{i}.txt"), b"x");
            e.set(&p, &vec![i as f32]).unwrap();
        }

        let counts = e.entry_count_by_version().unwrap();
        // Should report v1 → 2, v2 → 3.
        let map: std::collections::HashMap<_, _> = counts.into_iter().collect();
        assert_eq!(map[&1], 2);
        assert_eq!(map[&2], 3);
    }
}

#[test]
fn entry_count_reflects_set_and_remove() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: ":memory:".into(),
        ..CacheOptions::default()
    })
    .unwrap();

    assert_eq!(engine.entry_count().unwrap(), 0);

    let p1 = write_file(&dir, "ec1.txt", b"a");
    let p2 = write_file(&dir, "ec2.txt", b"b");
    engine.set(&p1, &vec![1.0_f32]).unwrap();
    engine.set(&p2, &vec![2.0_f32]).unwrap();
    assert_eq!(engine.entry_count().unwrap(), 2);

    engine.remove(&p1).unwrap();
    assert_eq!(engine.entry_count().unwrap(), 1);
}

// ====================================================================
// Phase 5 — Async scan_dir_filtered + entry_count
// ====================================================================

#[cfg(feature = "async")]
mod async_phase5_tests {
    use super::*;
    use localcache::{AsyncCacheEngine, ScanOptions};

    #[tokio::test]
    async fn async_scan_dir_filtered() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("async_scan");
        fs::create_dir(&root).unwrap();

        let engine: AsyncCacheEngine<Vec<f32>> = AsyncCacheEngine::open(CacheOptions {
            database_path: ":memory:".into(),
            ..CacheOptions::default()
        })
        .await
        .unwrap();

        let txt = {
            let p = root.join("x.txt");
            fs::write(&p, b"t").unwrap();
            p
        };
        let bin = {
            let p = root.join("x.bin");
            fs::write(&p, b"b").unwrap();
            p
        };

        engine.set(txt.clone(), vec![1.0_f32]).await.unwrap();

        let opts = ScanOptions {
            recursive: false,
            extensions: vec!["txt".into()],
            ..ScanOptions::default()
        };
        let results = engine.scan_dir_filtered(root.clone(), opts).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, txt);

        let _ = bin;
    }

    #[tokio::test]
    async fn async_entry_count() {
        let dir = TempDir::new().unwrap();
        let engine: AsyncCacheEngine<Vec<f32>> = AsyncCacheEngine::open(CacheOptions {
            database_path: ":memory:".into(),
            ..CacheOptions::default()
        })
        .await
        .unwrap();

        let p = write_file(&dir, "ae.txt", b"x");
        engine.set(p, vec![1.0_f32]).await.unwrap();
        assert_eq!(engine.entry_count().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn async_purge_stale_versions() {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("async_purge.sqlite3");

        // Write with v1.
        {
            let e: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
                database_path: db.clone(),
                payload_version: 1,
                ..CacheOptions::default()
            })
            .unwrap();
            let p = write_file(&dir, "apv.txt", b"x");
            e.set(&p, &vec![1.0_f32]).unwrap();
        }

        // Purge with v2.
        let e2: AsyncCacheEngine<Vec<f32>> = AsyncCacheEngine::open(CacheOptions {
            database_path: db,
            payload_version: 2,
            ..CacheOptions::default()
        })
        .await
        .unwrap();

        let purged = e2.purge_stale_versions().await.unwrap();
        assert_eq!(purged, 1);
        assert_eq!(e2.entry_count().await.unwrap(), 0);
    }
}

// ====================================================================
// Phase 6 — Encryption (requires `encryption` feature)
// ====================================================================

#[cfg(feature = "encryption")]
mod encryption_tests {
    use super::*;

    fn key32(seed: u8) -> Vec<u8> {
        vec![seed; 32]
    }

    #[test]
    fn encrypted_payload_roundtrip() {
        let dir = TempDir::new().unwrap();
        let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: ":memory:".into(),
            encryption_key: Some(key32(0xAB)),
            ..CacheOptions::default()
        })
        .unwrap();

        let path = write_file(&dir, "enc.txt", b"secret content");
        let payload = vec![1.0_f32, 2.0, 3.0];
        engine.set(&path, &payload).unwrap();
        let entry = engine.get(&path).unwrap().unwrap();
        assert_eq!(entry.payload, payload);
    }

    #[test]
    fn encrypted_entry_has_aes_tag() {
        let dir = TempDir::new().unwrap();
        let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: ":memory:".into(),
            encryption_key: Some(key32(0x11)),
            ..CacheOptions::default()
        })
        .unwrap();

        let path = write_file(&dir, "tag.txt", b"data");
        engine.set(&path, &vec![1.0_f32]).unwrap();

        let entries = engine.list_entries().unwrap();
        assert_eq!(entries.len(), 1);
        assert!(
            entries[0].encoding.ends_with("-aes256gcm"),
            "expected aes256gcm suffix, got: {}",
            entries[0].encoding
        );
    }

    #[test]
    fn wrong_key_fails_decryption() {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("enc_key.sqlite3");

        {
            let e: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
                database_path: db.clone(),
                encryption_key: Some(key32(0x01)),
                ..CacheOptions::default()
            })
            .unwrap();
            let p = write_file(&dir, "wk.txt", b"data");
            e.set(&p, &vec![1.0_f32]).unwrap();
        }

        // Re-open with a different key.
        let e2: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: db,
            encryption_key: Some(key32(0x02)),
            ..CacheOptions::default()
        })
        .unwrap();
        let p = dir.path().join("wk.txt");
        let result = e2.get(&p);
        assert!(result.is_err(), "decryption with wrong key should fail");
    }

    #[test]
    fn no_key_fails_on_encrypted_entry() {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("enc_nokey.sqlite3");

        {
            let e: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
                database_path: db.clone(),
                encryption_key: Some(key32(0xCC)),
                ..CacheOptions::default()
            })
            .unwrap();
            let p = write_file(&dir, "nk.txt", b"data");
            e.set(&p, &vec![1.0_f32]).unwrap();
        }

        let e2: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: db,
            encryption_key: None,
            ..CacheOptions::default()
        })
        .unwrap();
        let p = dir.path().join("nk.txt");
        let result = e2.get(&p);
        assert!(
            result.is_err(),
            "reading encrypted entry without key should fail"
        );
    }

    #[test]
    fn encryption_and_freshness_check() {
        let dir = TempDir::new().unwrap();
        let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: ":memory:".into(),
            encryption_key: Some(key32(0x42)),
            ..CacheOptions::default()
        })
        .unwrap();

        let path = write_file(&dir, "enc_fresh.txt", b"stable");
        engine.set(&path, &vec![7.0_f32]).unwrap();

        assert_eq!(engine.check_status(&path).unwrap(), CacheStatus::Fresh);
        assert!(engine.get_if_fresh(&path).unwrap().is_some());
    }

    #[cfg(feature = "compression")]
    #[test]
    fn encryption_with_compression_roundtrip() {
        let dir = TempDir::new().unwrap();
        let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: ":memory:".into(),
            encryption_key: Some(key32(0x77)),
            compress_payloads: true,
            ..CacheOptions::default()
        })
        .unwrap();

        let path = write_file(&dir, "enc_zstd.txt", b"content");
        let payload: Vec<f32> = (0..500).map(|i| i as f32).collect();
        engine.set(&path, &payload).unwrap();
        let entry = engine.get(&path).unwrap().unwrap();
        assert_eq!(entry.payload.len(), 500);

        // Verify tag has both zstd and aes256gcm.
        let entries = engine.list_entries().unwrap();
        assert!(entries[0].encoding.contains("zstd"));
        assert!(entries[0].encoding.ends_with("-aes256gcm"));
    }
}

// ====================================================================
// Phase 6 — True LRU eviction (last_accessed_at tracking)
// ====================================================================

#[test]
fn lru_updates_last_accessed_on_get() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: ":memory:".into(),
        ..CacheOptions::default()
    })
    .unwrap();

    let path = write_file(&dir, "lru.txt", b"x");
    engine.set(&path, &vec![1.0_f32]).unwrap();

    let before = engine.list_entries().unwrap()[0].last_accessed_at;
    assert_eq!(before, 0, "last_accessed_at should be 0 after write");

    engine.get(&path).unwrap();

    let after = engine.list_entries().unwrap()[0].last_accessed_at;
    assert!(after > 0, "last_accessed_at should be non-zero after read");
}

#[test]
fn lru_evicts_least_recently_accessed() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: ":memory:".into(),
        max_entries: Some(2),
        ..CacheOptions::default()
    })
    .unwrap();

    let p1 = write_file(&dir, "lru1.txt", b"a");
    let p2 = write_file(&dir, "lru2.txt", b"b");

    engine.set(&p1, &vec![1.0_f32]).unwrap();
    engine.set(&p2, &vec![2.0_f32]).unwrap();

    // Read p2 → p2 is now more recently accessed than p1.
    engine.get(&p2).unwrap();

    // Adding p3 should evict p1 (least recently accessed).
    let p3 = write_file(&dir, "lru3.txt", b"c");
    engine.set(&p3, &vec![3.0_f32]).unwrap();

    assert_eq!(engine.entry_count().unwrap(), 2);
    // p2 and p3 should survive; p1 should be evicted.
    assert!(engine.get(&p2).unwrap().is_some(), "p2 should survive");
    assert!(engine.get(&p3).unwrap().is_some(), "p3 should survive");
}

// ====================================================================
// Phase 6 — Glob pattern matching in scan_dir_filtered
// ====================================================================

#[test]
fn glob_star_wildcard() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("glob_star");
    fs::create_dir(&root).unwrap();

    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: ":memory:".into(),
        ..CacheOptions::default()
    })
    .unwrap();

    let f1 = {
        let p = root.join("report_2024.txt");
        fs::write(&p, b"r").unwrap();
        p
    };
    let f2 = {
        let p = root.join("report_2025.txt");
        fs::write(&p, b"r").unwrap();
        p
    };
    let f3 = {
        let p = root.join("summary.txt");
        fs::write(&p, b"s").unwrap();
        p
    };

    let opts = ScanOptions {
        recursive: false,
        glob_pattern: Some("report_*.txt".into()),
        ..ScanOptions::default()
    };
    let results = engine.scan_dir_filtered(&root, opts).unwrap();
    let paths: Vec<_> = results.iter().map(|(p, _)| p.clone()).collect();

    assert_eq!(paths.len(), 2);
    assert!(paths.contains(&f1));
    assert!(paths.contains(&f2));
    assert!(!paths.contains(&f3));
}

#[test]
fn glob_question_wildcard() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("glob_q");
    fs::create_dir(&root).unwrap();

    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: ":memory:".into(),
        ..CacheOptions::default()
    })
    .unwrap();

    let fa = {
        let p = root.join("a1.txt");
        fs::write(&p, b"a").unwrap();
        p
    };
    let fb = {
        let p = root.join("b2.txt");
        fs::write(&p, b"b").unwrap();
        p
    };
    let fc = {
        let p = root.join("ab.txt");
        fs::write(&p, b"c").unwrap();
        p
    };

    // "??.txt" matches exactly two-char names before extension.
    let opts = ScanOptions {
        recursive: false,
        glob_pattern: Some("??.txt".into()),
        ..ScanOptions::default()
    };
    let results = engine.scan_dir_filtered(&root, opts).unwrap();
    let paths: Vec<_> = results.iter().map(|(p, _)| p.clone()).collect();

    assert_eq!(paths.len(), 3, "all three files match ??. txt");
    assert!(paths.contains(&fa));
    assert!(paths.contains(&fb));
    assert!(paths.contains(&fc));
}

#[test]
fn glob_combined_with_extension_filter() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("glob_ext");
    fs::create_dir(&root).unwrap();

    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: ":memory:".into(),
        ..CacheOptions::default()
    })
    .unwrap();

    let txt = {
        let p = root.join("data_01.txt");
        fs::write(&p, b"t").unwrap();
        p
    };
    let md = {
        let p = root.join("data_01.md");
        fs::write(&p, b"m").unwrap();
        p
    };
    let _x = {
        let p = root.join("other.txt");
        fs::write(&p, b"o").unwrap();
        p
    };

    let opts = ScanOptions {
        recursive: false,
        extensions: vec!["txt".into()],
        glob_pattern: Some("data_*.txt".into()),
        ..ScanOptions::default()
    };
    let results = engine.scan_dir_filtered(&root, opts).unwrap();
    let paths: Vec<_> = results.iter().map(|(p, _)| p.clone()).collect();

    // Only data_01.txt should match (correct extension + glob).
    assert_eq!(paths.len(), 1);
    assert!(paths.contains(&txt));
    assert!(!paths.contains(&md));
}

// ====================================================================
// Phase 6 — list_entries
// ====================================================================

#[test]
fn list_entries_returns_all_metadata() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: ":memory:".into(),
        payload_version: 3,
        ..CacheOptions::default()
    })
    .unwrap();

    let p1 = write_file(&dir, "le1.txt", b"a");
    let p2 = write_file(&dir, "le2.txt", b"b");
    engine.set(&p1, &vec![1.0_f32]).unwrap();
    engine.set(&p2, &vec![2.0_f32]).unwrap();

    let entries = engine.list_entries().unwrap();
    assert_eq!(entries.len(), 2);

    for e in &entries {
        assert_eq!(e.payload_version, 3);
        assert_eq!(e.encoding, "raw");
        assert!(e.updated_at > 0);
        assert_eq!(e.last_accessed_at, 0); // never read since write
    }
}

#[test]
fn list_entries_empty_namespace() {
    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: ":memory:".into(),
        ..CacheOptions::default()
    })
    .unwrap();

    let entries = engine.list_entries().unwrap();
    assert!(entries.is_empty());
}

#[test]
fn list_entries_last_accessed_at_updated_after_read() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: ":memory:".into(),
        ..CacheOptions::default()
    })
    .unwrap();

    let path = write_file(&dir, "le_read.txt", b"content");
    engine.set(&path, &vec![1.0_f32]).unwrap();

    let before = engine.list_entries().unwrap()[0].last_accessed_at;
    engine.get(&path).unwrap();
    let after = engine.list_entries().unwrap()[0].last_accessed_at;

    assert_eq!(before, 0);
    assert!(after > 0);
}

// ====================================================================
// Phase 6 — Schema migration (v3 → v4)
// ====================================================================

#[test]
fn migrates_v3_database_to_v4() {
    use rusqlite::Connection;

    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("v3tov4.sqlite3");

    {
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "
            PRAGMA user_version = 3;
            CREATE TABLE files (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                namespace       TEXT    NOT NULL DEFAULT 'default',
                path            TEXT    NOT NULL,
                mtime           INTEGER NOT NULL,
                file_size       INTEGER NOT NULL,
                hash            TEXT,
                updated_at      INTEGER NOT NULL,
                payload_version INTEGER NOT NULL DEFAULT 0,
                UNIQUE(namespace, path)
            );
            CREATE TABLE payloads (
                file_id  INTEGER PRIMARY KEY,
                content  BLOB NOT NULL,
                encoding TEXT NOT NULL DEFAULT 'raw',
                FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
            );
            INSERT INTO files (namespace, path, mtime, file_size, updated_at)
            VALUES ('default', '/v3/legacy.txt', 1000, 10, 1000);
            ",
        )
        .unwrap();
    }

    // Opening must migrate v3 → v4 transparently.
    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: db_path,
        ..CacheOptions::default()
    })
    .unwrap();

    // Legacy row is accessible.
    assert_eq!(
        engine.check_status("/v3/legacy.txt").unwrap(),
        CacheStatus::Missing
    );
}

// ====================================================================
// Phase 7 — CacheEngineBuilder
// ====================================================================
