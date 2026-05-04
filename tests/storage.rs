//! Integration tests — storage.

mod common;
use common::write_file;

use std::fs;

use localcache::{CacheEngine, CacheOptions, CacheStatus, ChangeDetectionMode};
use tempfile::TempDir;

// ====================================================================
// Phase 3 — True MetadataThenPartialHash
// ====================================================================

#[test]
fn partial_hash_fresh_for_unchanged_large_file() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: dir.path().join("partial.sqlite3"),
        change_detection_mode: ChangeDetectionMode::MetadataThenPartialHash,
        ..CacheOptions::default()
    })
    .unwrap();
    // 200 KiB > 2 × 64 KiB threshold → head+tail sampling
    let large: Vec<u8> = (0u8..=255).cycle().take(200 * 1024).collect();
    let path = write_file(&dir, "large.bin", &large);
    engine.set(&path, &vec![1.0_f32]).unwrap();
    assert_eq!(engine.check_status(&path).unwrap(), CacheStatus::Fresh);
    assert!(engine.get_if_fresh(&path).unwrap().is_some());
}

#[test]
fn partial_hash_detects_head_change() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: dir.path().join("partial_head.sqlite3"),
        change_detection_mode: ChangeDetectionMode::MetadataThenPartialHash,
        ..CacheOptions::default()
    })
    .unwrap();
    let mut large: Vec<u8> = vec![0xAA; 200 * 1024];
    let path = write_file(&dir, "head.bin", &large);
    engine.set(&path, &vec![1.0_f32]).unwrap();
    // Mutate head byte; also change size to trigger metadata diff
    large[0] = 0xBB;
    large.push(0xFF);
    write_file(&dir, "head.bin", &large);
    assert_eq!(engine.check_status(&path).unwrap(), CacheStatus::Stale);
}

#[test]
fn partial_hash_detects_tail_change() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: dir.path().join("partial_tail.sqlite3"),
        change_detection_mode: ChangeDetectionMode::MetadataThenPartialHash,
        ..CacheOptions::default()
    })
    .unwrap();
    let mut large: Vec<u8> = vec![0xCC; 200 * 1024];
    let path = write_file(&dir, "tail.bin", &large);
    engine.set(&path, &vec![2.0_f32]).unwrap();
    // Mutate tail byte; change size to trigger metadata diff
    *large.last_mut().unwrap() = 0xDD;
    large.push(0xFF);
    write_file(&dir, "tail.bin", &large);
    assert_eq!(engine.check_status(&path).unwrap(), CacheStatus::Stale);
}

#[test]
fn partial_hash_prefix_stored_in_db() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: dir.path().join("prefix.sqlite3"),
        change_detection_mode: ChangeDetectionMode::MetadataThenPartialHash,
        ..CacheOptions::default()
    })
    .unwrap();
    let path = write_file(&dir, "pref.txt", b"hello world");
    engine.set(&path, &vec![1.0_f32]).unwrap();
    let entry = engine.get(&path).unwrap().unwrap();
    let hash = entry.metadata.hash.as_deref().unwrap_or("");
    assert!(
        hash.starts_with("partial:"),
        "expected 'partial:' prefix, got: {hash}"
    );
}

#[test]
fn partial_hash_small_file_is_still_fresh() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: dir.path().join("small_partial.sqlite3"),
        change_detection_mode: ChangeDetectionMode::MetadataThenPartialHash,
        ..CacheOptions::default()
    })
    .unwrap();
    let path = write_file(&dir, "small.txt", b"tiny content");
    engine.set(&path, &vec![3.0_f32]).unwrap();
    assert_eq!(engine.check_status(&path).unwrap(), CacheStatus::Fresh);
}

// ====================================================================
// Phase 3 — In-memory backend
// ====================================================================

#[test]
fn in_memory_backend_basic_ops() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: ":memory:".into(),
        ..CacheOptions::default()
    })
    .unwrap();
    let path = write_file(&dir, "mem.txt", b"memory test");
    engine.set(&path, &vec![42.0_f32]).unwrap();
    assert_eq!(engine.get(&path).unwrap().unwrap().payload, vec![42.0_f32]);
}

#[test]
fn in_memory_backend_not_shared_between_instances() {
    let dir = TempDir::new().unwrap();
    let engine_a: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: ":memory:".into(),
        ..CacheOptions::default()
    })
    .unwrap();
    let engine_b: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: ":memory:".into(),
        ..CacheOptions::default()
    })
    .unwrap();
    let path = write_file(&dir, "iso.txt", b"isolation");
    engine_a.set(&path, &vec![1.0_f32]).unwrap();
    // engine_b has its own DB
    assert!(engine_b.get(&path).unwrap().is_none());
}

#[test]
fn in_memory_backend_change_detection() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: ":memory:".into(),
        change_detection_mode: ChangeDetectionMode::MetadataThenFullHash,
        ..CacheOptions::default()
    })
    .unwrap();
    let path = write_file(&dir, "cd.txt", b"original");
    engine.set(&path, &vec![1.0_f32]).unwrap();
    assert!(engine.get_if_fresh(&path).unwrap().is_some());
    write_file(&dir, "cd.txt", b"modified!!");
    assert!(engine.get_if_fresh(&path).unwrap().is_none());
}

// ====================================================================
// Phase 3 — Read-only mode
// ====================================================================

#[test]
fn read_only_allows_reads() {
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("ro.sqlite3");
    {
        let rw: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: db.clone(),
            ..CacheOptions::default()
        })
        .unwrap();
        let path = write_file(&dir, "ro.txt", b"read-only test");
        rw.set(&path, &vec![7.0_f32]).unwrap();
    }
    let ro: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: db,
        read_only: true,
        ..CacheOptions::default()
    })
    .unwrap();
    let path = dir.path().join("ro.txt");
    assert_eq!(ro.get(&path).unwrap().unwrap().payload, vec![7.0_f32]);
}

#[test]
fn read_only_blocks_set() {
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("ro2.sqlite3");
    CacheEngine::<Vec<f32>>::open(CacheOptions {
        database_path: db.clone(),
        ..CacheOptions::default()
    })
    .unwrap();
    let ro: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: db,
        read_only: true,
        ..CacheOptions::default()
    })
    .unwrap();
    let path = write_file(&dir, "block.txt", b"x");
    let result = ro.set(&path, &vec![1.0_f32]);
    assert!(
        matches!(result, Err(localcache::LocalFileCacheError::ReadOnly)),
        "expected ReadOnly error, got {result:?}"
    );
}

#[test]
fn read_only_blocks_remove() {
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("ro3.sqlite3");
    CacheEngine::<Vec<f32>>::open(CacheOptions {
        database_path: db.clone(),
        ..CacheOptions::default()
    })
    .unwrap();
    let ro: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: db,
        read_only: true,
        ..CacheOptions::default()
    })
    .unwrap();
    let path = write_file(&dir, "rblock.txt", b"y");
    assert!(matches!(
        ro.remove(&path),
        Err(localcache::LocalFileCacheError::ReadOnly)
    ));
}

#[test]
fn read_only_blocks_cleanup_and_vacuum() {
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("ro4.sqlite3");
    CacheEngine::<Vec<f32>>::open(CacheOptions {
        database_path: db.clone(),
        ..CacheOptions::default()
    })
    .unwrap();
    let ro: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: db,
        read_only: true,
        ..CacheOptions::default()
    })
    .unwrap();
    assert!(matches!(
        ro.cleanup_missing_files(),
        Err(localcache::LocalFileCacheError::ReadOnly)
    ));
    assert!(matches!(
        ro.shrink_database(),
        Err(localcache::LocalFileCacheError::ReadOnly)
    ));
}

// ====================================================================
// Phase 3 — Streaming bincode (correctness at scale)
// ====================================================================

#[test]
fn streaming_bincode_large_payload_roundtrip() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: ":memory:".into(),
        ..CacheOptions::default()
    })
    .unwrap();
    let path = write_file(&dir, "large_payload.txt", b"large");
    // 1 M floats ≈ 4 MB — exercises pre-allocated serialisation path
    let payload: Vec<f32> = (0..1_000_000).map(|i| i as f32 * 0.001).collect();
    engine.set(&path, &payload).unwrap();
    let entry = engine.get(&path).unwrap().unwrap();
    assert_eq!(entry.payload.len(), payload.len());
    assert!((entry.payload[999_999] - payload[999_999]).abs() < 1e-6);
}

// ====================================================================
// Phase 4 — scan_dir
// ====================================================================

#[test]
fn scan_dir_non_recursive() {
    let dir = TempDir::new().unwrap();
    let files_dir = dir.path().join("files");
    fs::create_dir(&files_dir).unwrap();

    // DB lives outside the scanned directory.
    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: ":memory:".into(),
        ..CacheOptions::default()
    })
    .unwrap();

    let f1 = {
        let p = files_dir.join("scan1.txt");
        fs::write(&p, b"a").unwrap();
        p
    };
    let f2 = {
        let p = files_dir.join("scan2.txt");
        fs::write(&p, b"b").unwrap();
        p
    };

    engine.set(&f1, &vec![1.0_f32]).unwrap();

    let results = engine.scan_dir(&files_dir, false).unwrap();
    assert_eq!(results.len(), 2);

    let status_of = |p: &std::path::PathBuf| -> CacheStatus {
        results.iter().find(|(rp, _)| rp == p).unwrap().1
    };
    assert_eq!(status_of(&f1), CacheStatus::Fresh);
    assert_eq!(status_of(&f2), CacheStatus::Missing);
}

#[test]
fn scan_dir_recursive() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("root");
    let sub = root.join("sub");
    fs::create_dir_all(&sub).unwrap();

    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: ":memory:".into(),
        ..CacheOptions::default()
    })
    .unwrap();

    let f_root = {
        let p = root.join("root.txt");
        fs::write(&p, b"root").unwrap();
        p
    };
    let f_sub = {
        let p = sub.join("sub.txt");
        fs::write(&p, b"sub").unwrap();
        p
    };

    engine.set(&f_root, &vec![1.0_f32]).unwrap();
    engine.set(&f_sub, &vec![2.0_f32]).unwrap();

    let results = engine.scan_dir(&root, true).unwrap();
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|(_, s)| *s == CacheStatus::Fresh));
}

#[test]
fn scan_dir_stale_appears_as_stale() {
    let dir = TempDir::new().unwrap();
    let files_dir = dir.path().join("files");
    fs::create_dir(&files_dir).unwrap();

    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: ":memory:".into(),
        ..CacheOptions::default()
    })
    .unwrap();

    let path = {
        let p = files_dir.join("stale_scan.txt");
        fs::write(&p, b"original").unwrap();
        p
    };
    engine.set(&path, &vec![1.0_f32]).unwrap();
    fs::write(&path, b"modified content!!").unwrap();

    let results = engine.scan_dir(&files_dir, false).unwrap();
    let (_, status) = results.iter().find(|(p, _)| p == &path).unwrap();
    assert_eq!(*status, CacheStatus::Stale);
}

// ====================================================================
// Phase 4 — Payload schema versioning
// ====================================================================

#[test]
fn payload_version_fresh_when_matching() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: ":memory:".into(),
        payload_version: 2,
        ..CacheOptions::default()
    })
    .unwrap();

    let path = write_file(&dir, "ver.txt", b"content");
    engine.set(&path, &vec![1.0_f32]).unwrap();

    assert_eq!(engine.check_status(&path).unwrap(), CacheStatus::Fresh);
    assert!(engine.get_if_fresh(&path).unwrap().is_some());
}

#[test]
fn payload_version_stale_when_mismatch() {
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("ver.sqlite3");

    // Write with version 1.
    {
        let writer: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: db.clone(),
            payload_version: 1,
            ..CacheOptions::default()
        })
        .unwrap();
        let path = write_file(&dir, "vermm.txt", b"content");
        writer.set(&path, &vec![1.0_f32]).unwrap();
    }

    // Read back with version 2 → should be stale.
    let reader: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: db,
        payload_version: 2,
        ..CacheOptions::default()
    })
    .unwrap();
    let path = dir.path().join("vermm.txt");
    assert_eq!(reader.check_status(&path).unwrap(), CacheStatus::Stale);
    assert!(reader.get_if_fresh(&path).unwrap().is_none());
}

#[test]
fn payload_version_zero_skips_version_check() {
    // Version 0 (default) disables version checks.
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("ver0.sqlite3");

    {
        let writer: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: db.clone(),
            payload_version: 5, // write with a specific version
            ..CacheOptions::default()
        })
        .unwrap();
        let path = write_file(&dir, "ver0.txt", b"content");
        writer.set(&path, &vec![1.0_f32]).unwrap();
    }

    // Open with version 0 → version check disabled → should be Fresh.
    let reader: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: db,
        payload_version: 0,
        ..CacheOptions::default()
    })
    .unwrap();
    let path = dir.path().join("ver0.txt");
    assert_eq!(reader.check_status(&path).unwrap(), CacheStatus::Fresh);
}

// ====================================================================
// Phase 4 — Schema migration (v2 → v3)
// ====================================================================

#[test]
fn migrates_v2_database_to_v3() {
    use rusqlite::Connection;

    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("v2tov3.sqlite3");

    // Build a v2-style database by hand.
    {
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "
            PRAGMA user_version = 2;
            CREATE TABLE files (
                id        INTEGER PRIMARY KEY AUTOINCREMENT,
                namespace TEXT    NOT NULL DEFAULT 'default',
                path      TEXT    NOT NULL,
                mtime     INTEGER NOT NULL,
                file_size INTEGER NOT NULL,
                hash      TEXT,
                updated_at INTEGER NOT NULL,
                UNIQUE(namespace, path)
            );
            CREATE TABLE payloads (
                file_id INTEGER PRIMARY KEY,
                content BLOB NOT NULL,
                FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
            );
            INSERT INTO files (namespace, path, mtime, file_size, updated_at)
            VALUES ('default', '/v2/legacy.txt', 1000, 10, 1000);
            ",
        )
        .unwrap();
    }

    // Opening must migrate transparently.
    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: db_path,
        ..CacheOptions::default()
    })
    .unwrap();

    // The legacy row should be reachable (as Missing, since file is absent).
    assert_eq!(
        engine.check_status("/v2/legacy.txt").unwrap(),
        CacheStatus::Missing
    );
}

// ====================================================================
// Phase 4 — Async engine (requires `async` feature)
// ====================================================================

#[cfg(feature = "async")]
mod async_tests {
    use super::*;
    use localcache::AsyncCacheEngine;

    #[tokio::test]
    async fn async_set_then_get() {
        let dir = TempDir::new().unwrap();
        let engine: AsyncCacheEngine<Vec<f32>> = AsyncCacheEngine::open(CacheOptions {
            database_path: ":memory:".into(),
            ..CacheOptions::default()
        })
        .await
        .unwrap();

        let path = write_file(&dir, "async.txt", b"hello async");
        engine
            .set(path.clone(), vec![1.0_f32, 2.0, 3.0])
            .await
            .unwrap();

        let entry = engine.get(path.clone()).await.unwrap().unwrap();
        assert_eq!(entry.payload, vec![1.0_f32, 2.0, 3.0]);
    }

    #[tokio::test]
    async fn async_get_if_fresh_unchanged() {
        let dir = TempDir::new().unwrap();
        let engine: AsyncCacheEngine<Vec<f32>> = AsyncCacheEngine::open(CacheOptions {
            database_path: ":memory:".into(),
            ..CacheOptions::default()
        })
        .await
        .unwrap();

        let path = write_file(&dir, "async_fresh.txt", b"stable");
        engine.set(path.clone(), vec![5.0_f32]).await.unwrap();

        assert!(engine.get_if_fresh(path.clone()).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn async_check_status_missing() {
        let engine: AsyncCacheEngine<Vec<f32>> = AsyncCacheEngine::open(CacheOptions {
            database_path: ":memory:".into(),
            ..CacheOptions::default()
        })
        .await
        .unwrap();

        let status = engine
            .check_status("/no/such/file.txt".into())
            .await
            .unwrap();
        assert_eq!(status, CacheStatus::Missing);
    }

    #[tokio::test]
    async fn async_remove() {
        let dir = TempDir::new().unwrap();
        let engine: AsyncCacheEngine<Vec<f32>> = AsyncCacheEngine::open(CacheOptions {
            database_path: ":memory:".into(),
            ..CacheOptions::default()
        })
        .await
        .unwrap();

        let path = write_file(&dir, "async_del.txt", b"bye");
        engine.set(path.clone(), vec![1.0_f32]).await.unwrap();

        let deleted = engine.remove(path.clone()).await.unwrap();
        assert!(deleted);
        assert!(engine.get(path).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn async_batch_set_and_get() {
        let dir = TempDir::new().unwrap();
        let engine: AsyncCacheEngine<Vec<f32>> = AsyncCacheEngine::open(CacheOptions {
            database_path: ":memory:".into(),
            ..CacheOptions::default()
        })
        .await
        .unwrap();

        let p1 = write_file(&dir, "ab1.txt", b"x");
        let p2 = write_file(&dir, "ab2.txt", b"y");

        let items = vec![(p1.clone(), vec![1.0_f32]), (p2.clone(), vec![2.0_f32])];
        let report = engine.batch_set(items).await.unwrap();
        assert_eq!(report.succeeded, 2);

        let results = engine.batch_get(vec![p1, p2]).await;
        assert_eq!(results.len(), 2);
        assert!(results[0].as_ref().unwrap().is_some());
        assert!(results[1].as_ref().unwrap().is_some());
    }

    #[tokio::test]
    async fn async_scan_dir() {
        let dir = TempDir::new().unwrap();
        let engine: AsyncCacheEngine<Vec<f32>> = AsyncCacheEngine::open(CacheOptions {
            database_path: ":memory:".into(),
            ..CacheOptions::default()
        })
        .await
        .unwrap();

        let f1 = write_file(&dir, "sc1.txt", b"a");
        let f2 = write_file(&dir, "sc2.txt", b"b");

        engine.set(f1.clone(), vec![1.0_f32]).await.unwrap();

        let results = engine
            .scan_dir(dir.path().to_path_buf(), false)
            .await
            .unwrap();
        assert_eq!(results.len(), 2);

        let fresh_count = results
            .iter()
            .filter(|(_, s)| *s == CacheStatus::Fresh)
            .count();
        let missing_count = results
            .iter()
            .filter(|(_, s)| *s == CacheStatus::Missing)
            .count();
        assert_eq!(fresh_count, 1);
        assert_eq!(missing_count, 1);

        let _ = (f1, f2); // suppress unused warnings
    }

    #[tokio::test]
    async fn async_engine_is_clone_safe() {
        // Two clones of AsyncCacheEngine share the same underlying DB.
        let dir = TempDir::new().unwrap();
        let engine: AsyncCacheEngine<Vec<f32>> = AsyncCacheEngine::open(CacheOptions {
            database_path: ":memory:".into(),
            ..CacheOptions::default()
        })
        .await
        .unwrap();
        let engine2 = engine.clone();

        let path = write_file(&dir, "clone.txt", b"shared");
        engine.set(path.clone(), vec![7.0_f32]).await.unwrap();

        // engine2 must see the same entry.
        let entry = engine2.get(path).await.unwrap().unwrap();
        assert_eq!(entry.payload, vec![7.0_f32]);
    }
}

// ====================================================================
// Phase 4 — Compression (requires `compression` feature)
// ====================================================================

#[cfg(feature = "compression")]
mod compression_tests {
    use super::*;

    #[test]
    fn compressed_payload_roundtrip() {
        let dir = TempDir::new().unwrap();
        let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: ":memory:".into(),
            compress_payloads: true,
            ..CacheOptions::default()
        })
        .unwrap();

        let path = write_file(&dir, "comp.txt", b"content");
        let payload: Vec<f32> = (0..1000).map(|i| i as f32).collect();

        engine.set(&path, &payload).unwrap();
        let entry = engine.get(&path).unwrap().unwrap();
        assert_eq!(entry.payload, payload);
    }

    #[test]
    fn compressed_entry_is_fresh() {
        let dir = TempDir::new().unwrap();
        let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: ":memory:".into(),
            compress_payloads: true,
            ..CacheOptions::default()
        })
        .unwrap();

        let path = write_file(&dir, "comp_fresh.txt", b"stable");
        engine.set(&path, &vec![1.0_f32]).unwrap();
        assert_eq!(engine.check_status(&path).unwrap(), CacheStatus::Fresh);
        assert!(engine.get_if_fresh(&path).unwrap().is_some());
    }

    #[test]
    fn uncompressed_and_compressed_coexist() {
        // Two engines on the same DB: one writes raw, the other compressed.
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("mixed.sqlite3");

        let engine_raw: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: db.clone(),
            namespace: "raw_ns".to_owned(),
            compress_payloads: false,
            ..CacheOptions::default()
        })
        .unwrap();

        let engine_zstd: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: db,
            namespace: "zstd_ns".to_owned(),
            compress_payloads: true,
            ..CacheOptions::default()
        })
        .unwrap();

        let path = write_file(&dir, "mixed.txt", b"mixed");
        let payload = vec![1.234_f32; 100];

        engine_raw.set(&path, &payload).unwrap();
        engine_zstd.set(&path, &payload).unwrap();

        assert_eq!(engine_raw.get(&path).unwrap().unwrap().payload, payload);
        assert_eq!(engine_zstd.get(&path).unwrap().unwrap().payload, payload);
    }
}

// ====================================================================
// Phase 5 — JSON codec
// ====================================================================
