//! RFC 026: eviction follows the order of *uses* (reads, writes, overwrites,
//! `touch`), strictly; an import is a restore, not a use; an export keeps the
//! relative recency. Through the public API and the real clock; the cases that
//! need a clock the test controls are unit tests in the crate.

mod common;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use common::write_file;
use localcache::{CacheEngine, ExportRecord, LocalFileCacheError, SyncCacheEngine};
use rusqlite::Connection;
use tempfile::TempDir;

fn names(keys: Vec<PathBuf>) -> String {
    let mut names: Vec<String> = keys
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names.join(",")
}

/// RFC 026's observed pathology. Before v0.23.0 the survivors were `a,g,h`:
/// `a` was read once and so outranked every entry that was only written.
#[test]
fn an_entry_read_once_is_evicted_in_its_turn() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(":memory:")
        .max_entries(3)
        .build()
        .unwrap();
    let files: Vec<PathBuf> = ["a", "b", "c", "d", "e", "f", "g", "h"]
        .iter()
        .map(|name| write_file(&dir, name, b"x"))
        .collect();
    for file in &files[..3] {
        engine.set(file, &vec![1.0]).unwrap();
    }
    engine.get(&files[0]).unwrap();
    for file in &files[3..] {
        engine.set(file, &vec![1.0]).unwrap();
    }
    assert_eq!(names(engine.keys(None).unwrap()), "f,g,h");
}

/// Overwriting an entry makes it the newest. Within one clock second the old
/// rule (`updated_at`, then `id`) evicted the overwritten entry instead.
#[test]
fn an_overwrite_makes_the_entry_the_newest() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(":memory:")
        .max_entries(2)
        .build()
        .unwrap();
    let [a, b, c] = ["a", "b", "c"].map(|name| write_file(&dir, name, b"x"));
    engine.set(&a, &vec![1.0]).unwrap();
    engine.set(&b, &vec![1.0]).unwrap();
    engine.set(&a, &vec![2.0]).unwrap();
    engine.set(&c, &vec![1.0]).unwrap();
    assert_eq!(names(engine.keys(None).unwrap()), "a,c");
}

/// Four engines (each with its own connection pool) on one file, eight threads
/// reading, writing, overwriting and touching for a few seconds. Afterwards no
/// two entries of a namespace share a use value: SQLite serialises the writers
/// and the statement that assigns the value carries its own `MAX + 1`.
#[test]
fn concurrent_engines_never_assign_the_same_use_value() {
    let dir = TempDir::new().unwrap();
    let database = dir.path().join("concurrent.sqlite3");
    let files: Arc<Vec<PathBuf>> = Arc::new(
        (0..60)
            .map(|i| write_file(&dir, &format!("f{i}.txt"), b"x"))
            .collect(),
    );
    let engines: Vec<SyncCacheEngine<Vec<f32>>> = (0..4)
        .map(|i| {
            SyncCacheEngine::open(localcache::CacheOptions {
                database_path: database.clone(),
                namespace: if i % 2 == 0 { "even" } else { "odd" }.to_owned(),
                ..localcache::CacheOptions::default()
            })
            .unwrap()
        })
        .collect();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
    let surfaced = Arc::new(Mutex::new(Vec::<String>::new()));
    let mut threads = Vec::new();
    for t in 0..8_u64 {
        let engine = engines[(t % 4) as usize].clone();
        let files = Arc::clone(&files);
        let surfaced = Arc::clone(&surfaced);
        threads.push(std::thread::spawn(move || {
            let mut state = 0x9e37_79b9_7f4a_7c15_u64 ^ (t + 1).wrapping_mul(0x2545_f491_4f6c_dd1d);
            let mut next = move || {
                state ^= state >> 12;
                state ^= state << 25;
                state ^= state >> 27;
                state.wrapping_mul(0x2545_f491_4f6c_dd1d)
            };
            while std::time::Instant::now() < deadline {
                let file = &files[(next() % files.len() as u64) as usize];
                let result = match next() % 10 {
                    0..=2 => engine.set(file, &vec![1.0]),
                    3..=4 => engine.touch(file).map(|_| ()),
                    _ => engine.get(file).map(|_| ()),
                };
                match result {
                    Ok(()) => {}
                    // A writer that waited out the busy timeout: not a defect of
                    // the order, and not new.
                    Err(LocalFileCacheError::Database(error))
                        if error.sqlite_extended_code().map(|code| code & 0xff) == Some(5) => {}
                    Err(other) => surfaced.lock().unwrap().push(other.to_string()),
                }
            }
        }));
    }
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(*surfaced.lock().unwrap(), Vec::<String>::new());

    let conn = Connection::open(&database).unwrap();
    let rows: i64 = conn
        .query_row("SELECT count(*) FROM files", [], |row| row.get(0))
        .unwrap();
    assert!(rows > 0);
    let duplicates: i64 = conn
        .query_row(
            "SELECT count(*) FROM (SELECT namespace, last_used_us FROM files
                                   GROUP BY namespace, last_used_us HAVING count(*) > 1)",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        duplicates, 0,
        "two entries of a namespace share a use value"
    );
    let not_integer: i64 = conn
        .query_row(
            "SELECT count(*) FROM files WHERE typeof(last_used_us) <> 'integer'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(not_integer, 0);
}

// ---------------------------------------------------------------------------
// Export and import (RFC 026 R2, F2, F3)
// ---------------------------------------------------------------------------

fn template_record() -> ExportRecord {
    let dir = TempDir::new().unwrap();
    let source: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();
    let file = write_file(&dir, "template.txt", b"x");
    source.set(&file, &vec![7.0]).unwrap();
    source.export_entries().unwrap().remove(0)
}

fn record(path: &str, last_accessed_at: i64, updated_at: i64) -> ExportRecord {
    ExportRecord {
        path: path.to_owned(),
        last_accessed_at,
        updated_at,
        ..template_record()
    }
}

/// F3: the export is ordered by use, oldest first, whatever the write times.
#[test]
fn an_export_lists_the_entries_oldest_use_first() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();
    let files: Vec<PathBuf> = ["a", "b", "c", "d"]
        .iter()
        .map(|name| write_file(&dir, name, b"x"))
        .collect();
    for file in &files {
        engine.set(file, &vec![1.0]).unwrap();
    }
    // Uses, in order: b, d, a (c was only written).
    engine.get(&files[1]).unwrap();
    engine.touch(&files[3]).unwrap();
    engine.get(&files[0]).unwrap();
    let order: Vec<String> = engine
        .export_entries()
        .unwrap()
        .iter()
        .map(|record| record.path.rsplit('/').next().unwrap().to_owned())
        .collect();
    assert_eq!(order, ["c", "b", "d", "a"]);
}

/// R2 and F3 together: entries used within the same second (the public field
/// cannot tell them apart) keep their relative recency through an export and an
/// import into a fresh database, because the export is in use order and the
/// import ties by arrival.
#[test]
fn relative_recency_survives_a_round_trip_within_one_second() {
    let dir = TempDir::new().unwrap();
    let source: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();
    let names = ["a", "b", "c", "d", "e", "f"];
    let files: Vec<PathBuf> = names
        .iter()
        .map(|name| write_file(&dir, name, b"x"))
        .collect();
    for file in &files {
        source.set(file, &vec![1.0]).unwrap();
    }
    // The use order of the source: c, a, e, b, f, d.
    for index in [2, 0, 4, 1, 5, 3] {
        source.get(&files[index]).unwrap();
    }
    let records = source.export_entries().unwrap();
    let seconds: HashSet<i64> = records
        .iter()
        .map(|record| record.last_accessed_at)
        .collect();

    let evicted = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = Arc::clone(&evicted);
    let target: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(":memory:")
        .max_entries(6)
        .on_evict(move |path| {
            sink.lock()
                .unwrap()
                .push(path.file_name().unwrap().to_string_lossy().into_owned());
        })
        .build()
        .unwrap();
    target.import_entries(&records).unwrap();
    // Each new write evicts the oldest use of the target.
    let extra: Vec<PathBuf> = (0..6)
        .map(|i| write_file(&dir, &format!("x{i}"), b"x"))
        .collect();
    for file in &extra {
        target.set(file, &vec![2.0]).unwrap();
    }
    assert_eq!(
        *evicted.lock().unwrap(),
        ["c", "a", "e", "b", "f", "d"],
        "the evictions must follow the source's use order (the records shared {} second(s))",
        seconds.len()
    );
}

/// An import is a restore, not a use: an imported entry does not become the
/// newest, and every later use outranks every imported entry.
#[test]
fn an_import_does_not_make_entries_newer_than_a_later_use() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(":memory:")
        .max_entries(3)
        .build()
        .unwrap();
    let a = write_file(&dir, "a", b"x");
    engine.set(&a, &vec![1.0]).unwrap();
    // Two records with a recency far in the past, and one in the future of 1970.
    engine
        .import_entries(&[
            record("/portable/old1", 5, 5),
            record("/portable/old2", 6, 6),
        ])
        .unwrap();
    // `a` was written before the import but is far newer than the records.
    let b = write_file(&dir, "b", b"x");
    engine.set(&b, &vec![1.0]).unwrap();
    let kept = names(engine.keys(None).unwrap());
    assert_eq!(kept, "a,b,old2");
}

/// R2: a record that was never read (`last_accessed_at == 0`, from before
/// v0.23.0) is placed by the time it was written.
#[test]
fn a_record_that_was_never_read_is_placed_by_its_write_time() {
    let evicted = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = Arc::clone(&evicted);
    let engine: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(":memory:")
        .max_entries(3)
        .on_evict(move |path| sink.lock().unwrap().push(path.display().to_string()))
        .build()
        .unwrap();
    engine
        .import_entries(&[
            record("/portable/written-late", 0, 9_000),
            record("/portable/written-early", 0, 1_000),
            record("/portable/read-between", 5_000, 100),
        ])
        .unwrap();
    let dir2 = TempDir::new().unwrap();
    for name in ["n1", "n2", "n3"] {
        engine
            .set(write_file(&dir2, name, b"x"), &vec![1.0])
            .unwrap();
    }
    assert_eq!(
        *evicted.lock().unwrap(),
        [
            "/portable/written-early",
            "/portable/read-between",
            "/portable/written-late"
        ]
    );
}

/// F2: a record whose recency the use order cannot hold fails the whole import,
/// before anything is written.
#[test]
fn a_record_out_of_range_fails_the_whole_import_before_writing() {
    let largest_ok = (1_i64 << 62) / 1_000_000;
    let bad_records = [
        record("/portable/bad", i64::MAX, 1),
        record("/portable/bad", largest_ok + 1, 1),
        record("/portable/bad", 0, -1),
        record("/portable/bad", 0, i64::MIN),
    ];
    for bad in bad_records {
        let engine: CacheEngine<Vec<f32>> =
            CacheEngine::builder().database(":memory:").build().unwrap();
        let error = engine
            .import_entries(&[record("/portable/good", 10, 10), bad.clone()])
            .unwrap_err();
        match &error {
            LocalFileCacheError::InvalidArgument { argument, .. } => {
                assert_eq!(*argument, "records")
            }
            other => panic!("expected InvalidArgument {{ argument: \"records\" }}, got {other:?}"),
        }
        assert_eq!(
            engine.entry_count().unwrap(),
            0,
            "nothing may be written: {bad:?}"
        );
    }
    // The largest representable recency is accepted.
    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();
    engine
        .import_entries(&[record("/portable/edge", largest_ok, 1)])
        .unwrap();
    assert_eq!(
        engine.list_entries().unwrap()[0].last_accessed_at,
        largest_ok
    );
}
