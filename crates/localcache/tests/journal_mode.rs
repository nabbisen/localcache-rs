//! RFC 024 B2 — an engine that does not choose a journal mode leaves an
//! existing database's journal mode alone, and gives a new database WAL.
//!
//! Only `WAL` is stored in the database file. `DELETE`, `TRUNCATE`,
//! `PERSIST`, `MEMORY` and `OFF` belong to a connection, and a new connection
//! to a file in any of them reports `delete`. So "the database's journal
//! mode" is observed here with a fresh connection, which sees `wal` or
//! `delete` and nothing else.

mod common;
use common::write_file;

use std::fs;
use std::path::{Path, PathBuf};

use localcache::{CacheEngine, CacheOptions, JournalMode, ReadPool};
use rusqlite::Connection;
use tempfile::TempDir;

fn journal_mode(database: &Path) -> String {
    Connection::open(database)
        .unwrap()
        .query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
        .unwrap()
}

fn user_version(database: &Path) -> i64 {
    Connection::open(database)
        .unwrap()
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap()
}

/// A database with one entry, created with an explicit `mode` and closed.
fn database_in(dir: &TempDir, name: &str, mode: JournalMode) -> PathBuf {
    let database = dir.path().join(name);
    let engine: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(&database)
        .journal_mode(mode)
        .build()
        .unwrap();
    let file = write_file(dir, &format!("{name}.seed.txt"), b"seed");
    engine.set(&file, &vec![1.0_f32]).unwrap();
    drop(engine);
    database
}

/// Open with the defaults (no journal mode chosen), write, and close.
fn default_open_write_close(dir: &TempDir, database: &Path, name: &str) {
    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: database.to_path_buf(),
        ..CacheOptions::default()
    })
    .unwrap();
    let file = write_file(dir, name, b"later");
    engine.set(&file, &vec![2.0_f32]).unwrap();
    drop(engine);
}

#[test]
fn the_default_journal_mode_is_preserve() {
    assert_eq!(CacheOptions::default().journal_mode, JournalMode::Preserve);
    assert_eq!(JournalMode::default(), JournalMode::Preserve);
    let dir = TempDir::new().unwrap();
    let built: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(dir.path().join("built.sqlite3"))
        .build()
        .unwrap();
    drop(built);
    assert_eq!(journal_mode(&dir.path().join("built.sqlite3")), "wal");
}

#[test]
fn a_delete_database_stays_delete_after_a_default_open_a_write_and_a_close() {
    let dir = TempDir::new().unwrap();
    let database = database_in(&dir, "delete.sqlite3", JournalMode::Delete);
    assert_eq!(journal_mode(&database), "delete", "sanity");

    default_open_write_close(&dir, &database, "more.txt");

    assert_eq!(journal_mode(&database), "delete");
    assert!(
        !dir.path().join("delete.sqlite3-wal").exists(),
        "no -wal file appears"
    );
}

#[test]
fn a_wal_database_stays_wal_after_a_default_open_a_write_and_a_close() {
    let dir = TempDir::new().unwrap();
    let database = database_in(&dir, "wal.sqlite3", JournalMode::Wal);
    assert_eq!(journal_mode(&database), "wal", "sanity");

    default_open_write_close(&dir, &database, "more.txt");

    assert_eq!(journal_mode(&database), "wal");
}

#[test]
fn a_new_database_becomes_wal() {
    let dir = TempDir::new().unwrap();
    let database = dir.path().join("new.sqlite3");
    assert!(!database.exists());

    default_open_write_close(&dir, &database, "first.txt");

    assert_eq!(journal_mode(&database), "wal");
}

#[test]
fn an_empty_existing_file_counts_as_new_and_becomes_wal() {
    let dir = TempDir::new().unwrap();
    let database = dir.path().join("empty.sqlite3");
    fs::write(&database, b"").unwrap();

    default_open_write_close(&dir, &database, "first.txt");

    assert_eq!(journal_mode(&database), "wal");
}

/// An existing database with an older schema counts as existing: it is
/// migrated and keeps its journal mode, in both modes.
#[test]
fn a_database_with_an_older_schema_keeps_its_journal_mode_through_the_migration() {
    for (wal, expected) in [(false, "delete"), (true, "wal")] {
        let dir = TempDir::new().unwrap();
        let database = dir.path().join("old.sqlite3");
        fs::copy(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/compat-v0_1.sqlite3"
            ),
            &database,
        )
        .unwrap();
        if wal {
            let conn = Connection::open(&database).unwrap();
            let mode: String = conn
                .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
                .unwrap();
            assert_eq!(mode, "wal");
        }
        assert_eq!(journal_mode(&database), expected, "sanity");
        let before = user_version(&database);

        default_open_write_close(&dir, &database, "after-migration.txt");

        assert!(user_version(&database) > before, "the schema was migrated");
        assert_eq!(journal_mode(&database), expected);
    }
}

/// Guards: an explicit choice is applied as before.
#[test]
fn an_explicit_wal_converts_a_delete_database() {
    let dir = TempDir::new().unwrap();
    let database = database_in(&dir, "to-wal.sqlite3", JournalMode::Delete);
    assert_eq!(journal_mode(&database), "delete", "sanity");

    let engine: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(&database)
        .journal_mode(JournalMode::Wal)
        .build()
        .unwrap();
    drop(engine);

    assert_eq!(journal_mode(&database), "wal");
}

#[test]
fn an_explicit_delete_converts_a_wal_database() {
    let dir = TempDir::new().unwrap();
    let database = database_in(&dir, "to-delete.sqlite3", JournalMode::Wal);
    assert_eq!(journal_mode(&database), "wal", "sanity");

    let engine: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(&database)
        .journal_mode(JournalMode::Delete)
        .build()
        .unwrap();
    drop(engine);

    assert_eq!(journal_mode(&database), "delete");
}

/// Guards: read-only opens and the pool never change a journal mode.
#[test]
fn read_only_opens_and_the_pool_never_change_a_journal_mode() {
    for (mode, expected) in [(JournalMode::Delete, "delete"), (JournalMode::Wal, "wal")] {
        let dir = TempDir::new().unwrap();
        let database = database_in(&dir, "ro.sqlite3", mode);

        let engine: CacheEngine<Vec<f32>> = CacheEngine::builder()
            .database(&database)
            .read_only()
            .build()
            .unwrap();
        drop(engine);
        assert_eq!(journal_mode(&database), expected, "read-only engine");

        let pool: ReadPool<Vec<f32>> = ReadPool::open(
            CacheOptions {
                database_path: database.clone(),
                ..CacheOptions::default()
            },
            2,
        )
        .unwrap();
        drop(pool);
        assert_eq!(journal_mode(&database), expected, "read pool");
    }
}

/// The watcher helper inherits the engine's mode, `Preserve` included, so it
/// cannot convert a database either.
#[cfg(feature = "watching")]
#[test]
fn a_watcher_does_not_convert_a_delete_database_opened_with_the_defaults() {
    let dir = TempDir::new().unwrap();
    let database = database_in(&dir, "watch.sqlite3", JournalMode::Delete);
    let engine: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: database.clone(),
        ..CacheOptions::default()
    })
    .unwrap();
    let watcher = engine.watcher().unwrap();
    assert_eq!(journal_mode(&database), "delete");
    drop(watcher);
}

/// A journal mode that only a connection holds is gone when the connection
/// closes: a file last used with `MEMORY` is `DELETE` for the next open, and
/// stays so. (This is why no file database can be in `OFF` or `MEMORY` when
/// an engine opens it; the `DatabaseState` refusal for those modes is tested
/// on a connection in `db::schema::tests::configuration`.)
#[test]
fn a_mode_that_only_a_connection_holds_does_not_persist_in_the_file() {
    let dir = TempDir::new().unwrap();
    let database = database_in(&dir, "memory.sqlite3", JournalMode::Memory);
    assert_eq!(journal_mode(&database), "delete");

    default_open_write_close(&dir, &database, "more.txt");

    assert_eq!(journal_mode(&database), "delete");
}
