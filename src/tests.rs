//! Integration tests for `localcache`.

#[cfg(test)]
mod integration {
    use std::fs;
    use std::io::Write;

    use serde::{Deserialize, Serialize};
    use tempfile::TempDir;

    use crate::{CacheEngine, CacheOptions, CacheStatus, ChangeDetectionMode};

    // ------------------------------------------------------------------
    // Helpers
    // ------------------------------------------------------------------

    fn make_engine(dir: &TempDir, mode: ChangeDetectionMode) -> CacheEngine<Vec<f32>> {
        CacheEngine::open(CacheOptions {
            database_path: dir.path().join("test.sqlite3"),
            change_detection_mode: mode,
        })
        .unwrap()
    }

    fn write_file(dir: &TempDir, name: &str, content: &[u8]) -> std::path::PathBuf {
        let path = dir.path().join(name);
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(content).unwrap();
        path
    }

    // ------------------------------------------------------------------
    // 16.1 Basic operations
    // ------------------------------------------------------------------

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
        let payload = vec![4.0_f32];

        engine.set(&path, &payload).unwrap();
        let deleted = engine.remove(&path).unwrap();
        assert!(deleted, "remove should return true when entry existed");

        let result = engine.get(&path).unwrap();
        assert!(result.is_none(), "entry must be gone after remove");
    }

    #[test]
    fn get_missing_key_returns_none() {
        let dir = TempDir::new().unwrap();
        let engine = make_engine(&dir, ChangeDetectionMode::MetadataOnly);
        let path = write_file(&dir, "c.txt", b"x");

        // Never called set, so entry should not exist.
        let result = engine.get(&path).unwrap();
        assert!(result.is_none());
    }

    // ------------------------------------------------------------------
    // 16.2 Change detection
    // ------------------------------------------------------------------

    #[test]
    fn unchanged_file_is_fresh() {
        let dir = TempDir::new().unwrap();
        let engine = make_engine(&dir, ChangeDetectionMode::MetadataOnly);
        let path = write_file(&dir, "d.txt", b"stable");

        engine.set(&path, &vec![0.0_f32]).unwrap();
        let status = engine.check_status(&path).unwrap();
        assert_eq!(status, CacheStatus::Fresh);
    }

    #[test]
    fn modified_file_is_stale_metadata() {
        let dir = TempDir::new().unwrap();
        let engine = make_engine(&dir, ChangeDetectionMode::MetadataOnly);
        let path = write_file(&dir, "e.txt", b"original");
        engine.set(&path, &vec![0.0_f32]).unwrap();

        // Overwrite with different content (different size → metadata change).
        write_file(&dir, "e.txt", b"modified content that is longer");

        let status = engine.check_status(&path).unwrap();
        assert_eq!(status, CacheStatus::Stale);
    }

    #[test]
    fn modified_file_is_stale_full_hash() {
        let dir = TempDir::new().unwrap();
        let engine = make_engine(&dir, ChangeDetectionMode::StrictFullHash);
        let path = write_file(&dir, "f.txt", b"original");
        engine.set(&path, &vec![0.0_f32]).unwrap();

        write_file(&dir, "f.txt", b"changed!!");

        let status = engine.check_status(&path).unwrap();
        assert_eq!(status, CacheStatus::Stale);
    }

    #[test]
    fn deleted_file_is_missing() {
        let dir = TempDir::new().unwrap();
        let engine = make_engine(&dir, ChangeDetectionMode::MetadataOnly);
        let path = write_file(&dir, "g.txt", b"will be deleted");
        engine.set(&path, &vec![0.0_f32]).unwrap();

        fs::remove_file(&path).unwrap();

        let status = engine.check_status(&path).unwrap();
        assert_eq!(status, CacheStatus::Missing);
    }

    // ------------------------------------------------------------------
    // 16.3 Cleanup
    // ------------------------------------------------------------------

    #[test]
    fn cleanup_removes_missing_files() {
        let dir = TempDir::new().unwrap();
        let engine = make_engine(&dir, ChangeDetectionMode::MetadataOnly);

        let path_keep = write_file(&dir, "keep.txt", b"keep");
        let path_del = write_file(&dir, "del.txt", b"delete me");

        engine.set(&path_keep, &vec![1.0_f32]).unwrap();
        engine.set(&path_del, &vec![2.0_f32]).unwrap();

        fs::remove_file(&path_del).unwrap();

        let removed = engine.cleanup_missing_files().unwrap();
        assert_eq!(removed, 1);

        // The kept entry must still be there.
        assert!(engine.get(&path_keep).unwrap().is_some());
    }

    #[test]
    fn cleanup_cascade_deletes_payload() {
        let dir = TempDir::new().unwrap();

        // Use a separate engine to inspect the DB directly via a second
        // connection would require public DB access; instead we confirm the
        // entry is gone from the public API.
        let engine = make_engine(&dir, ChangeDetectionMode::MetadataOnly);
        let path = write_file(&dir, "cascade.txt", b"data");
        engine.set(&path, &vec![9.0_f32]).unwrap();

        fs::remove_file(&path).unwrap();
        let removed = engine.cleanup_missing_files().unwrap();
        assert_eq!(removed, 1);

        // A fresh engine pointing at the same DB should see no entry.
        let engine2 = make_engine(&dir, ChangeDetectionMode::MetadataOnly);
        // File is gone so get() would fail with FileNotFound – use direct DB
        // absence check: try removing a non-existent path.
        let deleted_again = engine2
            .remove(dir.path().join("cascade.txt"))
            .unwrap_or(false);
        assert!(!deleted_again, "entry should have been cascade-deleted");
    }

    // ------------------------------------------------------------------
    // 16.4 Payload types
    // ------------------------------------------------------------------

    #[test]
    fn vec_f32_roundtrip() {
        let dir = TempDir::new().unwrap();
        let engine = make_engine(&dir, ChangeDetectionMode::MetadataOnly);
        let path = write_file(&dir, "vec.txt", b"vec content");
        let payload = vec![0.1_f32, 0.2, 0.3, 0.4, 0.5];

        engine.set(&path, &payload).unwrap();
        let entry = engine.get(&path).unwrap().unwrap();
        assert_eq!(entry.payload, payload);
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
            change_detection_mode: ChangeDetectionMode::MetadataOnly,
        })
        .unwrap();

        let path = write_file(&dir, "struct.txt", b"struct content");
        let payload = MyStruct {
            label: "test".to_owned(),
            values: vec![1.1, 2.2, 3.3],
            count: 42,
        };

        engine.set(&path, &payload).unwrap();
        let entry = engine.get(&path).unwrap().unwrap();
        assert_eq!(entry.payload, payload);
    }

    // ------------------------------------------------------------------
    // 16.5 Upsert behaviour
    // ------------------------------------------------------------------

    #[test]
    fn repeated_set_upserts() {
        let dir = TempDir::new().unwrap();
        let engine = make_engine(&dir, ChangeDetectionMode::MetadataOnly);
        let path = write_file(&dir, "upsert.txt", b"content");

        engine.set(&path, &vec![1.0_f32]).unwrap();
        engine.set(&path, &vec![9.9_f32]).unwrap(); // overwrite

        let entry = engine.get(&path).unwrap().unwrap();
        assert_eq!(entry.payload, vec![9.9_f32], "payload must be updated");
    }

    // ------------------------------------------------------------------
    // get_if_fresh
    // ------------------------------------------------------------------

    #[test]
    fn get_if_fresh_returns_entry_when_unchanged() {
        let dir = TempDir::new().unwrap();
        let engine = make_engine(&dir, ChangeDetectionMode::MetadataOnly);
        let path = write_file(&dir, "fresh.txt", b"stable");

        engine.set(&path, &vec![7.0_f32]).unwrap();

        let entry = engine.get_if_fresh(&path).unwrap();
        assert!(entry.is_some(), "should be fresh");
    }

    #[test]
    fn get_if_fresh_returns_none_when_stale() {
        let dir = TempDir::new().unwrap();
        let engine = make_engine(&dir, ChangeDetectionMode::MetadataOnly);
        let path = write_file(&dir, "stale.txt", b"original");
        engine.set(&path, &vec![7.0_f32]).unwrap();

        write_file(&dir, "stale.txt", b"bigger content now!!");

        let entry = engine.get_if_fresh(&path).unwrap();
        assert!(entry.is_none(), "should be None when stale");
    }
}
