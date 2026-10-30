//! RFC 025 R6 (the first three rows) and R8: the inputs that v0.21.4 and
//! v0.21.5 announced would be rejected from v0.22.0, and the boundaries that
//! stay accepted. Every rejection through every entry point, and before any
//! side effect, is in `open_rejects_before_side_effects.rs`; this file holds the
//! rest.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use localcache::{CacheEngine, CacheOptions, LocalFileCacheError};
use rusqlite::Connection;
use serde::{Serialize, Serializer};
use tempfile::TempDir;

use common::write_file;

fn open_with(directory: &TempDir, mutate: impl FnOnce(&mut CacheOptions)) -> CacheEngine<Vec<f32>> {
    let mut options = CacheOptions {
        database_path: directory.path().join("cache.sqlite3"),
        ..CacheOptions::default()
    };
    mutate(&mut options);
    CacheEngine::open(options).unwrap()
}

fn error_of<T>(result: Result<T, LocalFileCacheError>) -> LocalFileCacheError {
    match result {
        Ok(_) => panic!("the call was accepted"),
        Err(error) => error,
    }
}

// ---------------------------------------------------------------------------
// Boundaries that stay accepted (guards)
// ---------------------------------------------------------------------------

#[test]
fn a_ttl_of_exactly_one_second_is_accepted() {
    let directory = TempDir::new().unwrap();
    open_with(&directory, |options| {
        options.ttl = Some(Duration::from_secs(1))
    });
}

#[test]
fn max_entries_of_one_is_accepted() {
    let directory = TempDir::new().unwrap();
    open_with(&directory, |options| options.max_entries = Some(1));
}

/// Move every entry's `updated_at` to `seconds` relative to the database's own
/// clock (negative: in the past; a future value counts as age zero).
fn set_updated_at_relative_to_now(database: &std::path::Path, seconds: i64) {
    Connection::open(database)
        .unwrap()
        .execute(
            "UPDATE files SET updated_at = CAST(strftime('%s','now') AS INTEGER) + ?1",
            [seconds],
        )
        .unwrap();
}

/// R8: a fractional TTL of one second or more is truncated to whole seconds.
/// `1500 ms` behaves as `1 s`: an entry one second old is expired, an entry of
/// age zero is not. The ages are set through the database, not by sleeping.
#[test]
fn a_ttl_of_1500_ms_is_accepted_and_behaves_as_one_second() {
    let directory = TempDir::new().unwrap();
    let engine = open_with(&directory, |options| {
        options.ttl = Some(Duration::from_millis(1500));
    });
    let source = write_file(&directory, "s.bin", b"x");
    engine.set(&source, &vec![1.0]).unwrap();
    let database = directory.path().join("cache.sqlite3");

    // Written "in the future" counts as age zero: fresh.
    set_updated_at_relative_to_now(&database, 100);
    assert!(engine.get_if_fresh(&source).unwrap().is_some());

    // One second old: expired. With 1500 ms taken literally it would still be
    // fresh; truncated to 1 s it is expired. (Two seconds old would be expired
    // either way, so this is the case that pins the truncation.)
    set_updated_at_relative_to_now(&database, -1);
    assert!(engine.get_if_fresh(&source).unwrap().is_none());
}

// ---------------------------------------------------------------------------
// Read-only engines are not exempt from configuration validation
// ---------------------------------------------------------------------------

#[test]
fn a_read_only_engine_with_an_invalid_option_is_rejected_too() {
    let directory = TempDir::new().unwrap();
    open_with(&directory, |_| {}); // an existing, current database
    for (mutate, option) in [
        (
            (|options: &mut CacheOptions| options.max_entries = Some(0)) as fn(&mut CacheOptions),
            "max_entries",
        ),
        (
            |options: &mut CacheOptions| options.ttl = Some(Duration::from_millis(1)),
            "ttl",
        ),
    ] {
        let mut options = CacheOptions {
            database_path: directory.path().join("cache.sqlite3"),
            read_only: true,
            ..CacheOptions::default()
        };
        mutate(&mut options);
        let error = error_of(CacheEngine::<Vec<f32>>::open(options));
        assert!(
            matches!(&error, LocalFileCacheError::InvalidConfiguration { option: found, .. } if *found == option),
            "{error:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// batch_set and max_entries
// ---------------------------------------------------------------------------

fn engine_with_bound(directory: &TempDir, bound: usize) -> CacheEngine<Vec<f32>> {
    open_with(directory, |options| options.max_entries = Some(bound))
}

fn files(directory: &TempDir, count: usize) -> Vec<(std::path::PathBuf, Vec<f32>)> {
    (0..count)
        .map(|i| {
            (
                write_file(directory, &format!("f{i}.txt"), b"x"),
                vec![i as f32],
            )
        })
        .collect()
}

#[test]
fn a_batch_of_more_distinct_paths_than_max_entries_is_rejected_and_changes_nothing() {
    let directory = TempDir::new().unwrap();
    let engine = engine_with_bound(&directory, 3);
    // Two entries are already stored; the bound is 3.
    let stored = files(&directory, 2);
    engine.batch_set(&stored).unwrap();
    assert_eq!(engine.entry_count().unwrap(), 2);

    let batch: Vec<_> = (10..14)
        .map(|i| {
            (
                write_file(&directory, &format!("g{i}.txt"), b"x"),
                vec![i as f32],
            )
        })
        .collect();
    let error = error_of(engine.batch_set(&batch));
    match &error {
        LocalFileCacheError::InvalidArgument { argument, reason } => {
            assert_eq!(*argument, "items");
            assert_eq!(reason, "4 distinct paths exceed max_entries (3)");
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
    assert_eq!(
        error.to_string(),
        "invalid argument: items: 4 distinct paths exceed max_entries (3)"
    );

    // Nothing was written and nothing was evicted.
    assert_eq!(engine.entry_count().unwrap(), 2);
    for (path, _) in &stored {
        assert!(engine.get(path).unwrap().is_some(), "{path:?} was evicted");
    }
    for (path, _) in &batch {
        assert!(engine.get(path).unwrap().is_none(), "{path:?} was written");
    }
}

#[test]
fn a_batch_of_exactly_max_entries_distinct_paths_is_accepted() {
    let directory = TempDir::new().unwrap();
    let engine = engine_with_bound(&directory, 3);
    let report = engine.batch_set(&files(&directory, 3)).unwrap();
    assert_eq!(report.succeeded, 3);
    assert!(report.failed.is_empty());
    assert_eq!(engine.entry_count().unwrap(), 3);
}

/// "Distinct" is the canonical key: duplicates, and one file spelled two ways,
/// count once. Five items, three keys.
#[test]
fn duplicates_and_alternate_spellings_count_once() {
    let directory = TempDir::new().unwrap();
    let engine = engine_with_bound(&directory, 3);
    let a = write_file(&directory, "a.txt", b"x");
    let b = write_file(&directory, "b.txt", b"x");
    let c = write_file(&directory, "c.txt", b"x");
    // `dir/../dir/a.txt` canonicalizes to the same key as `dir/a.txt`.
    let a_again = directory
        .path()
        .join("..")
        .join(directory.path().file_name().unwrap())
        .join("a.txt");
    let items = vec![
        (a.clone(), vec![1.0]),
        (b, vec![2.0]),
        (c, vec![3.0]),
        (a.clone(), vec![4.0]),
        (a_again, vec![5.0]),
    ];
    let report = engine.batch_set(&items).unwrap();
    assert_eq!(report.failed.len(), 0, "{:?}", report.failed);
    assert_eq!(engine.entry_count().unwrap(), 3);
}

/// A missing file occupies no row and is not counted: four items, one missing,
/// bound 3. The call is accepted and reports that item as a failure.
#[test]
fn a_missing_file_is_not_counted_and_stays_a_per_item_failure() {
    let directory = TempDir::new().unwrap();
    let engine = engine_with_bound(&directory, 3);
    let mut items = files(&directory, 3);
    let missing = directory.path().join("never-created.txt");
    items.insert(1, (missing.clone(), vec![9.0]));

    let report = engine.batch_set(&items).unwrap();
    assert_eq!(report.succeeded, 3);
    assert_eq!(report.failed.len(), 1);
    assert_eq!(report.failed[0].0, missing);
    assert!(matches!(
        report.failed[0].1,
        LocalFileCacheError::FileNotFound { .. } | LocalFileCacheError::Io(_)
    ));
    assert_eq!(engine.entry_count().unwrap(), 3);
}

static SERIALIZED: AtomicUsize = AtomicUsize::new(0);

/// A payload whose `Serialize` counts its calls.
struct Counting;

impl Serialize for Counting {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        SERIALIZED.fetch_add(1, Ordering::SeqCst);
        serializer.serialize_u8(0)
    }
}

impl<'de> serde::Deserialize<'de> for Counting {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        u8::deserialize(deserializer).map(|_| Counting)
    }
}

/// The rejection comes before any payload is encoded.
#[test]
fn the_rejection_happens_before_any_payload_is_encoded() {
    let directory = TempDir::new().unwrap();
    let engine: CacheEngine<Counting> = CacheEngine::builder()
        .database(directory.path().join("c.sqlite3"))
        .max_entries(2)
        .build()
        .unwrap();
    let items: Vec<_> = (0..3)
        .map(|i| (write_file(&directory, &format!("c{i}.txt"), b"x"), Counting))
        .collect();

    SERIALIZED.store(0, Ordering::SeqCst);
    let error = error_of(engine.batch_set(&items));
    assert!(matches!(
        error,
        LocalFileCacheError::InvalidArgument {
            argument: "items",
            ..
        }
    ));
    assert_eq!(
        SERIALIZED.load(Ordering::SeqCst),
        0,
        "a payload was encoded"
    );

    // The same batch within the bound does encode them (the guard).
    let report = engine.batch_set(&items[..2]).unwrap();
    assert_eq!(report.succeeded, 2);
    assert_eq!(SERIALIZED.load(Ordering::SeqCst), 2);
}

/// The rejection comes before the `IMMEDIATE` transaction starts: with another
/// connection holding the write lock, a rejected batch still fails at once with
/// `InvalidArgument`, where a batch that reached the transaction would wait out
/// the busy timeout (about five seconds) and fail with a database error.
#[test]
fn the_rejection_happens_before_the_write_transaction_starts() {
    let directory = TempDir::new().unwrap();
    let engine = engine_with_bound(&directory, 1);
    let items = files(&directory, 2);

    let blocker = Connection::open(directory.path().join("cache.sqlite3")).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE;").unwrap();

    let started = Instant::now();
    let error = error_of(engine.batch_set(&items));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    assert!(matches!(
        error,
        LocalFileCacheError::InvalidArgument {
            argument: "items",
            ..
        }
    ));
    blocker.execute_batch("ROLLBACK;").unwrap();
}
