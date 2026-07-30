//! Integration tests — ReadPool (RFC 007).

mod common;
use common::write_file;

use std::thread;

use tempfile::TempDir;

use localcache::{CacheEngine, CacheOptions, LocalFileCacheError, ReadPool};

fn make_populated_db(dir: &TempDir) -> std::path::PathBuf {
    let db = dir.path().join("pool.sqlite3");
    let engine: CacheEngine<Vec<f32>> = CacheEngine::builder().database(&db).build().unwrap();
    for i in 0..10u32 {
        let p = write_file(dir, &format!("f{i:02}.txt"), b"x");
        engine.set(&p, &vec![i as f32]).unwrap();
    }
    db
}

// ---------------------------------------------------------------------------
// Basic read operations
// ---------------------------------------------------------------------------

#[test]
fn read_pool_get_returns_stored_entry() {
    let dir = TempDir::new().unwrap();
    let db = make_populated_db(&dir);
    let pool: ReadPool<Vec<f32>> = ReadPool::open(
        CacheOptions {
            database_path: db,
            ..CacheOptions::default()
        },
        2,
    )
    .unwrap();

    let path = dir.path().join("f00.txt");
    let entry = pool.get_if_fresh(&path).unwrap();
    assert!(
        entry.is_some(),
        "pool.get_if_fresh must return stored entry"
    );
}

#[test]
fn read_pool_keys_returns_all_entries() {
    let dir = TempDir::new().unwrap();
    let db = make_populated_db(&dir);
    let pool: ReadPool<Vec<f32>> = ReadPool::open(
        CacheOptions {
            database_path: db,
            ..CacheOptions::default()
        },
        2,
    )
    .unwrap();

    let keys = pool.keys(None).unwrap();
    assert_eq!(keys.len(), 10);
}

#[test]
fn read_pool_entry_count_matches_writer() {
    let dir = TempDir::new().unwrap();
    let db = make_populated_db(&dir);
    let pool: ReadPool<Vec<f32>> = ReadPool::open(
        CacheOptions {
            database_path: db,
            ..CacheOptions::default()
        },
        2,
    )
    .unwrap();

    assert_eq!(pool.entry_count().unwrap(), 10);
}

#[test]
fn read_pool_contains_returns_true_for_stored_path() {
    let dir = TempDir::new().unwrap();
    let db = make_populated_db(&dir);
    let pool: ReadPool<Vec<f32>> = ReadPool::open(
        CacheOptions {
            database_path: db,
            ..CacheOptions::default()
        },
        2,
    )
    .unwrap();

    let path = dir.path().join("f05.txt");
    assert!(pool.contains(&path).unwrap());
}

#[test]
fn read_pool_check_status_returns_fresh() {
    let dir = TempDir::new().unwrap();
    let db = make_populated_db(&dir);
    let pool: ReadPool<Vec<f32>> = ReadPool::open(
        CacheOptions {
            database_path: db,
            ..CacheOptions::default()
        },
        2,
    )
    .unwrap();

    let path = dir.path().join("f01.txt");
    let status = pool.check_status(&path).unwrap();
    assert!(
        matches!(status, localcache::CacheStatus::Fresh),
        "unmodified file must be Fresh"
    );
}

// ---------------------------------------------------------------------------
// query_run / query_dry_run
// ---------------------------------------------------------------------------

#[test]
fn read_pool_query_run_with_path_in_dir() {
    let dir = TempDir::new().unwrap();
    let db = make_populated_db(&dir);
    let pool: ReadPool<Vec<f32>> = ReadPool::open(
        CacheOptions {
            database_path: db,
            ..CacheOptions::default()
        },
        2,
    )
    .unwrap();

    let results = pool
        .query_run(|q| q.path_in_dir(dir.path(), false))
        .unwrap();
    assert_eq!(results.len(), 10, "all entries are in the root dir");
}

#[test]
fn read_pool_query_dry_run_returns_plan() {
    let dir = TempDir::new().unwrap();
    let db = make_populated_db(&dir);
    let pool: ReadPool<Vec<f32>> = ReadPool::open(
        CacheOptions {
            database_path: db,
            ..CacheOptions::default()
        },
        2,
    )
    .unwrap();

    let plan = pool.query_dry_run(|q| q.path_like("%.txt")).unwrap();
    assert!(!plan.is_empty());
}

// ---------------------------------------------------------------------------
// Concurrency
// ---------------------------------------------------------------------------

#[test]
fn read_pool_concurrent_lookups_across_threads() {
    let dir = TempDir::new().unwrap();
    let db = make_populated_db(&dir);

    let pool: ReadPool<Vec<f32>> = ReadPool::open(
        CacheOptions {
            database_path: db,
            ..CacheOptions::default()
        },
        4,
    )
    .unwrap();

    // 8 threads each performing 50 get_if_fresh calls.
    let mut handles = Vec::new();
    for t in 0..8u32 {
        let pool = pool.clone();
        let path = dir.path().join(format!("f0{}.txt", t % 10));
        handles.push(thread::spawn(move || {
            for _ in 0..50 {
                pool.get_if_fresh(&path)
                    .expect("concurrent get_if_fresh failed");
            }
        }));
    }
    for h in handles {
        h.join().expect("thread panicked");
    }
    // After all reads, entry count is unchanged (no writes).
    assert_eq!(pool.entry_count().unwrap(), 10);
}

// RFC 018 R4 regression guard: with exactly one slot, `checkout()`'s
// `try_lock` scan has only one slot to try, so every checkout beyond the
// first is guaranteed to take the blocking fallback path under real
// contention. None of them may error — contention must never be mistaken
// for poisoning.
#[test]
fn read_pool_single_slot_contention_without_poisoning_still_succeeds() {
    let dir = TempDir::new().unwrap();
    let db = make_populated_db(&dir);
    let pool: ReadPool<Vec<f32>> = ReadPool::open(
        CacheOptions {
            database_path: db,
            ..CacheOptions::default()
        },
        1,
    )
    .unwrap();

    let mut handles = Vec::new();
    for t in 0..8u32 {
        let pool = pool.clone();
        let path = dir.path().join(format!("f0{}.txt", t % 10));
        handles.push(thread::spawn(move || {
            for _ in 0..20 {
                pool.get_if_fresh(&path)
                    .expect("contended single-slot checkout must not error");
            }
        }));
    }
    for h in handles {
        h.join().expect("thread panicked");
    }
}

#[test]
fn read_pool_poisoned_slot_yields_poisoned_error() {
    let dir = TempDir::new().unwrap();
    let db = make_populated_db(&dir);
    let pool: ReadPool<Vec<f32>> = ReadPool::open(
        CacheOptions {
            database_path: db,
            ..CacheOptions::default()
        },
        1,
    )
    .unwrap();

    // Poison the pool's one slot: `query_run`'s closure holds the slot's
    // lock guard on the stack for the duration of the call, so a panic
    // inside it poisons the mutex when the guard drops during unwind.
    let poison_pool = pool.clone();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = poison_pool.query_run(|_q| panic!("intentional test panic"));
    }));

    let path = dir.path().join("f00.txt");
    let result = pool.get_if_fresh(&path);
    assert!(
        matches!(
            result,
            Err(LocalFileCacheError::Poisoned {
                resource: "ReadPool"
            })
        ),
        "expected Poisoned {{ resource: \"ReadPool\" }}, got {result:?}"
    );

    // Batch variants report Poisoned per element rather than panicking or
    // silently recovering.
    let batch = pool.batch_get_fresh(&[path]);
    assert_eq!(batch.len(), 1);
    assert!(matches!(
        batch[0],
        Err(LocalFileCacheError::Poisoned {
            resource: "ReadPool"
        })
    ));
}

#[test]
fn read_pool_concurrent_writer_and_readers() {
    // Writer + 4-slot ReadPool running simultaneously — no SQLITE_BUSY.
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("concurrent.sqlite3");

    // Populate via writer.
    {
        let writer: CacheEngine<Vec<f32>> = CacheEngine::builder().database(&db).build().unwrap();
        for i in 0..5u32 {
            let p = write_file(&dir, &format!("c{i}.txt"), b"data");
            writer.set(&p, &vec![i as f32]).unwrap();
        }
    }

    let pool: ReadPool<Vec<f32>> = ReadPool::open(
        CacheOptions {
            database_path: db.clone(),
            ..CacheOptions::default()
        },
        4,
    )
    .unwrap();

    // Concurrent reads while we re-open the writer and add entries.
    let pool_clone = pool.clone();
    let reader_handle = thread::spawn(move || {
        for _ in 0..100 {
            let _ = pool_clone.entry_count().unwrap();
        }
    });

    let writer: CacheEngine<Vec<f32>> = CacheEngine::builder().database(&db).build().unwrap();
    for i in 5..10u32 {
        let p = write_file(&dir, &format!("c{i}.txt"), b"data");
        writer.set(&p, &vec![i as f32]).unwrap();
    }

    reader_handle.join().expect("reader thread panicked");
}

// ---------------------------------------------------------------------------
// Size and construction errors
// ---------------------------------------------------------------------------

#[test]
fn read_pool_size_reports_correctly() {
    let dir = TempDir::new().unwrap();
    let db = make_populated_db(&dir);
    let pool: ReadPool<Vec<f32>> = ReadPool::open(
        CacheOptions {
            database_path: db,
            ..CacheOptions::default()
        },
        3,
    )
    .unwrap();
    assert_eq!(pool.size(), 3);
}

#[test]
fn read_pool_rejects_size_zero() {
    let dir = TempDir::new().unwrap();
    let db = make_populated_db(&dir);
    let result: Result<ReadPool<Vec<f32>>, _> = ReadPool::open(
        CacheOptions {
            database_path: db,
            ..CacheOptions::default()
        },
        0,
    );
    assert!(
        matches!(result, Err(LocalFileCacheError::UnsupportedFeature(_))),
        "size=0 must be rejected"
    );
}

#[test]
fn read_pool_rejects_memory_database() {
    let result: Result<ReadPool<Vec<f32>>, _> = ReadPool::open(
        CacheOptions {
            database_path: ":memory:".into(),
            ..CacheOptions::default()
        },
        2,
    );
    assert!(
        matches!(result, Err(LocalFileCacheError::UnsupportedFeature(_))),
        ":memory: must be rejected"
    );
}

// ---------------------------------------------------------------------------
// Builder integration
// ---------------------------------------------------------------------------

#[test]
fn build_read_pool_via_builder() {
    let dir = TempDir::new().unwrap();
    let db = make_populated_db(&dir);
    let pool: ReadPool<Vec<f32>> = CacheEngine::builder()
        .database(db)
        .build_read_pool(2)
        .unwrap();
    assert_eq!(pool.entry_count().unwrap(), 10);
}

// ---------------------------------------------------------------------------
// Shared-cache backend
// ---------------------------------------------------------------------------

#[test]
fn read_pool_shared_cache_reads_writer_data() {
    let dir = TempDir::new().unwrap();
    let db = make_populated_db(&dir);
    // shared_cache = true selects the RFC 004 backend.
    let pool: ReadPool<Vec<f32>> = ReadPool::open(
        CacheOptions {
            database_path: db,
            shared_cache: true,
            ..CacheOptions::default()
        },
        2,
    )
    .unwrap();
    assert_eq!(pool.entry_count().unwrap(), 10);
}
