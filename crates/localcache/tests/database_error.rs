//! RFC 025 R7: `LocalFileCacheError::Database` carries an opaque `DatabaseError`.
//!
//! The assertions name no database library type: a caller recognises a busy
//! database, reads the message and walks `source()` through this crate's own
//! types. The busy test uses one connection of the library only to hold the write
//! lock, the way a second process would.

mod common;

use localcache::{CacheEngine, CacheOptions, DatabaseError, LocalFileCacheError};
use tempfile::TempDir;

use common::write_file;

fn assert_send_sync_static<T: Send + Sync + 'static>() {}

#[test]
fn the_error_types_are_send_sync_and_static() {
    assert_send_sync_static::<LocalFileCacheError>();
    assert_send_sync_static::<DatabaseError>();
}

/// `SQLITE_BUSY` is primary result code 5. The write below waits out the
/// connection's busy timeout (about five seconds) before it gives up, so this
/// test takes that long.
#[test]
fn a_busy_database_is_recognised_without_the_database_library() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("busy.sqlite3");
    let source = write_file(&directory, "source.bin", b"x");
    let engine: CacheEngine<Vec<f32>> = CacheEngine::builder().database(&database).build().unwrap();

    // A second connection holds the write lock, as a second process would.
    let blocker = rusqlite::Connection::open(&database).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE;").unwrap();

    let error = engine.set(&source, &vec![1.0]).unwrap_err();
    match error {
        LocalFileCacheError::Database(database_error) => {
            assert_eq!(
                database_error
                    .sqlite_extended_code()
                    .map(|code| code & 0xff),
                Some(5),
                "expected SQLITE_BUSY, got {database_error}"
            );
        }
        other => panic!("expected a database error, got {other}"),
    }
    blocker.execute_batch("ROLLBACK;").unwrap();
}

/// A database that cannot be opened: SQLite reports `SQLITE_CANTOPEN`.
fn open_failure(directory: &TempDir) -> (std::path::PathBuf, LocalFileCacheError) {
    let missing = directory
        .path()
        .join("no-such-directory")
        .join("cache.sqlite3");
    let options = CacheOptions {
        database_path: missing.clone(),
        ..CacheOptions::default()
    };
    match CacheEngine::<Vec<f32>>::open(options) {
        Ok(_) => panic!("opening a database in a missing directory succeeded"),
        Err(error) => (missing, error),
    }
}

#[test]
fn the_message_is_the_one_the_database_library_printed() {
    let directory = TempDir::new().unwrap();
    let (missing, error) = open_failure(&directory);
    let expected = format!("unable to open database file: {}", missing.display());

    assert_eq!(error.to_string(), format!("database error: {expected}"));
    match &error {
        LocalFileCacheError::Database(database_error) => {
            assert_eq!(database_error.to_string(), expected);
            // CANTOPEN is primary result code 14.
            assert_eq!(
                database_error
                    .sqlite_extended_code()
                    .map(|code| code & 0xff),
                Some(14)
            );
        }
        other => panic!("expected a database error, got {other}"),
    }
}

#[test]
fn source_walks_from_the_error_to_the_database_error_to_the_cause() {
    let directory = TempDir::new().unwrap();
    let (_missing, error) = open_failure(&directory);

    let database_error =
        std::error::Error::source(&error).expect("LocalFileCacheError::Database has a source");
    assert!(
        database_error.is::<DatabaseError>(),
        "the source of the library error is the DatabaseError"
    );
    assert!(
        database_error.source().is_some(),
        "the DatabaseError has the underlying error as its source"
    );
}
