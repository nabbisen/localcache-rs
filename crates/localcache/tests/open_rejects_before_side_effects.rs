//! RFC 025 Amendment 1: a configuration the engine rejects is rejected before
//! the first side effect. Creating the file, opening a connection to an
//! existing file, migrating and changing a pragma all come later.
//!
//! The rejected configuration here is an encryption key of the wrong length,
//! the one `CacheOptions`-only check that is not already first in `open`.

#![cfg(feature = "encryption")]

mod common;

use std::fs;
use std::path::Path;

use localcache::{CacheEngine, CacheOptions, JournalMode, LocalFileCacheError, SyncCacheEngine};
use rusqlite::{Connection, OpenFlags};
use tempfile::TempDir;

use common::write_file;

type EntryPoint = fn(&Path) -> Result<(), LocalFileCacheError>;

const SHORT_KEY_LEN: usize = 31;

fn short_key() -> Vec<u8> {
    vec![0x5A; SHORT_KEY_LEN]
}

fn rejected_options(database: &Path) -> CacheOptions {
    CacheOptions {
        database_path: database.to_path_buf(),
        encryption_key: Some(short_key()),
        ..CacheOptions::default()
    }
}

fn engine_open(database: &Path) -> Result<(), LocalFileCacheError> {
    CacheEngine::<Vec<f32>>::open(rejected_options(database)).map(drop)
}

fn builder_build(database: &Path) -> Result<(), LocalFileCacheError> {
    CacheEngine::<Vec<f32>>::builder()
        .database(database)
        .encryption_key(short_key())
        .build()
        .map(drop)
}

fn builder_build_read_pool(database: &Path) -> Result<(), LocalFileCacheError> {
    CacheEngine::<Vec<f32>>::builder()
        .database(database)
        .encryption_key(short_key())
        .build_read_pool(2)
        .map(drop)
}

fn read_pool_open(database: &Path) -> Result<(), LocalFileCacheError> {
    localcache::ReadPool::<Vec<f32>>::open(rejected_options(database), 2).map(drop)
}

fn sync_engine_open(database: &Path) -> Result<(), LocalFileCacheError> {
    SyncCacheEngine::<Vec<f32>>::open(rejected_options(database)).map(drop)
}

#[allow(deprecated)]
fn shared_engine_open(database: &Path) -> Result<(), LocalFileCacheError> {
    localcache::shared_engine::<Vec<f32>>(rejected_options(database)).map(drop)
}

#[cfg(any(feature = "async", feature = "async-std", feature = "smol"))]
fn async_engine_open(database: &Path) -> Result<(), LocalFileCacheError> {
    let open = async {
        localcache::AsyncCacheEngine::<Vec<f32>>::open(rejected_options(database))
            .await
            .map(drop)
    };
    #[cfg(feature = "async")]
    {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(open)
    }
    #[cfg(all(not(feature = "async"), feature = "async-std"))]
    {
        async_std::task::block_on(open)
    }
    #[cfg(all(not(feature = "async"), not(feature = "async-std"), feature = "smol"))]
    {
        smol::block_on(open)
    }
}

fn entry_points() -> Vec<(&'static str, EntryPoint)> {
    vec![
        ("CacheEngine::open", engine_open),
        ("CacheEngineBuilder::build", builder_build),
        (
            "CacheEngineBuilder::build_read_pool",
            builder_build_read_pool,
        ),
        ("ReadPool::open", read_pool_open),
        ("SyncCacheEngine::open", sync_engine_open),
        ("shared_engine", shared_engine_open),
        #[cfg(any(feature = "async", feature = "async-std", feature = "smol"))]
        ("AsyncCacheEngine::open", async_engine_open),
    ]
}

/// Every file in `directory`, by name, with its bytes.
fn listing(directory: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<(String, Vec<u8>)> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().to_string_lossy().into_owned(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect();
    files.sort();
    files
}

/// `Err` carries a description of what was wrong, so a test can report every
/// entry point that misbehaves and not only the first.
fn key_length_rejection(name: &str, result: Result<(), LocalFileCacheError>) -> Result<(), String> {
    match result {
        Ok(()) => Err(format!("{name}: a {SHORT_KEY_LEN}-byte key was accepted")),
        Err(error)
            if matches!(
                error,
                LocalFileCacheError::InvalidConfiguration {
                    option: "encryption_key",
                    ..
                }
            ) && error.to_string().contains("exactly 32 bytes") =>
        {
            Ok(())
        }
        Err(error) => Err(format!(
            "{name}: expected the key-length rejection, got: {error}"
        )),
    }
}

fn assert_no_failures(failures: &[String]) {
    assert!(failures.is_empty(), "{failures:#?}");
}

fn pragma(database: &Path, name: &str) -> String {
    let connection =
        Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    connection
        .query_row(&format!("PRAGMA {name}"), [], |row| {
            row.get::<_, rusqlite::types::Value>(0)
        })
        .map(|value| match value {
            rusqlite::types::Value::Text(text) => text,
            rusqlite::types::Value::Integer(number) => number.to_string(),
            other => panic!("unexpected PRAGMA {name} value: {other:?}"),
        })
        .unwrap()
}

/// A current-schema database in the rollback-journal mode, closed cleanly.
fn current_database_in_delete_mode(directory: &TempDir, name: &str) -> std::path::PathBuf {
    let database = directory.path().join(name);
    let engine: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(&database)
        .journal_mode(JournalMode::Delete)
        .build()
        .unwrap();
    engine
        .set(write_file(directory, "source.bin", b"x"), &vec![1.0])
        .unwrap();
    database
}

/// The schema as version 4 wrote it, with one row.
fn version_4_database(directory: &TempDir, name: &str) -> std::path::PathBuf {
    let database = directory.path().join(name);
    let connection = Connection::open(&database).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE files (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                namespace TEXT NOT NULL DEFAULT 'default',
                path TEXT NOT NULL,
                mtime INTEGER NOT NULL,
                file_size INTEGER NOT NULL,
                hash TEXT,
                updated_at INTEGER NOT NULL,
                payload_version INTEGER NOT NULL DEFAULT 0,
                last_accessed_at INTEGER NOT NULL DEFAULT 0,
                UNIQUE(namespace, path)
             );
             CREATE TABLE payloads (
                file_id INTEGER PRIMARY KEY,
                content BLOB NOT NULL,
                encoding TEXT NOT NULL DEFAULT 'raw',
                FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
             );
             CREATE INDEX idx_files_namespace_path ON files(namespace, path);
             CREATE INDEX idx_files_lru
                ON files(namespace, last_accessed_at, updated_at);
             INSERT INTO files(id, namespace, path, mtime, file_size, updated_at)
             VALUES (1, 'default', '/fixture/v4', 8, 9, 10);
             INSERT INTO payloads(file_id, content) VALUES (1, X'0001FF');
             PRAGMA user_version = 4;",
        )
        .unwrap();
    drop(connection);
    database
}

#[test]
fn rejected_open_on_a_missing_path_creates_nothing() {
    let mut failures = Vec::new();
    for (name, open) in entry_points() {
        let directory = TempDir::new().unwrap();
        let database = directory.path().join("absent.sqlite3");

        failures.extend(key_length_rejection(name, open(&database)).err());

        if database.exists() {
            failures.push(format!("{name}: the database file was created"));
        }
        let left = listing(directory.path());
        if !left.is_empty() {
            let names: Vec<_> = left.iter().map(|(file, _)| file.as_str()).collect();
            failures.push(format!("{name}: the rejected open left {names:?}"));
        }
    }
    assert_no_failures(&failures);
}

#[test]
fn rejected_open_leaves_a_current_database_untouched_in_its_journal_mode() {
    let mut failures = Vec::new();
    for (name, open) in entry_points() {
        let directory = TempDir::new().unwrap();
        let database = current_database_in_delete_mode(&directory, "current.sqlite3");
        let version_before = pragma(&database, "user_version");
        assert_eq!(pragma(&database, "journal_mode"), "delete");
        let before = listing(directory.path());

        // The default `journal_mode` is WAL, so an accepted open would switch it.
        failures.extend(key_length_rejection(name, open(&database)).err());

        if listing(directory.path()) != before {
            failures.push(format!("{name}: the files changed"));
        }
        if pragma(&database, "user_version") != version_before {
            failures.push(format!("{name}: user_version changed"));
        }
        let journal_mode = pragma(&database, "journal_mode");
        if journal_mode != "delete" {
            failures.push(format!("{name}: journal_mode is now {journal_mode}"));
        }
    }
    assert_no_failures(&failures);
}

#[test]
fn rejected_open_does_not_migrate_a_version_4_database() {
    let mut failures = Vec::new();
    for (name, open) in entry_points() {
        let directory = TempDir::new().unwrap();
        let database = version_4_database(&directory, "v4.sqlite3");
        assert_eq!(pragma(&database, "user_version"), "4");
        let before = listing(directory.path());

        failures.extend(key_length_rejection(name, open(&database)).err());

        if listing(directory.path()) != before {
            failures.push(format!("{name}: the files changed"));
        }
        let version = pragma(&database, "user_version");
        if version != "4" {
            failures.push(format!("{name}: migrated to user_version {version}"));
        }
    }
    assert_no_failures(&failures);
}

#[test]
fn rejected_open_on_a_wal_database_leaves_no_side_files() {
    let mut failures = Vec::new();
    for (name, open) in entry_points() {
        let directory = TempDir::new().unwrap();
        let database = directory.path().join("wal.sqlite3");
        {
            let engine: CacheEngine<Vec<f32>> = CacheEngine::builder()
                .database(&database)
                .journal_mode(JournalMode::Wal)
                .build()
                .unwrap();
            engine
                .set(write_file(&directory, "source.bin", b"x"), &vec![1.0])
                .unwrap();
        }
        let before = listing(directory.path());

        failures.extend(key_length_rejection(name, open(&database)).err());

        if listing(directory.path()) != before {
            failures.push(format!("{name}: the files changed"));
        }
    }
    assert_no_failures(&failures);
}
