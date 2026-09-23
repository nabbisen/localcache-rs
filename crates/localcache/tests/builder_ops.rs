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

    // ----------------------------------------------------------------
    // RFC 022 R1 — the rotating engine itself must keep working
    // ----------------------------------------------------------------

    #[test]
    fn same_engine_read_after_rotation() {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("rot_same_read.sqlite3");
        let paths: Vec<_> = (0..3u32)
            .map(|i| write_file(&dir, &format!("read{i}.txt"), b"data"))
            .collect();

        let e: CacheEngine<Vec<f32>> = CacheEngine::builder()
            .database(db)
            .encryption_key(key(0xA1))
            .build()
            .unwrap();
        for (i, p) in paths.iter().enumerate() {
            e.set(p, &vec![i as f32]).unwrap();
        }

        let rotated = e.rotate_encryption_key(&key(0xB1)).unwrap();
        assert_eq!(rotated, 3);

        // Same engine instance, no reopen: every rotated row must decode.
        for (i, p) in paths.iter().enumerate() {
            let entry = e
                .get(p)
                .unwrap_or_else(|err| panic!("get after rotation failed for {p:?}: {err}"))
                .unwrap_or_else(|| panic!("row must still exist after rotation: {p:?}"));
            assert_eq!(entry.payload, vec![i as f32]);
        }
    }

    #[test]
    fn same_engine_write_after_rotation() {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("rot_same_write.sqlite3");
        let existing = write_file(&dir, "existing.txt", b"data");
        let new_path = write_file(&dir, "new_after_rotation.txt", b"data2");

        let e: CacheEngine<Vec<f32>> = CacheEngine::builder()
            .database(db.clone())
            .encryption_key(key(0xC1))
            .build()
            .unwrap();
        e.set(&existing, &vec![1.0_f32]).unwrap();

        e.rotate_encryption_key(&key(0xD1)).unwrap();

        // Write a NEW row through the SAME engine handle, after rotation.
        e.set(&new_path, &vec![2.0_f32]).unwrap();
        drop(e);

        // Reopen with the NEW key ONLY — every row, including the one
        // written after rotation, must decode.
        let e_new: CacheEngine<Vec<f32>> = CacheEngine::builder()
            .database(db)
            .encryption_key(key(0xD1))
            .build()
            .unwrap();

        let existing_entry = e_new
            .get(&existing)
            .unwrap_or_else(|err| panic!("existing row: {err}"))
            .expect("existing row must exist");
        assert_eq!(existing_entry.payload, vec![1.0_f32]);

        let new_entry = e_new
            .get(&new_path)
            .unwrap_or_else(|err| {
                panic!("row written through the rotating engine after rotation: {err}")
            })
            .expect("new row must exist");
        assert_eq!(new_entry.payload, vec![2.0_f32]);
    }

    #[test]
    fn same_engine_read_after_rotation_through_sync_cache_engine() {
        use localcache::SyncCacheEngine;

        let dir = TempDir::new().unwrap();
        let db = dir.path().join("rot_pool_read.sqlite3");
        let paths: Vec<_> = (0..3u32)
            .map(|i| write_file(&dir, &format!("pool_read{i}.txt"), b"data"))
            .collect();

        let pool: SyncCacheEngine<Vec<f32>> = SyncCacheEngine::open(CacheOptions {
            database_path: db,
            encryption_key: Some(key(0x61)),
            ..CacheOptions::default()
        })
        .unwrap();

        for (i, p) in paths.iter().enumerate() {
            pool.set(p, &vec![i as f32]).unwrap();
        }

        let rotated = pool.rotate_encryption_key(&key(0x62)).unwrap();
        assert_eq!(rotated, 3);

        for (i, p) in paths.iter().enumerate() {
            let entry = pool
                .get(p)
                .unwrap_or_else(|err| panic!("pooled get after rotation failed for {p:?}: {err}"))
                .unwrap_or_else(|| panic!("row must still exist after rotation: {p:?}"));
            assert_eq!(entry.payload, vec![i as f32]);
        }
    }

    #[test]
    fn same_engine_write_after_rotation_through_sync_cache_engine() {
        use localcache::SyncCacheEngine;

        let dir = TempDir::new().unwrap();
        let db = dir.path().join("rot_pool_write.sqlite3");
        let existing = write_file(&dir, "pool_existing.txt", b"data");
        let new_path = write_file(&dir, "pool_new_after_rotation.txt", b"data2");

        let pool: SyncCacheEngine<Vec<f32>> = SyncCacheEngine::open(CacheOptions {
            database_path: db.clone(),
            encryption_key: Some(key(0x71)),
            ..CacheOptions::default()
        })
        .unwrap();
        pool.set(&existing, &vec![1.0_f32]).unwrap();

        pool.rotate_encryption_key(&key(0x72)).unwrap();
        pool.set(&new_path, &vec![2.0_f32]).unwrap();
        drop(pool);

        let e_new: CacheEngine<Vec<f32>> = CacheEngine::builder()
            .database(db)
            .encryption_key(key(0x72))
            .build()
            .unwrap();
        let existing_entry = e_new
            .get(&existing)
            .unwrap_or_else(|err| panic!("existing row: {err}"))
            .expect("existing row must exist");
        assert_eq!(existing_entry.payload, vec![1.0_f32]);

        let new_entry = e_new
            .get(&new_path)
            .unwrap_or_else(|err| panic!("row written through the pool after rotation: {err}"))
            .expect("new row must exist");
        assert_eq!(new_entry.payload, vec![2.0_f32]);
    }

    #[test]
    fn failed_rotation_keeps_old_key() {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("rot_fail.sqlite3");
        let good = write_file(&dir, "good.txt", b"data");
        let bad = write_file(&dir, "bad.txt", b"data2");

        let e: CacheEngine<Vec<f32>> = CacheEngine::builder()
            .database(db.clone())
            .encryption_key(key(0xE1))
            .build()
            .unwrap();
        e.set(&good, &vec![1.0_f32]).unwrap();
        e.set(&bad, &vec![2.0_f32]).unwrap();

        // Corrupt `bad`'s ciphertext (flip the last byte, breaking AEAD
        // authentication) via direct SQL, mirroring the pattern
        // `json_decode_failure_yields_serialization_error` (codec_lru.rs)
        // uses to reach an otherwise-unreachable decode failure.
        {
            let stored = e.keys(None).unwrap();
            let bad_key = stored
                .iter()
                .find(|p| p.to_str().unwrap().contains("bad.txt"))
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned();

            let conn = rusqlite::Connection::open(&db).unwrap();
            let content: Vec<u8> = conn
                .query_row(
                    "SELECT content FROM payloads
                     WHERE file_id = (SELECT id FROM files WHERE path = ?1)",
                    rusqlite::params![bad_key],
                    |row| row.get(0),
                )
                .unwrap();
            let mut corrupted = content;
            let last = corrupted.len() - 1;
            corrupted[last] ^= 0xFF;
            conn.execute(
                "UPDATE payloads SET content = ?1
                 WHERE file_id = (SELECT id FROM files WHERE path = ?2)",
                rusqlite::params![corrupted, bad_key],
            )
            .unwrap();
        }

        let result = e.rotate_encryption_key(&key(0xF1));
        assert!(
            result.is_err(),
            "rotation must fail when a row is undecryptable, got {result:?}"
        );

        // The engine must still read the OTHER row with the OLD key.
        let entry = e
            .get(&good)
            .unwrap_or_else(|err| panic!("good row must still decode with the old key: {err}"))
            .expect("good row must still exist");
        assert_eq!(entry.payload, vec![1.0_f32]);
    }

    #[test]
    fn rotation_with_nothing_to_reencrypt_adopts_the_new_key() {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("rot_empty.sqlite3");

        let e: CacheEngine<Vec<f32>> = CacheEngine::builder()
            .database(db.clone())
            .encryption_key(key(0x61))
            .build()
            .unwrap();

        // Nothing has been written yet: rotating re-encrypts zero rows.
        let rotated = e.rotate_encryption_key(&key(0x62)).unwrap();
        assert_eq!(rotated, 0);

        // `Ok` must still mean this engine switched keys: a write through
        // the same handle right after must be encoded with the NEW key.
        let p = write_file(&dir, "empty_rotation.txt", b"data");
        e.set(&p, &vec![7.0_f32]).unwrap();
        drop(e);

        let e_new: CacheEngine<Vec<f32>> = CacheEngine::builder()
            .database(db)
            .encryption_key(key(0x62))
            .build()
            .unwrap();
        let entry = e_new
            .get(&p)
            .unwrap_or_else(|err| {
                panic!("row written after a zero-row rotation must decode with the new key: {err}")
            })
            .expect("row must exist");
        assert_eq!(entry.payload, vec![7.0_f32]);
    }

    #[test]
    fn query_builder_across_rotation_decodes_with_current_key() {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("rot_query.sqlite3");
        let p = write_file(&dir, "query_rot.txt", b"data");

        let e: CacheEngine<Vec<f32>> = CacheEngine::builder()
            .database(db)
            .encryption_key(key(0x51))
            .build()
            .unwrap();
        e.set(&p, &vec![9.0_f32]).unwrap();

        // Built BEFORE rotation; both are shared borrows of `e`.
        let q = e.query();
        e.rotate_encryption_key(&key(0x52)).unwrap();
        let results = q
            .run()
            .unwrap_or_else(|err| panic!("query across rotation failed: {err}"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].payload, vec![9.0_f32]);
    }

    // ----------------------------------------------------------------
    // RFC 022 R1 — through AsyncCacheEngine, on every async backend this
    // suite runs. Mirrors the block_on/tokio macro split in
    // `pool_observe.rs`'s `panic_inside_blocking_closure_yields_async_task_panicked_test!`
    // (RFC 005 / RFC 015): the bodies below are identical, module for
    // module, across async-std, smol, and Tokio -- only the test-harness
    // shape differs.
    // ----------------------------------------------------------------

    #[cfg(any(feature = "async", feature = "async-std", feature = "smol"))]
    macro_rules! rotation_async_tests {
        (block_on = $block_on_fn:path) => {
            fn block_on<F: std::future::Future>(f: F) -> F::Output {
                $block_on_fn(f)
            }

            #[test]
            fn same_engine_read_after_rotation_async() {
                block_on(async {
                    let dir = TempDir::new().unwrap();
                    let db = dir.path().join("rot_async_read.sqlite3");
                    let paths: Vec<_> = (0..3u32)
                        .map(|i| write_file(&dir, &format!("async_read{i}.txt"), b"data"))
                        .collect();

                    let engine = AsyncCacheEngine::<Vec<f32>>::open(CacheOptions {
                        database_path: db,
                        encryption_key: Some(key(0x81)),
                        ..CacheOptions::default()
                    })
                    .await
                    .unwrap();

                    for (i, p) in paths.iter().enumerate() {
                        engine.set(p.clone(), vec![i as f32]).await.unwrap();
                    }

                    let rotated = engine.rotate_encryption_key(key(0x82)).await.unwrap();
                    assert_eq!(rotated, 3);

                    for (i, p) in paths.iter().enumerate() {
                        let entry = engine
                            .get(p.clone())
                            .await
                            .unwrap_or_else(|err| {
                                panic!("async get after rotation failed for {p:?}: {err}")
                            })
                            .unwrap_or_else(|| {
                                panic!("row must still exist after rotation: {p:?}")
                            });
                        assert_eq!(entry.payload, vec![i as f32]);
                    }
                });
            }

            #[test]
            fn same_engine_write_after_rotation_async() {
                block_on(async {
                    let dir = TempDir::new().unwrap();
                    let db = dir.path().join("rot_async_write.sqlite3");
                    let existing = write_file(&dir, "async_existing.txt", b"data");
                    let new_path = write_file(&dir, "async_new_after_rotation.txt", b"data2");

                    let engine = AsyncCacheEngine::<Vec<f32>>::open(CacheOptions {
                        database_path: db.clone(),
                        encryption_key: Some(key(0x91)),
                        ..CacheOptions::default()
                    })
                    .await
                    .unwrap();
                    engine.set(existing.clone(), vec![1.0_f32]).await.unwrap();

                    engine.rotate_encryption_key(key(0x92)).await.unwrap();
                    engine.set(new_path.clone(), vec![2.0_f32]).await.unwrap();
                    drop(engine);

                    let engine_new = AsyncCacheEngine::<Vec<f32>>::open(CacheOptions {
                        database_path: db,
                        encryption_key: Some(key(0x92)),
                        ..CacheOptions::default()
                    })
                    .await
                    .unwrap();

                    let existing_entry = engine_new
                        .get(existing)
                        .await
                        .unwrap_or_else(|err| panic!("existing row: {err}"))
                        .expect("existing row must exist");
                    assert_eq!(existing_entry.payload, vec![1.0_f32]);

                    let new_entry = engine_new
                        .get(new_path)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("row written through the rotating engine after rotation: {err}")
                        })
                        .expect("new row must exist");
                    assert_eq!(new_entry.payload, vec![2.0_f32]);
                });
            }
        };
        (tokio) => {
            #[tokio::test]
            async fn same_engine_read_after_rotation_async() {
                let dir = TempDir::new().unwrap();
                let db = dir.path().join("rot_async_read.sqlite3");
                let paths: Vec<_> = (0..3u32)
                    .map(|i| write_file(&dir, &format!("async_read{i}.txt"), b"data"))
                    .collect();

                let engine = AsyncCacheEngine::<Vec<f32>>::open(CacheOptions {
                    database_path: db,
                    encryption_key: Some(key(0x81)),
                    ..CacheOptions::default()
                })
                .await
                .unwrap();

                for (i, p) in paths.iter().enumerate() {
                    engine.set(p.clone(), vec![i as f32]).await.unwrap();
                }

                let rotated = engine.rotate_encryption_key(key(0x82)).await.unwrap();
                assert_eq!(rotated, 3);

                for (i, p) in paths.iter().enumerate() {
                    let entry = engine
                        .get(p.clone())
                        .await
                        .unwrap_or_else(|err| {
                            panic!("async get after rotation failed for {p:?}: {err}")
                        })
                        .unwrap_or_else(|| panic!("row must still exist after rotation: {p:?}"));
                    assert_eq!(entry.payload, vec![i as f32]);
                }
            }

            #[tokio::test]
            async fn same_engine_write_after_rotation_async() {
                let dir = TempDir::new().unwrap();
                let db = dir.path().join("rot_async_write.sqlite3");
                let existing = write_file(&dir, "async_existing.txt", b"data");
                let new_path = write_file(&dir, "async_new_after_rotation.txt", b"data2");

                let engine = AsyncCacheEngine::<Vec<f32>>::open(CacheOptions {
                    database_path: db.clone(),
                    encryption_key: Some(key(0x91)),
                    ..CacheOptions::default()
                })
                .await
                .unwrap();
                engine.set(existing.clone(), vec![1.0_f32]).await.unwrap();

                engine.rotate_encryption_key(key(0x92)).await.unwrap();
                engine.set(new_path.clone(), vec![2.0_f32]).await.unwrap();
                drop(engine);

                let engine_new = AsyncCacheEngine::<Vec<f32>>::open(CacheOptions {
                    database_path: db,
                    encryption_key: Some(key(0x92)),
                    ..CacheOptions::default()
                })
                .await
                .unwrap();

                let existing_entry = engine_new
                    .get(existing)
                    .await
                    .unwrap_or_else(|err| panic!("existing row: {err}"))
                    .expect("existing row must exist");
                assert_eq!(existing_entry.payload, vec![1.0_f32]);

                let new_entry = engine_new
                    .get(new_path)
                    .await
                    .unwrap_or_else(|err| {
                        panic!("row written through the rotating engine after rotation: {err}")
                    })
                    .expect("new row must exist");
                assert_eq!(new_entry.payload, vec![2.0_f32]);
            }
        };
    }

    #[cfg(all(not(feature = "async"), feature = "async-std"))]
    mod rotation_async_std {
        use super::*;
        use localcache::AsyncCacheEngine;

        rotation_async_tests!(block_on = async_std::task::block_on);
    }

    #[cfg(all(not(feature = "async"), not(feature = "async-std"), feature = "smol"))]
    mod rotation_smol {
        use super::*;
        use localcache::AsyncCacheEngine;

        rotation_async_tests!(block_on = smol::block_on);
    }

    #[cfg(feature = "async")]
    mod rotation_tokio {
        use super::*;
        use localcache::AsyncCacheEngine;

        rotation_async_tests!(tokio);
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
