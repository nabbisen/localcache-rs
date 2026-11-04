//! RFC 026: the use order, through the engine, with the real clock. The cases
//! that depend on the clock itself — one that never moves, one that runs
//! backwards, uses inside one microsecond — are tests of the `Store`, where the
//! use order lives (`db/store/tests.rs`).

use std::path::PathBuf;

use tempfile::TempDir;

use crate::CacheEngine;

fn write_file(dir: &TempDir, name: &str) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, b"x").unwrap();
    path
}

fn engine(max_entries: usize) -> CacheEngine<Vec<u8>> {
    CacheEngine::builder()
        .database(":memory:")
        .max_entries(max_entries)
        .build()
        .unwrap()
}

fn survivors(engine: &CacheEngine<Vec<u8>>) -> String {
    let mut names: Vec<String> = engine
        .keys(None)
        .unwrap()
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names.join(",")
}

/// The pathology of RFC 026: before v0.23.0 an entry read once, ever, outlived
/// every entry that was only written, so the survivors were `a`, `g`, `h`.
#[test]
fn an_entry_read_once_is_not_immortal() {
    {
        let dir = TempDir::new().unwrap();
        let engine = engine(3);
        let files: Vec<PathBuf> = ["a", "b", "c", "d", "e", "f", "g", "h"]
            .iter()
            .map(|name| write_file(&dir, name))
            .collect();
        for file in &files[..3] {
            engine.set(file, &vec![1]).unwrap();
        }
        engine.get(&files[0]).unwrap();
        for file in &files[3..] {
            engine.set(file, &vec![1]).unwrap();
        }
        assert_eq!(survivors(&engine), "f,g,h");
    }
}

#[test]
fn touch_protects_from_eviction() {
    let dir = TempDir::new().unwrap();
    let engine = engine(3);
    let [a, b, c, d] = ["a", "b", "c", "d"].map(|name| write_file(&dir, name));
    for file in [&a, &b, &c] {
        engine.set(file, &vec![1]).unwrap();
    }
    assert!(engine.touch(&a).unwrap());
    engine.set(&d, &vec![1]).unwrap();
    // `b` is the oldest use; `a`, touched after it was written, is protected.
    assert_eq!(survivors(&engine), "a,c,d");
}

#[test]
fn a_read_hit_protects_from_eviction() {
    let dir = TempDir::new().unwrap();
    let engine = engine(2);
    let [a, b, c] = ["a", "b", "c"].map(|name| write_file(&dir, name));
    engine.set(&a, &vec![1]).unwrap();
    engine.set(&b, &vec![1]).unwrap();
    engine.get(&a).unwrap().unwrap();
    engine.set(&c, &vec![1]).unwrap();
    assert_eq!(survivors(&engine), "a,c");
}

#[test]
fn an_overwrite_is_a_use() {
    let dir = TempDir::new().unwrap();
    let engine = engine(2);
    let [a, b, c] = ["a", "b", "c"].map(|name| write_file(&dir, name));
    engine.set(&a, &vec![1]).unwrap();
    engine.set(&b, &vec![1]).unwrap();
    engine.set(&a, &vec![2]).unwrap(); // `a` is now newer than `b`
    engine.set(&c, &vec![1]).unwrap();
    assert_eq!(survivors(&engine), "a,c");
}

#[test]
fn a_batch_is_a_run_of_uses_and_never_evicts_its_own_rows() {
    let dir = TempDir::new().unwrap();
    let engine = engine(3);
    let [a, b, c, d] = ["a", "b", "c", "d"].map(|name| write_file(&dir, name));
    engine.set(&a, &vec![1]).unwrap();
    engine.set(&b, &vec![1]).unwrap();
    engine
        .batch_set(&[(c.clone(), vec![1]), (d.clone(), vec![1])])
        .unwrap();
    assert_eq!(survivors(&engine), "b,c,d");
}

#[test]
fn a_read_only_engine_records_no_use() {
    let dir = TempDir::new().unwrap();
    let database = dir.path().join("readonly.sqlite3");
    let file = write_file(&dir, "a");
    {
        let writer: CacheEngine<Vec<u8>> =
            CacheEngine::builder().database(&database).build().unwrap();
        writer.set(&file, &vec![1]).unwrap();
    }
    let stored = |database: &PathBuf| -> i64 {
        rusqlite::Connection::open(database)
            .unwrap()
            .query_row("SELECT last_used_us FROM files", [], |row| row.get(0))
            .unwrap()
    };
    let before = stored(&database);
    let reader: CacheEngine<Vec<u8>> = CacheEngine::builder()
        .database(&database)
        .read_only()
        .build()
        .unwrap();
    assert!(reader.get(&file).unwrap().is_some());
    assert!(reader.get_if_fresh(&file).unwrap().is_some());
    assert!(
        reader.touch(&file).is_err(),
        "a read-only engine refuses touch"
    );
    drop(reader);
    assert_eq!(stored(&database), before);
}

#[test]
fn a_read_that_cannot_record_its_use_still_succeeds_but_touch_reports_it() {
    let dir = TempDir::new().unwrap();
    let database = dir.path().join("exhausted.sqlite3");
    let file = write_file(&dir, "a");
    let engine: CacheEngine<Vec<u8>> = CacheEngine::builder().database(&database).build().unwrap();
    engine.set(&file, &vec![1]).unwrap();
    // A damaged file: another entry sits at the very end of the order.
    rusqlite::Connection::open(&database)
        .unwrap()
        .execute_batch(
            "INSERT INTO files (namespace, path, mtime, file_size, updated_at, last_used_us)
             VALUES ('default', '/elsewhere', 0, 0, 0, 4611686018427387903);
             INSERT INTO payloads (file_id, content) SELECT id, X'00' FROM files WHERE path = '/elsewhere';",
        )
        .unwrap();
    // The read succeeds although its bookkeeping cannot (RFC 026 F4)…
    assert!(engine.get(&file).unwrap().is_some());
    // …`touch` and `set` say so.
    let touched = engine.touch(&file).unwrap_err();
    assert!(
        matches!(&touched, crate::LocalFileCacheError::DatabaseState(m) if m.contains("use order exhausted")),
        "{touched:?}"
    );
    let written = engine.set(&file, &vec![2]).unwrap_err();
    assert!(
        matches!(&written, crate::LocalFileCacheError::DatabaseState(m) if m.contains("use order exhausted")),
        "{written:?}"
    );
    // Nothing was stored by the failed write.
    assert_eq!(engine.get(&file).unwrap().unwrap().payload, vec![1]);
}

#[test]
fn a_read_pool_slot_records_no_use() {
    let dir = TempDir::new().unwrap();
    let database = dir.path().join("pool.sqlite3");
    let file = write_file(&dir, "a");
    {
        let writer: CacheEngine<Vec<u8>> =
            CacheEngine::builder().database(&database).build().unwrap();
        writer.set(&file, &vec![1]).unwrap();
    }
    let stored = || -> i64 {
        rusqlite::Connection::open(&database)
            .unwrap()
            .query_row("SELECT last_used_us FROM files", [], |row| row.get(0))
            .unwrap()
    };
    let before = stored();
    let pool = CacheEngine::<Vec<u8>>::builder()
        .database(&database)
        .build_read_pool(2)
        .unwrap();
    for _ in 0..4 {
        assert!(pool.get(&file).unwrap().is_some());
    }
    drop(pool);
    assert_eq!(stored(), before);
}
