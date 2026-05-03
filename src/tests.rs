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

        let entry = engine.get(&path).unwrap().expect("entry must exist");
        assert_eq!(entry.payload, payload);
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
        // Entry is gone; removing again should return false.
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

        // Cleanup on ns_a should not touch ns_b's entry.
        assert_eq!(engine_a.cleanup_missing_files().unwrap(), 1);
        // ns_b still has 1 entry (path stored string), cleanup sees it missing too.
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
        let p_bad = dir.path().join("does_not_exist.txt"); // never created

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
        // p2 intentionally not set

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

        // Modify p_stale to make it stale.
        write_file(&dir, "stale2.txt", b"modified content!!");

        let results = engine.batch_get_fresh(&[p_fresh.clone(), p_stale.clone()]);
        assert!(results[0].as_ref().unwrap().is_some()); // fresh
        assert!(results[1].as_ref().unwrap().is_none()); // stale
    }

    // ====================================================================
    // Phase 2 — TTL
    // ====================================================================

    #[test]
    fn ttl_expired_entry_is_none() {
        let dir = TempDir::new().unwrap();
        // TTL of 0 seconds — entries expire immediately.
        let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: dir.path().join("ttl.sqlite3"),
            ttl: Some(Duration::from_secs(0)),
            ..CacheOptions::default()
        })
        .unwrap();

        let path = write_file(&dir, "ttl.txt", b"content");
        engine.set(&path, &vec![1.0_f32]).unwrap();

        // With TTL=0 the entry is always considered expired.
        assert!(engine.get_if_fresh(&path).unwrap().is_none());
        assert_eq!(engine.check_status(&path).unwrap(), CacheStatus::Stale);
    }

    #[test]
    fn ttl_not_expired_entry_is_fresh() {
        let dir = TempDir::new().unwrap();
        let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: dir.path().join("ttl2.sqlite3"),
            ttl: Some(Duration::from_secs(3600)), // 1 hour
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

        let removed = engine.cleanup_expired().unwrap();
        assert_eq!(removed, 1);
        // get() skips TTL, so we confirm by checking status.
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

        // Delete the file first.
        fs::remove_file(&path).unwrap();

        // remove() must still succeed (fall-back path matching).
        let deleted = engine.remove(&path).unwrap();
        assert!(
            deleted,
            "entry should be found and deleted even though file is gone"
        );
    }

    // ====================================================================
    // Phase 2 — schema migration (v1 → v2)
    // ====================================================================

    #[test]
    fn migrates_v1_database() {
        use rusqlite::Connection;

        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("migrate.sqlite3");

        // Build a minimal v1-style database by hand.
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

        // Opening with CacheEngine must complete migration without error.
        let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
            database_path: db_path,
            ..CacheOptions::default()
        })
        .unwrap();

        // Legacy data is accessible in the default namespace.
        // (We can't call get() because the file doesn't exist, but we can
        //  verify the row was migrated via check_status returning Missing.)
        let status = engine.check_status("/legacy/file.txt").unwrap();
        assert_eq!(status, CacheStatus::Missing);
    }
}
