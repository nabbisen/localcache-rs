//! RFC 024 B2 — the CLI's writable commands inherit `JournalMode::Preserve`:
//! they leave an existing database's journal mode alone and give a new
//! database WAL.
//!
//! The journal mode is read from the SQLite file header (bytes 18 and 19, the
//! file-format versions: 1 for a rollback journal, 2 for WAL), which needs no
//! second SQLite binding in this crate's tests.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use localcache::{CacheEngine, JournalMode};
use tempfile::TempDir;

fn localcache(database: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_localcache"))
        .arg("--database")
        .arg(database)
        .args(arguments)
        .output()
        .unwrap()
}

fn succeeded(output: &Output) {
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// `"wal"` or `"delete"`, from the file header of a closed database.
fn journal_mode(database: &Path) -> &'static str {
    let mut header = [0_u8; 20];
    fs::File::open(database)
        .unwrap()
        .read_exact(&mut header)
        .unwrap();
    assert_eq!(header[18], header[19], "write and read versions agree");
    match header[18] {
        1 => "delete",
        2 => "wal",
        other => panic!("unexpected file-format version {other}"),
    }
}

/// A database in `mode` holding one entry, closed.
fn database_in(directory: &TempDir, mode: JournalMode) -> (PathBuf, PathBuf) {
    let database = directory.path().join("db.sqlite3");
    let source = directory.path().join("source.bin");
    fs::write(&source, b"seed").unwrap();
    let engine: CacheEngine<Vec<u8>> = CacheEngine::builder()
        .database(&database)
        .journal_mode(mode)
        .build()
        .unwrap();
    engine.set(&source, &b"seed".to_vec()).unwrap();
    drop(engine);
    (database, source)
}

/// A one-record import file for `path`.
fn import_file(directory: &TempDir, path: &Path) -> PathBuf {
    let engine: CacheEngine<Vec<u8>> = CacheEngine::builder().database(":memory:").build().unwrap();
    engine.set(path, &b"imported".to_vec()).unwrap();
    let records = engine.export_entries().unwrap();
    let file = directory.path().join("in.jsonl");
    fs::write(&file, serde_json::to_string(&records[0]).unwrap()).unwrap();
    file
}

#[test]
fn import_and_cleanup_leave_a_delete_database_delete() {
    let directory = TempDir::new().unwrap();
    let (database, source) = database_in(&directory, JournalMode::Delete);
    assert_eq!(journal_mode(&database), "delete", "sanity");
    let input = import_file(&directory, &source);

    succeeded(&localcache(
        &database,
        &["import", "--input", input.to_str().unwrap()],
    ));
    assert_eq!(journal_mode(&database), "delete", "after import");

    succeeded(&localcache(&database, &["cleanup"]));
    assert_eq!(journal_mode(&database), "delete", "after cleanup");
    assert!(!directory.path().join("db.sqlite3-wal").exists());
}

#[test]
fn import_and_cleanup_leave_a_wal_database_wal() {
    let directory = TempDir::new().unwrap();
    let (database, source) = database_in(&directory, JournalMode::Wal);
    assert_eq!(journal_mode(&database), "wal", "sanity");
    let input = import_file(&directory, &source);

    succeeded(&localcache(
        &database,
        &["import", "--input", input.to_str().unwrap()],
    ));
    succeeded(&localcache(&database, &["cleanup"]));

    assert_eq!(journal_mode(&database), "wal");
}

#[test]
fn a_writable_command_gives_a_new_database_wal() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("created.sqlite3");

    succeeded(&localcache(&database, &["cleanup"]));

    assert_eq!(journal_mode(&database), "wal");
}
