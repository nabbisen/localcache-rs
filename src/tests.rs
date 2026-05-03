//! Integration tests for `localcache`.

#[cfg(test)]
mod integration {
    use std::fs;
    use std::io::Write;
    use std::time::Duration;

    use serde::{Deserialize, Serialize};
    use tempfile::TempDir;

    use crate::{
        CacheEngine, CacheOptions, CacheStatus, ChangeDetectionMode, JournalMode, SynchronousMode,
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
}
