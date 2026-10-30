//! RFC 025 Amendment 1: a configuration the engine rejects is rejected before
//! the first side effect. Creating the file, opening a connection to an
//! existing file, migrating and changing a pragma all come later.
//!
//! Every rejection the library makes from the options alone is run through
//! every entry point: an encryption key of the wrong length (with the
//! `encryption` feature), `max_entries(0)`, and a TTL under one second (RFC 025
//! R6, v0.22.0).

mod common;

use std::fs;
use std::path::Path;
use std::time::Duration;

use localcache::{
    CacheEngine, CacheEngineBuilder, CacheOptions, JournalMode, LocalFileCacheError,
    SyncCacheEngine,
};
use rusqlite::{Connection, OpenFlags};
use tempfile::TempDir;

use common::write_file;

/// A configuration that must be rejected before anything is touched.
#[derive(Clone, Copy, Debug)]
enum Rejection {
    ZeroMaxEntries,
    ZeroTtl,
    SubSecondTtl,
    #[cfg(feature = "encryption")]
    ShortKey,
}

const REJECTIONS: &[Rejection] = &[
    Rejection::ZeroMaxEntries,
    Rejection::ZeroTtl,
    Rejection::SubSecondTtl,
    #[cfg(feature = "encryption")]
    Rejection::ShortKey,
];

impl Rejection {
    fn options(self, database: &Path) -> CacheOptions {
        let mut options = CacheOptions {
            database_path: database.to_path_buf(),
            ..CacheOptions::default()
        };
        match self {
            Self::ZeroMaxEntries => options.max_entries = Some(0),
            Self::ZeroTtl => options.ttl = Some(Duration::ZERO),
            Self::SubSecondTtl => options.ttl = Some(Duration::from_millis(999)),
            #[cfg(feature = "encryption")]
            Self::ShortKey => options.encryption_key = Some(vec![0x5A; 31]),
        }
        options
    }

    fn configure(self, builder: CacheEngineBuilder<Vec<f32>>) -> CacheEngineBuilder<Vec<f32>> {
        match self {
            Self::ZeroMaxEntries => builder.max_entries(0),
            Self::ZeroTtl => builder.ttl(Duration::ZERO),
            Self::SubSecondTtl => builder.ttl(Duration::from_millis(999)),
            #[cfg(feature = "encryption")]
            Self::ShortKey => builder.encryption_key(vec![0x5A; 31]),
        }
    }

    /// The `option` the error names and its whole `Display`.
    fn expected(self) -> (&'static str, &'static str) {
        match self {
            Self::ZeroMaxEntries => (
                "max_entries",
                "invalid configuration: max_entries: must be at least 1; use None for no limit",
            ),
            Self::ZeroTtl => (
                "ttl",
                "invalid configuration: ttl: must be at least one second, got 0ns",
            ),
            Self::SubSecondTtl => (
                "ttl",
                "invalid configuration: ttl: must be at least one second, got 999ms",
            ),
            #[cfg(feature = "encryption")]
            Self::ShortKey => (
                "encryption_key",
                "invalid configuration: encryption_key: must be exactly 32 bytes, got 31",
            ),
        }
    }

    /// `Err` carries a description of what was wrong, so a test can report every
    /// entry point that misbehaves and not only the first.
    fn check(self, name: &str, result: Result<(), LocalFileCacheError>) -> Result<(), String> {
        let (option, display) = self.expected();
        match result {
            Ok(()) => Err(format!("{name}: the rejected configuration was accepted")),
            Err(error @ LocalFileCacheError::InvalidConfiguration { option: found, .. })
                if found == option && error.to_string() == display =>
            {
                Ok(())
            }
            Err(error) => Err(format!("{name}: expected `{display}`, got: {error}")),
        }
    }
}

type EntryPoint = fn(&Path, Rejection) -> Result<(), LocalFileCacheError>;

fn engine_open(database: &Path, rejection: Rejection) -> Result<(), LocalFileCacheError> {
    CacheEngine::<Vec<f32>>::open(rejection.options(database)).map(drop)
}

fn builder_build(database: &Path, rejection: Rejection) -> Result<(), LocalFileCacheError> {
    rejection
        .configure(CacheEngine::<Vec<f32>>::builder().database(database))
        .build()
        .map(drop)
}

fn builder_build_read_pool(
    database: &Path,
    rejection: Rejection,
) -> Result<(), LocalFileCacheError> {
    rejection
        .configure(CacheEngine::<Vec<f32>>::builder().database(database))
        .build_read_pool(2)
        .map(drop)
}

fn read_pool_open(database: &Path, rejection: Rejection) -> Result<(), LocalFileCacheError> {
    localcache::ReadPool::<Vec<f32>>::open(rejection.options(database), 2).map(drop)
}

fn sync_engine_open(database: &Path, rejection: Rejection) -> Result<(), LocalFileCacheError> {
    SyncCacheEngine::<Vec<f32>>::open(rejection.options(database)).map(drop)
}

#[cfg(any(feature = "async", feature = "async-std", feature = "smol"))]
fn async_engine_open(database: &Path, rejection: Rejection) -> Result<(), LocalFileCacheError> {
    let open = async {
        localcache::AsyncCacheEngine::<Vec<f32>>::open(rejection.options(database))
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

/// Every entry point that builds an engine or a pool, with every rejection.
fn cases() -> Vec<(String, EntryPoint, Rejection)> {
    let entry_points: Vec<(&str, EntryPoint)> = vec![
        ("CacheEngine::open", engine_open),
        ("CacheEngineBuilder::build", builder_build),
        (
            "CacheEngineBuilder::build_read_pool",
            builder_build_read_pool,
        ),
        ("ReadPool::open", read_pool_open),
        ("SyncCacheEngine::open", sync_engine_open),
        #[cfg(any(feature = "async", feature = "async-std", feature = "smol"))]
        ("AsyncCacheEngine::open", async_engine_open),
    ];
    let mut cases = Vec::new();
    for rejection in REJECTIONS {
        for (name, open) in &entry_points {
            cases.push((format!("{name} / {rejection:?}"), *open, *rejection));
        }
    }
    cases
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
    for (name, open, rejection) in cases() {
        let directory = TempDir::new().unwrap();
        let database = directory.path().join("absent.sqlite3");

        failures.extend(rejection.check(&name, open(&database, rejection)).err());

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
    for (name, open, rejection) in cases() {
        let directory = TempDir::new().unwrap();
        let database = current_database_in_delete_mode(&directory, "current.sqlite3");
        let version_before = pragma(&database, "user_version");
        assert_eq!(pragma(&database, "journal_mode"), "delete");
        let before = listing(directory.path());

        // The default `journal_mode` is WAL, so an accepted open would switch it.
        failures.extend(rejection.check(&name, open(&database, rejection)).err());

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
    for (name, open, rejection) in cases() {
        let directory = TempDir::new().unwrap();
        let database = version_4_database(&directory, "v4.sqlite3");
        assert_eq!(pragma(&database, "user_version"), "4");
        let before = listing(directory.path());

        failures.extend(rejection.check(&name, open(&database, rejection)).err());

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
    for (name, open, rejection) in cases() {
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

        failures.extend(rejection.check(&name, open(&database, rejection)).err());

        if listing(directory.path()) != before {
            failures.push(format!("{name}: the files changed"));
        }
    }
    assert_no_failures(&failures);
}
