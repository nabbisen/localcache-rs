//! Integration tests — core.

mod common;
use common::{make_engine, write_file};

use std::fs;
use std::time::Duration;

use localcache::{
    CacheEngine, CacheOptions, CacheStatus, ChangeDetectionMode, JournalMode, SynchronousMode,
};
use serde::{Deserialize, Serialize};
use tempfile::TempDir;

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

// RFC 025 R1 — one expiry rule. A backward clock step leaves an entry with an
// `updated_at` in the future; its age is zero, so it is fresh.

/// An engine with a one-hour TTL, one entry, and that entry's `updated_at`
/// moved `seconds_ahead` seconds past "now", as a clock stepped back leaves it.
fn clock_stepped_back(
    dir: &TempDir,
    seconds_ahead: i64,
) -> (CacheEngine<Vec<f32>>, std::path::PathBuf) {
    let database = dir.path().join("clock-back.sqlite3");
    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: database.clone(),
        ttl: Some(Duration::from_secs(3600)),
        ..CacheOptions::default()
    })
    .unwrap();
    let path = write_file(dir, "clock-back.txt", b"content");
    engine.set(&path, &vec![1.0_f32]).unwrap();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    rusqlite::Connection::open(&database)
        .unwrap()
        .execute(
            "UPDATE files SET updated_at = ?1",
            rusqlite::params![now + seconds_ahead],
        )
        .unwrap();
    (engine, path)
}

#[test]
fn an_entry_written_in_the_future_is_fresh_not_expired() {
    let dir = TempDir::new().unwrap();
    let (engine, path) = clock_stepped_back(&dir, 1000);
    assert!(
        engine.get_if_fresh(&path).unwrap().is_some(),
        "get_if_fresh must not treat a future updated_at as a wrapped huge age"
    );
    assert_eq!(engine.check_status(&path).unwrap(), CacheStatus::Fresh);
}

#[test]
fn cleanup_expired_keeps_an_entry_written_in_the_future() {
    let dir = TempDir::new().unwrap();
    let (engine, path) = clock_stepped_back(&dir, 1000);
    assert_eq!(engine.cleanup_expired().unwrap(), 0);
    assert!(engine.contains(&path).unwrap(), "the entry must remain");
}

#[test]
fn explain_agrees_with_the_reads_for_an_entry_written_in_the_future() {
    let dir = TempDir::new().unwrap();
    let (engine, path) = clock_stepped_back(&dir, 1000);
    let diagnosis = engine.explain(&path).unwrap();
    assert_eq!(diagnosis.status, CacheStatus::Fresh);
    assert_eq!(
        diagnosis.ttl_remaining_secs,
        Some(3600),
        "age counts as zero, so the whole TTL remains"
    );
}

#[test]
fn explain_with_a_huge_ttl_reports_a_huge_remaining_time_not_zero() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: dir.path().join("huge-ttl.sqlite3"),
        ttl: Some(Duration::MAX),
        ..CacheOptions::default()
    })
    .unwrap();
    let path = write_file(&dir, "huge-ttl.txt", b"content");
    engine.set(&path, &vec![1.0_f32]).unwrap();

    assert!(engine.get_if_fresh(&path).unwrap().is_some());
    let diagnosis = engine.explain(&path).unwrap();
    assert_eq!(diagnosis.status, CacheStatus::Fresh);
    assert_eq!(
        diagnosis.ttl_remaining_secs,
        Some(i64::MAX),
        "a remaining time too large for i64 saturates; it must never read 0 while the entry is fresh"
    );
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
            CREATE INDEX idx_files_path ON files(path);
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

// RFC 022 Q0a — CacheEngine's auto traits must not change when
// `encryption_key` becomes a `Cell`. Observed on v0.21.3, before the Cell
// change: `CacheEngine<Vec<f32>>` is Send, but already !Sync (rusqlite's
// `Connection` holds a `RefCell`-based statement cache), already
// !UnwindSafe, and already !RefUnwindSafe (the `RefCell` statement cache
// again, plus the `dyn Fn` in `evict_callback`, neither of which is
// `RefUnwindSafe`). The Cell added for key rotation must not change any of
// the four — it only makes the existing !RefUnwindSafe status "more true".
//
// Not gated on `encryption` (RFC 022 R1 Amendment 2 / R-a): these four
// properties are facts about `CacheEngine` in every configuration, driven
// by `rusqlite::Connection` and `evict_callback` regardless of which
// optional fields are compiled in. Gating this module would hide a future
// auto-trait regression in every other feature row.
mod auto_traits {
    fn assert_send<T: Send>() {}

    #[test]
    fn cache_engine_is_send() {
        assert_send::<localcache::CacheEngine<Vec<f32>>>();
    }

    // The ambiguity trick: this only compiles when `T` does NOT implement
    // the trait. When `T` DOES implement it, both `AmbiguousIf*::<()>`
    // (the unconditional blanket impl) and `AmbiguousIf*::<u8>` (the
    // trait-bounded impl) apply, and `check` is ambiguous — a compile
    // error. When `T` lacks the trait, only the `()` impl applies, so the
    // item compiles only if the trait is genuinely absent.

    trait AmbiguousIfSync<A> {
        fn check() {}
    }
    impl<T: ?Sized> AmbiguousIfSync<()> for T {}
    impl<T: ?Sized + Sync> AmbiguousIfSync<u8> for T {}
    #[test]
    fn cache_engine_is_not_sync() {
        let _ = <localcache::CacheEngine<Vec<f32>> as AmbiguousIfSync<_>>::check;
    }

    trait AmbiguousIfUnwindSafe<A> {
        fn check() {}
    }
    impl<T: ?Sized> AmbiguousIfUnwindSafe<()> for T {}
    impl<T: ?Sized + std::panic::UnwindSafe> AmbiguousIfUnwindSafe<u8> for T {}
    #[test]
    fn cache_engine_is_not_unwind_safe() {
        let _ = <localcache::CacheEngine<Vec<f32>> as AmbiguousIfUnwindSafe<_>>::check;
    }

    trait AmbiguousIfRefUnwindSafe<A> {
        fn check() {}
    }
    impl<T: ?Sized> AmbiguousIfRefUnwindSafe<()> for T {}
    impl<T: ?Sized + std::panic::RefUnwindSafe> AmbiguousIfRefUnwindSafe<u8> for T {}
    #[test]
    fn cache_engine_is_not_ref_unwind_safe() {
        let _ = <localcache::CacheEngine<Vec<f32>> as AmbiguousIfRefUnwindSafe<_>>::check;
    }
}
