//! Integration tests for `localcache`.

#[cfg(test)]
mod integration {
    use std::fs;
    use std::io::Write;
    use std::time::Duration;

    use serde::{Deserialize, Serialize};
    use tempfile::TempDir;

    use crate::{
        CacheEngine, CacheOptions, CacheStatus, ChangeDetectionMode, JournalMode, ScanOptions,
        SynchronousMode,
    };

    // ------------------------------------------------------------------
    // Helpers
    // ------------------------------------------------------------------

    fn make_engine(dir: &TempDir, mode: ChangeDetectionMode) -> CacheEngine<Vec<f32>> {
        CacheEngine::open(CacheOptions {
            database_path: dir.path().join("test.sqlite3"),
            change_detection_mode: mode,
            ..CacheOptions::default()
        })
        .unwrap()
    }

    fn write_file(dir: &TempDir, name: &str, content: &[u8]) -> std::path::PathBuf {
        let path = dir.path().join(name);
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(content).unwrap();
        path
    }

    // ====================================================================
    // Phase 1 — Basic operations
    // ====================================================================

    #[test]
    fn set_then_get() {
        let dir = TempDir::new().unwrap();
        let engine = make_engine(&dir, ChangeDetectionMode::MetadataOnly);
        let path = write_file(&dir, "a.txt", b"hello");
        let payload = vec![1.0_f32, 2.0, 3.0];
        engine.set(&path, &payload).unwrap();
        assert_eq!(engine.get(&path).unwrap().unwrap().payload, payload);
    }

    #[test]
    fn remove_deletes_entry() {
        let dir = TempDir::new().unwrap();
        let engine = make_engine(&dir, ChangeDetectionMode::MetadataOnly);
        let path = write_file(&dir, "b.txt", b"world");
        engine.set(&path, &vec![4.0_f32]).unwrap();
        assert!(engine.remove(&path).unwrap());
        assert!(engine.get(&path).unwrap().is_none());
    }

    #[test]
    fn get_missing_key_returns_none() {
        let dir = TempDir::new().unwrap();
        let engine = make_engine(&dir, ChangeDetectionMode::MetadataOnly);
        let path = write_file(&dir, "c.txt", b"x");
        assert!(engine.get(&path).unwrap().is_none());
    }

    // ====================================================================
    // Phase 1 — Change detection
    // ====================================================================

    #[test]
    fn unchanged_file_is_fresh() {
        let dir = TempDir::new().unwrap();
        let engine = make_engine(&dir, ChangeDetectionMode::MetadataOnly);
        let path = write_file(&dir, "d.txt", b"stable");
        engine.set(&path, &vec![0.0_f32]).unwrap();
        assert_eq!(engine.check_status(&path).unwrap(), CacheStatus::Fresh);
    }

    #[test]
    fn modified_file_is_stale_metadata() {
        let dir = TempDir::new().unwrap();
        let engine = make_engine(&dir, ChangeDetectionMode::MetadataOnly);
        let path = write_file(&dir, "e.txt", b"original");
        engine.set(&path, &vec![0.0_f32]).unwrap();
        write_file(&dir, "e.txt", b"modified content that is longer");
        assert_eq!(engine.check_status(&path).unwrap(), CacheStatus::Stale);
    }

    #[test]
    fn modified_file_is_stale_full_hash() {
        let dir = TempDir::new().unwrap();
        let engine = make_engine(&dir, ChangeDetectionMode::StrictFullHash);
        let path = write_file(&dir, "f.txt", b"original");
        engine.set(&path, &vec![0.0_f32]).unwrap();
        write_file(&dir, "f.txt", b"changed!!");
        assert_eq!(engine.check_status(&path).unwrap(), CacheStatus::Stale);
    }

    #[test]
    fn deleted_file_is_missing() {
        let dir = TempDir::new().unwrap();
        let engine = make_engine(&dir, ChangeDetectionMode::MetadataOnly);
        let path = write_file(&dir, "g.txt", b"will be deleted");
        engine.set(&path, &vec![0.0_f32]).unwrap();
        fs::remove_file(&path).unwrap();
        assert_eq!(engine.check_status(&path).unwrap(), CacheStatus::Missing);
    }

    // ====================================================================
    // Phase 1 — Cleanup
    // ====================================================================

    #[test]
    fn cleanup_removes_missing_files() {
        let dir = TempDir::new().unwrap();
        let engine = make_engine(&dir, ChangeDetectionMode::MetadataOnly);
        let keep = write_file(&dir, "keep.txt", b"keep");
        let del = write_file(&dir, "del.txt", b"delete me");
        engine.set(&keep, &vec![1.0_f32]).unwrap();
        engine.set(&del, &vec![2.0_f32]).unwrap();
        fs::remove_file(&del).unwrap();
        assert_eq!(engine.cleanup_missing_files().unwrap(), 1);
        assert!(engine.get(&keep).unwrap().is_some());
    }

    #[test]
    fn cleanup_cascade_deletes_payload() {
        let dir = TempDir::new().unwrap();
        let engine = make_engine(&dir, ChangeDetectionMode::MetadataOnly);
        let path = write_file(&dir, "cascade.txt", b"data");
        engine.set(&path, &vec![9.0_f32]).unwrap();
        fs::remove_file(&path).unwrap();
        assert_eq!(engine.cleanup_missing_files().unwrap(), 1);
        assert!(
            !engine
                .remove(dir.path().join("cascade.txt"))
                .unwrap_or(false)
        );
    }

    // ====================================================================
    // Phase 1 — Payload types
    // ====================================================================

    #[test]
    fn vec_f32_roundtrip() {
        let dir = TempDir::new().unwrap();
        let engine = make_engine(&dir, ChangeDetectionMode::MetadataOnly);
        let path = write_file(&dir, "vec.txt", b"vec content");
        let payload = vec![0.1_f32, 0.2, 0.3, 0.4, 0.5];
        engine.set(&path, &payload).unwrap();
        assert_eq!(engine.get(&path).unwrap().unwrap().payload, payload);
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct MyStruct {
        label: String,
        values: Vec<f64>,
        count: u32,
    }

    #[test]
    fn custom_struct_roundtrip() {
        let dir = TempDir::new().unwrap();
        let engine: CacheEngine<MyStruct> = CacheEngine::open(CacheOptions {
            database_path: dir.path().join("custom.sqlite3"),
            ..CacheOptions::default()
        })
        .unwrap();
        let path = write_file(&dir, "struct.txt", b"struct content");
        let payload = MyStruct {
            label: "test".to_owned(),
            values: vec![1.1, 2.2, 3.3],
            count: 42,
        };
        engine.set(&path, &payload).unwrap();
        assert_eq!(engine.get(&path).unwrap().unwrap().payload, payload);
    }

    // ====================================================================
    // Phase 1 — Upsert
    // ====================================================================

    #[test]
    fn repeated_set_upserts() {
        let dir = TempDir::new().unwrap();
        let engine = make_engine(&dir, ChangeDetectionMode::MetadataOnly);
        let path = write_file(&dir, "upsert.txt", b"content");
        engine.set(&path, &vec![1.0_f32]).unwrap();
        engine.set(&path, &vec![9.9_f32]).unwrap();
        assert_eq!(engine.get(&path).unwrap().unwrap().payload, vec![9.9_f32]);
    }

    // ====================================================================
    // Phase 1 — get_if_fresh
    // ====================================================================

    #[test]
    fn get_if_fresh_returns_entry_when_unchanged() {
        let dir = TempDir::new().unwrap();
        let engine = make_engine(&dir, ChangeDetectionMode::MetadataOnly);
        let path = write_file(&dir, "fresh.txt", b"stable");
        engine.set(&path, &vec![7.0_f32]).unwrap();
        assert!(engine.get_if_fresh(&path).unwrap().is_some());
    }

    #[test]
    fn get_if_fresh_returns_none_when_stale() {
        let dir = TempDir::new().unwrap();
        let engine = make_engine(&dir, ChangeDetectionMode::MetadataOnly);
        let path = write_file(&dir, "stale.txt", b"original");
        engine.set(&path, &vec![7.0_f32]).unwrap();
        write_file(&dir, "stale.txt", b"bigger content now!!");
        assert!(engine.get_if_fresh(&path).unwrap().is_none());
    }

    // ====================================================================
    // Phase 2 — Namespaces
    // ====================================================================

    #[test]
    fn namespaces_isolate_entries() {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("ns.sqlite3");
        let engine_a: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: db.clone(),
            namespace: "embeddings".to_owned(),
            ..CacheOptions::default()
        })
        .unwrap();
        let engine_b: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: db.clone(),
            namespace: "thumbnails".to_owned(),
            ..CacheOptions::default()
        })
        .unwrap();
        let path = write_file(&dir, "shared.txt", b"content");
        engine_a.set(&path, &vec![1.0_f32]).unwrap();
        engine_b.set(&path, &vec![2.0_f32]).unwrap();
        assert_eq!(engine_a.get(&path).unwrap().unwrap().payload, vec![1.0_f32]);
        assert_eq!(engine_b.get(&path).unwrap().unwrap().payload, vec![2.0_f32]);
    }

    #[test]
    fn cleanup_scoped_to_namespace() {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("nsclean.sqlite3");
        let engine_a: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: db.clone(),
            namespace: "ns_a".to_owned(),
            ..CacheOptions::default()
        })
        .unwrap();
        let engine_b: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: db.clone(),
            namespace: "ns_b".to_owned(),
            ..CacheOptions::default()
        })
        .unwrap();
        let path = write_file(&dir, "shared2.txt", b"hello");
        engine_a.set(&path, &vec![1.0_f32]).unwrap();
        engine_b.set(&path, &vec![2.0_f32]).unwrap();
        fs::remove_file(&path).unwrap();
        assert_eq!(engine_a.cleanup_missing_files().unwrap(), 1);
        assert_eq!(engine_b.cleanup_missing_files().unwrap(), 1);
    }

    // ====================================================================
    // Phase 2 — Batch set / get
    // ====================================================================

    #[test]
    fn batch_set_stores_all_entries() {
        let dir = TempDir::new().unwrap();
        let engine = make_engine(&dir, ChangeDetectionMode::MetadataOnly);
        let p1 = write_file(&dir, "b1.txt", b"file1");
        let p2 = write_file(&dir, "b2.txt", b"file2");
        let p3 = write_file(&dir, "b3.txt", b"file3");
        let items = vec![
            (p1.clone(), vec![1.0_f32]),
            (p2.clone(), vec![2.0_f32]),
            (p3.clone(), vec![3.0_f32]),
        ];
        let report = engine.batch_set(&items).unwrap();
        assert_eq!(report.succeeded, 3);
        assert!(report.failed.is_empty());
        assert_eq!(engine.get(&p1).unwrap().unwrap().payload, vec![1.0_f32]);
        assert_eq!(engine.get(&p2).unwrap().unwrap().payload, vec![2.0_f32]);
        assert_eq!(engine.get(&p3).unwrap().unwrap().payload, vec![3.0_f32]);
    }

    #[test]
    fn batch_set_partial_failure() {
        let dir = TempDir::new().unwrap();
        let engine = make_engine(&dir, ChangeDetectionMode::MetadataOnly);
        let p_good = write_file(&dir, "good.txt", b"exists");
        let p_bad = dir.path().join("does_not_exist.txt");
        let items = vec![(p_good.clone(), vec![1.0_f32]), (p_bad, vec![2.0_f32])];
        let report = engine.batch_set(&items).unwrap();
        assert_eq!(report.succeeded, 1);
        assert_eq!(report.failed.len(), 1);
        assert!(engine.get(&p_good).unwrap().is_some());
    }

    #[test]
    fn batch_get_returns_results_in_order() {
        let dir = TempDir::new().unwrap();
        let engine = make_engine(&dir, ChangeDetectionMode::MetadataOnly);
        let p1 = write_file(&dir, "g1.txt", b"x");
        let p2 = write_file(&dir, "g2.txt", b"y");
        engine.set(&p1, &vec![10.0_f32]).unwrap();
        let results = engine.batch_get(&[p1.clone(), p2.clone()]);
        assert_eq!(results.len(), 2);
        assert_eq!(
            results[0].as_ref().unwrap().as_ref().unwrap().payload,
            vec![10.0_f32]
        );
        assert!(results[1].as_ref().unwrap().is_none());
    }

    #[test]
    fn batch_get_fresh_filters_stale() {
        let dir = TempDir::new().unwrap();
        let engine = make_engine(&dir, ChangeDetectionMode::MetadataOnly);
        let p_fresh = write_file(&dir, "fresh2.txt", b"stable");
        let p_stale = write_file(&dir, "stale2.txt", b"original");
        engine.set(&p_fresh, &vec![1.0_f32]).unwrap();
        engine.set(&p_stale, &vec![2.0_f32]).unwrap();
        write_file(&dir, "stale2.txt", b"modified content!!");
        let results = engine.batch_get_fresh(&[p_fresh.clone(), p_stale.clone()]);
        assert!(results[0].as_ref().unwrap().is_some());
        assert!(results[1].as_ref().unwrap().is_none());
    }

    // ====================================================================
    // Phase 2 — TTL
    // ====================================================================

    #[test]
    fn ttl_expired_entry_is_none() {
        let dir = TempDir::new().unwrap();
        let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: dir.path().join("ttl.sqlite3"),
            ttl: Some(Duration::from_secs(0)),
            ..CacheOptions::default()
        })
        .unwrap();
        let path = write_file(&dir, "ttl.txt", b"content");
        engine.set(&path, &vec![1.0_f32]).unwrap();
        assert!(engine.get_if_fresh(&path).unwrap().is_none());
        assert_eq!(engine.check_status(&path).unwrap(), CacheStatus::Stale);
    }

    #[test]
    fn ttl_not_expired_entry_is_fresh() {
        let dir = TempDir::new().unwrap();
        let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: dir.path().join("ttl2.sqlite3"),
            ttl: Some(Duration::from_secs(3600)),
            ..CacheOptions::default()
        })
        .unwrap();
        let path = write_file(&dir, "ttl2.txt", b"content");
        engine.set(&path, &vec![1.0_f32]).unwrap();
        assert!(engine.get_if_fresh(&path).unwrap().is_some());
    }

    #[test]
    fn cleanup_expired_removes_old_entries() {
        let dir = TempDir::new().unwrap();
        let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: dir.path().join("ttlclean.sqlite3"),
            ttl: Some(Duration::from_secs(0)),
            ..CacheOptions::default()
        })
        .unwrap();
        let path = write_file(&dir, "exp.txt", b"content");
        engine.set(&path, &vec![1.0_f32]).unwrap();
        assert_eq!(engine.cleanup_expired().unwrap(), 1);
        assert_eq!(engine.check_status(&path).unwrap(), CacheStatus::Missing);
    }

    // ====================================================================
    // Phase 2 — journal_mode / synchronous
    // ====================================================================

    #[test]
    fn delete_journal_mode_works() {
        let dir = TempDir::new().unwrap();
        let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: dir.path().join("journal.sqlite3"),
            journal_mode: JournalMode::Delete,
            synchronous: SynchronousMode::Full,
            ..CacheOptions::default()
        })
        .unwrap();
        let path = write_file(&dir, "j.txt", b"data");
        engine.set(&path, &vec![5.0_f32]).unwrap();
        assert_eq!(engine.get(&path).unwrap().unwrap().payload, vec![5.0_f32]);
    }

    // ====================================================================
    // Phase 2 — remove of missing-file paths
    // ====================================================================

    #[test]
    fn remove_after_file_deleted() {
        let dir = TempDir::new().unwrap();
        let engine = make_engine(&dir, ChangeDetectionMode::MetadataOnly);
        let path = write_file(&dir, "gone.txt", b"bye");
        engine.set(&path, &vec![1.0_f32]).unwrap();
        fs::remove_file(&path).unwrap();
        assert!(engine.remove(&path).unwrap());
    }

    // ====================================================================
    // Phase 2 — schema migration (v1 → v2)
    // ====================================================================

    #[test]
    fn migrates_v1_database() {
        use rusqlite::Connection;

        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("migrate.sqlite3");
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "
                PRAGMA user_version = 1;
                CREATE TABLE files (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    path TEXT NOT NULL UNIQUE,
                    mtime INTEGER NOT NULL,
                    file_size INTEGER NOT NULL,
                    hash TEXT,
                    updated_at INTEGER NOT NULL
                );
                CREATE TABLE payloads (
                    file_id INTEGER PRIMARY KEY,
                    content BLOB NOT NULL,
                    FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
                );
                INSERT INTO files (path, mtime, file_size, updated_at)
                VALUES ('/legacy/file.txt', 1000, 42, 1000);
                ",
            )
            .unwrap();
        }
        let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: db_path,
            ..CacheOptions::default()
        })
        .unwrap();
        assert_eq!(
            engine.check_status("/legacy/file.txt").unwrap(),
            CacheStatus::Missing
        );
    }

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
        use crate::detection::hash::PARTIAL_PREFIX;

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
            hash.starts_with(PARTIAL_PREFIX),
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
            matches!(result, Err(crate::LocalFileCacheError::ReadOnly)),
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
            Err(crate::LocalFileCacheError::ReadOnly)
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
            Err(crate::LocalFileCacheError::ReadOnly)
        ));
        assert!(matches!(
            ro.shrink_database(),
            Err(crate::LocalFileCacheError::ReadOnly)
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
        use crate::AsyncCacheEngine;

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

    #[cfg(feature = "json")]
    mod json_tests {
        use super::*;
        use crate::Codec;

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
        use crate::{AsyncCacheEngine, ScanOptions};

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
}
