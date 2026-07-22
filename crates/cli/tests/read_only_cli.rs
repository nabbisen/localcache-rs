use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use localcache::{CacheEngine, CacheOptions};
use tempfile::TempDir;

fn localcache(database: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_localcache"))
        .arg("--database")
        .arg(database)
        .args(arguments)
        .output()
        .unwrap()
}

fn write_file(directory: &TempDir, name: &str, content: &[u8]) -> PathBuf {
    let path = directory.path().join(name);
    fs::write(&path, content).unwrap();
    path
}

#[test]
fn observational_commands_do_not_create_or_change_the_cache_database() {
    let directory = TempDir::new().unwrap();
    for (name, arguments) in [
        ("missing-list.sqlite3", vec!["list"]),
        ("missing-inspect.sqlite3", vec!["inspect", "missing.bin"]),
    ] {
        let database = directory.path().join(name);
        let output = localcache(&database, &arguments);
        assert!(!output.status.success());
        assert!(!database.exists());
    }

    let database = directory.path().join("current.sqlite3");
    let source = write_file(&directory, "current.bin", b"current");
    {
        let writer: CacheEngine<Vec<u8>> =
            CacheEngine::builder().database(&database).build().unwrap();
        writer.set(&source, &b"payload".to_vec()).unwrap();
    }
    let before = fs::read(&database).unwrap();
    let source_text = source.to_str().unwrap();
    let directory_text = directory.path().to_str().unwrap();
    for arguments in [
        vec!["list"],
        vec!["stats"],
        vec!["check", source_text],
        vec!["scan", directory_text],
        vec!["export"],
        vec!["query"],
        vec!["inspect", source_text],
        vec!["namespaces"],
    ] {
        let output = localcache(&database, &arguments);
        assert!(
            output.status.success(),
            "command {arguments:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(fs::read(database).unwrap(), before);
}

#[test]
fn observational_command_rejects_historical_fixture_without_migration() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("historical.sqlite3");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../localcache/tests/fixtures/compat-v0_1.sqlite3");
    fs::copy(fixture, &database).unwrap();
    let before = fs::read(&database).unwrap();

    let output = localcache(&database, &["list"]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("read-only open requires the current database schema")
    );
    assert_eq!(fs::read(database).unwrap(), before);
}

#[test]
fn copy_opens_writable_destination_before_read_only_source() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("copy.sqlite3");
    let source_path = write_file(&directory, "copy.bin", b"copy");
    {
        let source: CacheEngine<Vec<u8>> = CacheEngine::builder()
            .database(&database)
            .namespace("source")
            .build()
            .unwrap();
        source.set(&source_path, &b"payload".to_vec()).unwrap();
    }

    let output = Command::new(env!("CARGO_BIN_EXE_localcache"))
        .args([
            "--database",
            database.to_str().unwrap(),
            "--namespace",
            "destination",
            "copy",
            "--from",
            "source",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let destination: CacheEngine<Vec<u8>> = CacheEngine::builder()
        .database(database)
        .namespace("destination")
        .read_only()
        .build()
        .unwrap();
    assert_eq!(
        destination.get(source_path).unwrap().unwrap().payload,
        b"payload"
    );
}

#[test]
fn migrate_may_upgrade_its_source_and_populates_destination() {
    let directory = TempDir::new().unwrap();
    let source = directory.path().join("source.sqlite3");
    let destination = directory.path().join("destination.sqlite3");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../localcache/tests/fixtures/compat-v0_1.sqlite3");
    fs::copy(fixture, &source).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_localcache"))
        .args([
            "migrate",
            "--src-db",
            source.to_str().unwrap(),
            "--dst-db",
            destination.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    for database in [source, destination] {
        CacheEngine::<Vec<u8>>::open(CacheOptions {
            database_path: database,
            read_only: true,
            ..CacheOptions::default()
        })
        .unwrap();
    }
}

#[cfg(feature = "watching")]
#[test]
fn watch_command_is_writable_and_can_initialize_an_empty_cache() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("watch.sqlite3");
    let output = localcache(&database, &["watch"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(database.exists());
    CacheEngine::<Vec<u8>>::open(CacheOptions {
        database_path: database,
        read_only: true,
        ..CacheOptions::default()
    })
    .unwrap();
}
