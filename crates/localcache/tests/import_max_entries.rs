//! RFC 024 B3 / RFC 025 R6 — `import_entries` and `import_from` enforce
//! `max_entries` under the rule `batch_set` follows: the eviction runs in the
//! import's own transaction, it never evicts a row the call wrote, and an
//! import of more distinct entries than the bound is rejected before anything
//! is written.

mod common;
use common::write_file;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use localcache::{CacheEngine, ExportRecord, LocalFileCacheError};
use rusqlite::Connection;
use tempfile::TempDir;

type Evicted = Arc<Mutex<Vec<PathBuf>>>;

/// A writable file-backed engine bounded to `max_entries`, recording what it
/// evicts.
fn bounded_engine(
    directory: &TempDir,
    namespace: &str,
    max_entries: Option<usize>,
) -> (CacheEngine<Vec<f32>>, PathBuf, Evicted) {
    let database = directory.path().join("target.sqlite3");
    let evicted: Evicted = Arc::default();
    let sink = Arc::clone(&evicted);
    let mut builder = CacheEngine::<Vec<f32>>::builder()
        .database(&database)
        .namespace(namespace)
        .on_evict(move |path| sink.lock().unwrap().push(path.to_path_buf()));
    if let Some(max) = max_entries {
        builder = builder.max_entries(max);
    }
    (builder.build().unwrap(), database, evicted)
}

/// Store `count` real files as entries `e0..` and give them
/// `last_accessed_at` 100, 200, 300, ... so the eviction order is known.
fn store_aged_entries(
    directory: &TempDir,
    engine: &CacheEngine<Vec<f32>>,
    database: &Path,
    count: usize,
) -> Vec<PathBuf> {
    let mut stored = Vec::new();
    for i in 0..count {
        let file = write_file(directory, &format!("e{i}.txt"), b"x");
        engine.set(&file, &vec![i as f32]).unwrap();
        let canonical = std::fs::canonicalize(&file).unwrap();
        Connection::open(database)
            .unwrap()
            .execute(
                "UPDATE files SET last_accessed_at = ?1 WHERE path = ?2",
                rusqlite::params![100 * (i as i64 + 1), canonical.to_str().unwrap()],
            )
            .unwrap();
        stored.push(canonical);
    }
    stored
}

fn sorted_keys(engine: &CacheEngine<Vec<f32>>) -> Vec<PathBuf> {
    let mut keys = engine.keys(None).unwrap();
    keys.sort();
    keys
}

/// `count` records with distinct portable paths, each the oldest thing in the
/// database (`last_accessed_at = 1`), so only the protection keeps them.
fn records(count: usize) -> Vec<ExportRecord> {
    let directory = TempDir::new().unwrap();
    let source: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();
    let file = write_file(&directory, "template.txt", b"x");
    source.set(&file, &vec![7.0_f32]).unwrap();
    let template = source.export_entries().unwrap().remove(0);
    (0..count)
        .map(|i| ExportRecord {
            path: format!("/portable/r{i}.txt"),
            last_accessed_at: 1,
            ..template.clone()
        })
        .collect()
}

/// A source engine (its own in-memory database) holding `count` entries.
fn source_engine(directory: &TempDir, count: usize) -> CacheEngine<Vec<f32>> {
    let source: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();
    for i in 0..count {
        let file = write_file(directory, &format!("s{i}.txt"), b"x");
        source.set(&file, &vec![i as f32]).unwrap();
    }
    source
}

fn assert_rejected_records(error: LocalFileCacheError, distinct: usize, max: usize) {
    match &error {
        LocalFileCacheError::InvalidArgument { argument, .. } => {
            assert_eq!(*argument, "records", "{error}")
        }
        other => panic!("expected InvalidArgument {{ argument: \"records\" }}, got {other:?}"),
    }
    assert_eq!(
        error.to_string(),
        format!(
            "invalid argument: records: {distinct} distinct entries exceed max_entries ({max})"
        )
    );
}

// ---------------------------------------------------------------------------
// import_entries
// ---------------------------------------------------------------------------

#[test]
fn six_distinct_records_against_a_bound_of_three_are_rejected_before_any_write() {
    let directory = TempDir::new().unwrap();
    let (engine, _database, evicted) = bounded_engine(&directory, "default", Some(3));
    let stored = store_aged_entries(&directory, &engine, &_database, 2);
    let before = sorted_keys(&engine);

    let error = engine.import_entries(&records(6)).unwrap_err();

    assert_rejected_records(error, 6, 3);
    assert_eq!(sorted_keys(&engine), before, "no row written");
    assert_eq!(before.len(), stored.len());
    assert!(evicted.lock().unwrap().is_empty(), "nothing evicted");
}

#[test]
fn exactly_the_bound_is_accepted_into_an_empty_database() {
    let directory = TempDir::new().unwrap();
    let (engine, _database, evicted) = bounded_engine(&directory, "default", Some(3));

    let imported = engine.import_entries(&records(3)).unwrap();

    assert_eq!(imported, 3);
    assert_eq!(engine.entry_count().unwrap(), 3);
    assert!(evicted.lock().unwrap().is_empty());
}

#[test]
fn an_import_into_a_full_namespace_evicts_the_least_recently_used_older_rows() {
    let directory = TempDir::new().unwrap();
    let (engine, database, evicted) = bounded_engine(&directory, "default", Some(3));
    let stored = store_aged_entries(&directory, &engine, &database, 3);
    let incoming = records(2);

    let imported = engine.import_entries(&incoming).unwrap();

    assert_eq!(imported, 2);
    assert_eq!(engine.entry_count().unwrap(), 3, "ends at the bound");
    let keys = sorted_keys(&engine);
    for record in &incoming {
        assert!(
            keys.contains(&PathBuf::from(&record.path)),
            "the imported row {} survives although its last_accessed_at is the oldest",
            record.path
        );
    }
    assert!(
        keys.contains(&stored[2]),
        "the most recently used older row survives"
    );
    let mut evicted_paths = evicted.lock().unwrap().clone();
    evicted_paths.sort();
    let mut expected = vec![stored[0].clone(), stored[1].clone()];
    expected.sort();
    assert_eq!(
        evicted_paths, expected,
        "the callback gets exactly the two least recently used older rows"
    );
}

#[test]
fn eviction_touches_only_the_target_namespace() {
    let directory = TempDir::new().unwrap();
    let (engine, database, _evicted) = bounded_engine(&directory, "default", Some(3));
    store_aged_entries(&directory, &engine, &database, 3);
    let other: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(&database)
        .namespace("other")
        .build()
        .unwrap();
    let file = write_file(&directory, "other.txt", b"x");
    other.set(&file, &vec![9.0_f32]).unwrap();

    engine.import_entries(&records(2)).unwrap();

    assert_eq!(
        other.entry_count().unwrap(),
        1,
        "the other namespace is intact"
    );
}

#[test]
fn duplicate_records_count_once() {
    let directory = TempDir::new().unwrap();
    let (engine, _database, _evicted) = bounded_engine(&directory, "default", Some(3));
    let mut incoming = records(3);
    incoming.extend(records(3)); // the same three paths again: 6 records, 3 distinct

    let imported = engine.import_entries(&incoming).unwrap();

    assert_eq!(
        imported, 6,
        "the call still reports the records it imported"
    );
    assert_eq!(engine.entry_count().unwrap(), 3);
}

#[test]
fn without_max_entries_nothing_changes() {
    let directory = TempDir::new().unwrap();
    let (engine, database, evicted) = bounded_engine(&directory, "default", None);
    store_aged_entries(&directory, &engine, &database, 3);

    let imported = engine.import_entries(&records(6)).unwrap();

    assert_eq!(imported, 6);
    assert_eq!(engine.entry_count().unwrap(), 9, "no bound, no eviction");
    assert!(evicted.lock().unwrap().is_empty());
}

#[test]
fn an_empty_import_is_accepted_whatever_the_bound() {
    let directory = TempDir::new().unwrap();
    let (engine, _database, _evicted) = bounded_engine(&directory, "default", Some(1));
    assert_eq!(engine.import_entries(&[]).unwrap(), 0);
}

// ---------------------------------------------------------------------------
// import_from
// ---------------------------------------------------------------------------

#[test]
fn import_from_six_distinct_entries_against_a_bound_of_three_is_rejected_before_any_write() {
    let directory = TempDir::new().unwrap();
    let (engine, database, evicted) = bounded_engine(&directory, "default", Some(3));
    store_aged_entries(&directory, &engine, &database, 2);
    let before = sorted_keys(&engine);
    let source = source_engine(&directory, 6);

    let error = engine.import_from(&source).unwrap_err();

    assert_rejected_records(error, 6, 3);
    assert_eq!(sorted_keys(&engine), before, "no row written");
    assert!(evicted.lock().unwrap().is_empty());
}

#[test]
fn import_from_exactly_the_bound_is_accepted() {
    let directory = TempDir::new().unwrap();
    let (engine, _database, _evicted) = bounded_engine(&directory, "default", Some(3));
    let source = source_engine(&directory, 3);

    assert_eq!(engine.import_from(&source).unwrap(), 3);
    assert_eq!(engine.entry_count().unwrap(), 3);
}

#[test]
fn import_from_into_a_full_namespace_evicts_older_rows_and_keeps_what_it_wrote() {
    let directory = TempDir::new().unwrap();
    let (engine, database, evicted) = bounded_engine(&directory, "default", Some(3));
    let stored = store_aged_entries(&directory, &engine, &database, 3);
    let source = source_engine(&directory, 2);
    let incoming = sorted_keys(&source);

    assert_eq!(engine.import_from(&source).unwrap(), 2);

    assert_eq!(engine.entry_count().unwrap(), 3);
    let keys = sorted_keys(&engine);
    for key in &incoming {
        assert!(keys.contains(key), "{} survives", key.display());
    }
    assert!(keys.contains(&stored[2]));
    assert_eq!(evicted.lock().unwrap().len(), 2);
}

#[test]
fn import_from_without_max_entries_nothing_changes() {
    let directory = TempDir::new().unwrap();
    let (engine, database, _evicted) = bounded_engine(&directory, "default", None);
    store_aged_entries(&directory, &engine, &database, 3);
    let source = source_engine(&directory, 6);

    assert_eq!(engine.import_from(&source).unwrap(), 6);
    assert_eq!(engine.entry_count().unwrap(), 9);
}

// ---------------------------------------------------------------------------
// Wrappers
// ---------------------------------------------------------------------------

#[test]
fn the_sync_wrapper_follows_the_rule() {
    let directory = TempDir::new().unwrap();
    let shared = localcache::SyncCacheEngine::<Vec<f32>>::open(localcache::CacheOptions {
        database_path: directory.path().join("sync.sqlite3"),
        max_entries: Some(3),
        ..localcache::CacheOptions::default()
    })
    .unwrap();

    assert_rejected_records(shared.import_entries(&records(6)).unwrap_err(), 6, 3);
    assert_eq!(shared.import_entries(&records(3)).unwrap(), 3);
    assert_eq!(shared.entry_count().unwrap(), 3);
}
