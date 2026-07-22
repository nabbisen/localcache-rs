mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[cfg(feature = "watching")]
use std::time::Duration;

use localcache::{
    CacheEngine, CacheOptions, ConnectionPool, ExportRecord, LocalFileCacheError, ReadPool,
    ScanOptions, shared_engine,
};
use rusqlite::{Connection, OpenFlags};
use tempfile::TempDir;

use common::write_file;

const HISTORICAL_READ_ONLY_ERROR: &str = "unsupported feature: read-only open requires the current database schema; initialization or migration is not permitted; database was not modified";

#[derive(Debug, PartialEq, Eq)]
struct CurrentSnapshot {
    user_version: i64,
    schema: Vec<(String, String, String, Option<String>)>,
    files: Vec<FileSnapshot>,
    payloads: Vec<(i64, Vec<u8>, String)>,
}

#[derive(Debug, PartialEq, Eq)]
struct FileSnapshot {
    id: i64,
    namespace: String,
    path: String,
    mtime: i64,
    file_size: i64,
    hash: Option<String>,
    updated_at: i64,
    payload_version: i64,
    last_accessed_at: i64,
}

fn current_snapshot(path: &Path) -> CurrentSnapshot {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let schema = conn
        .prepare(
            "SELECT type, name, tbl_name, sql
             FROM main.sqlite_schema ORDER BY type, name",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let files = conn
        .prepare(
            "SELECT id, namespace, path, mtime, file_size, hash, updated_at,
                    payload_version, last_accessed_at
             FROM main.files ORDER BY id",
        )
        .unwrap()
        .query_map([], |row| {
            Ok(FileSnapshot {
                id: row.get(0)?,
                namespace: row.get(1)?,
                path: row.get(2)?,
                mtime: row.get(3)?,
                file_size: row.get(4)?,
                hash: row.get(5)?,
                updated_at: row.get(6)?,
                payload_version: row.get(7)?,
                last_accessed_at: row.get(8)?,
            })
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let payloads = conn
        .prepare("SELECT file_id, content, encoding FROM main.payloads ORDER BY file_id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    CurrentSnapshot {
        user_version: conn
            .query_row("PRAGMA main.user_version", [], |row| row.get(0))
            .unwrap(),
        schema,
        files,
        payloads,
    }
}

fn assert_read_only(error: LocalFileCacheError) {
    assert!(matches!(error, LocalFileCacheError::ReadOnly), "{error:?}");
}

fn open_error(options: CacheOptions) -> LocalFileCacheError {
    match CacheEngine::<Vec<f32>>::open(options) {
        Ok(_) => panic!("read-only open unexpectedly succeeded"),
        Err(error) => error,
    }
}

fn read_pool_error(options: CacheOptions) -> LocalFileCacheError {
    match ReadPool::<Vec<f32>>::open(options, 2) {
        Ok(_) => panic!("read-only pool unexpectedly opened"),
        Err(error) => error,
    }
}

fn create_historical(path: &Path, version: u8) {
    let conn = Connection::open(path).unwrap();
    let schema = match version {
        2 => {
            "CREATE TABLE files (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                namespace TEXT NOT NULL DEFAULT 'default',
                path TEXT NOT NULL,
                mtime INTEGER NOT NULL,
                file_size INTEGER NOT NULL,
                hash TEXT,
                updated_at INTEGER NOT NULL,
                UNIQUE(namespace, path)
             );
             CREATE TABLE payloads (
                file_id INTEGER PRIMARY KEY,
                content BLOB NOT NULL,
                FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
             );
             CREATE INDEX idx_files_namespace_path ON files(namespace, path);
             INSERT INTO files(id, namespace, path, mtime, file_size, updated_at)
             VALUES (7, 'legacy', '/v2.bin', 8, 9, 10);
             INSERT INTO payloads(file_id, content) VALUES (7, X'0001FF');
             PRAGMA user_version = 2;"
        }
        3 => {
            "CREATE TABLE files (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                namespace TEXT NOT NULL DEFAULT 'default',
                path TEXT NOT NULL,
                mtime INTEGER NOT NULL,
                file_size INTEGER NOT NULL,
                hash TEXT,
                updated_at INTEGER NOT NULL,
                payload_version INTEGER NOT NULL DEFAULT 0,
                UNIQUE(namespace, path)
             );
             CREATE TABLE payloads (
                file_id INTEGER PRIMARY KEY,
                content BLOB NOT NULL,
                encoding TEXT NOT NULL DEFAULT 'raw',
                FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
             );
             CREATE INDEX idx_files_namespace_path ON files(namespace, path);
             INSERT INTO files(id, namespace, path, mtime, file_size, updated_at,
                               payload_version)
             VALUES (7, 'legacy', '/v3.bin', 8, 9, 10, 11);
             INSERT INTO payloads(file_id, content, encoding)
             VALUES (7, X'0001FF', 'raw');
             PRAGMA user_version = 3;"
        }
        _ => unreachable!(),
    };
    conn.execute_batch(schema).unwrap();
}

fn assert_historical_rejected_unchanged(path: &Path) {
    let before = fs::read(path).unwrap();
    let error = open_error(CacheOptions {
        database_path: path.to_path_buf(),
        read_only: true,
        ..CacheOptions::default()
    });
    assert_eq!(error.to_string(), HISTORICAL_READ_ONLY_ERROR);
    assert_eq!(fs::read(path).unwrap(), before);
    assert!(!PathBuf::from(format!("{}-wal", path.display())).exists());
    assert!(!PathBuf::from(format!("{}-shm", path.display())).exists());
}

#[test]
fn exact_current_schema_supports_read_only_surfaces_without_lru_mutation() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("current.sqlite3");
    let source = write_file(&directory, "current.bin", b"current");
    {
        let writer: CacheEngine<Vec<f32>> = CacheEngine::builder()
            .database(&database)
            .namespace("readonly")
            .build()
            .unwrap();
        writer.set(&source, &vec![1.0, 2.0]).unwrap();
        writer.create_path_index("readonly").unwrap();
    }
    let before = current_snapshot(&database);

    let reader: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(&database)
        .namespace("readonly")
        .read_only()
        .build()
        .unwrap();
    assert_eq!(
        reader.get(&source).unwrap().unwrap().payload,
        vec![1.0, 2.0]
    );
    assert_eq!(reader.list_path_indexes().unwrap(), ["lc_user_readonly"]);
    assert_eq!(
        reader
            .query()
            .index_hint("lc_user_readonly")
            .run()
            .unwrap()
            .len(),
        1
    );
    assert!(
        reader
            .query()
            .index_hint("lc_user_readonly")
            .dry_run()
            .unwrap()
            .contains("lc_user_readonly")
    );
    drop(reader);
    assert_eq!(current_snapshot(&database), before);

    let shared_reader: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(&database)
        .namespace("readonly")
        .shared_cache()
        .build()
        .unwrap();
    assert!(shared_reader.contains(&source).unwrap());
    drop(shared_reader);

    let connection_pool = ConnectionPool::<Vec<f32>>::open(CacheOptions {
        database_path: database.clone(),
        namespace: "readonly".into(),
        read_only: true,
        ..CacheOptions::default()
    })
    .unwrap();
    assert!(connection_pool.contains(&source).unwrap());

    let pool = ReadPool::<Vec<f32>>::open(
        CacheOptions {
            database_path: database,
            namespace: "readonly".into(),
            ..CacheOptions::default()
        },
        2,
    )
    .unwrap();
    assert!(pool.contains(source).unwrap());
}

#[test]
fn historical_v0_v2_v3_and_v4_schemas_reject_without_writes() {
    let directory = TempDir::new().unwrap();
    let fixture_directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for fixture in ["compat-v0_1.sqlite3", "compat-v0_19-user-index.sqlite3"] {
        let destination = directory.path().join(fixture);
        fs::copy(fixture_directory.join(fixture), &destination).unwrap();
        assert_historical_rejected_unchanged(&destination);
    }

    for version in [2, 3] {
        let path = directory.path().join(format!("v{version}.sqlite3"));
        create_historical(&path, version);
        assert_historical_rejected_unchanged(&path);
        if version == 2 {
            let before = fs::read(&path).unwrap();
            let error = read_pool_error(CacheOptions {
                database_path: path.clone(),
                ..CacheOptions::default()
            });
            assert_eq!(error.to_string(), HISTORICAL_READ_ONLY_ERROR);
            assert_eq!(fs::read(path).unwrap(), before);
        }
    }
}

#[test]
fn fresh_future_and_malformed_schemas_reject_unchanged() {
    let directory = TempDir::new().unwrap();

    let empty = directory.path().join("empty.sqlite3");
    let conn = Connection::open(&empty).unwrap();
    conn.execute_batch("PRAGMA user_version = 0;").unwrap();
    drop(conn);
    assert_historical_rejected_unchanged(&empty);

    for (name, sql) in [
        (
            "negative.sqlite3",
            "CREATE TABLE invalid(value TEXT); PRAGMA user_version = -1;",
        ),
        (
            "future.sqlite3",
            "CREATE TABLE future(value TEXT); PRAGMA user_version = 6;",
        ),
        (
            "malformed.sqlite3",
            "CREATE TABLE wrong(value TEXT); PRAGMA user_version = 5;",
        ),
    ] {
        let path = directory.path().join(name);
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(sql).unwrap();
        drop(conn);
        let before = fs::read(&path).unwrap();
        let error = open_error(CacheOptions {
            database_path: path.clone(),
            read_only: true,
            ..CacheOptions::default()
        });
        assert!(
            matches!(error, LocalFileCacheError::UnsupportedFeature(ref message)
                if message.contains("unrecognized database schema")
                    && message.contains("database was not modified")),
            "{error:?}"
        );
        assert_eq!(fs::read(path).unwrap(), before);
    }
}

#[test]
fn missing_file_and_explicit_read_only_memory_never_initialize() {
    let directory = TempDir::new().unwrap();
    let missing = directory.path().join("missing.sqlite3");
    let error = open_error(CacheOptions {
        database_path: missing.clone(),
        read_only: true,
        ..CacheOptions::default()
    });
    assert!(matches!(error, LocalFileCacheError::Database(_)));
    assert!(!missing.exists());

    for shared_cache in [false, true] {
        let error = open_error(CacheOptions {
            database_path: ":memory:".into(),
            read_only: true,
            shared_cache,
            ..CacheOptions::default()
        });
        assert_eq!(
            error.to_string(),
            "unsupported feature: read-only mode does not support in-memory databases"
        );
    }

    let shared: CacheEngine<Vec<f32>> = CacheEngine::open(CacheOptions {
        database_path: ":memory:".into(),
        shared_cache: true,
        ..CacheOptions::default()
    })
    .unwrap();
    let source = write_file(&directory, "shared.bin", b"shared");
    shared.set(&source, &vec![3.0]).unwrap();
    assert_eq!(shared.get(source).unwrap().unwrap().payload, vec![3.0]);
}

#[test]
fn every_public_mutator_returns_read_only_before_other_work() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("mutators.sqlite3");
    let source = write_file(&directory, "mutators.bin", b"before");
    {
        let writer: CacheEngine<Vec<f32>> = CacheEngine::builder()
            .database(&database)
            .namespace("readonly")
            .build()
            .unwrap();
        writer.set(&source, &vec![5.0]).unwrap();
    }
    let before = current_snapshot(&database);

    let reader: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(&database)
        .namespace("readonly")
        .read_only()
        .build()
        .unwrap();
    let missing = directory.path().join("missing.bin");
    assert_read_only(reader.set(&missing, &vec![0.0]).unwrap_err());
    let empty_batch: Vec<(PathBuf, Vec<f32>)> = Vec::new();
    assert_read_only(reader.batch_set(&empty_batch).unwrap_err());
    assert_read_only(reader.remove(&missing).unwrap_err());
    assert_read_only(reader.touch(&missing).unwrap_err());
    let records: Vec<ExportRecord> = Vec::new();
    assert_read_only(reader.import_entries(&records).unwrap_err());
    assert_read_only(reader.import_from(&reader).unwrap_err());

    let factory_called = Arc::new(AtomicBool::new(false));
    let factory_called_in_call = Arc::clone(&factory_called);
    assert_read_only(
        reader
            .preload(&missing, ScanOptions::default(), false, move |_| {
                factory_called_in_call.store(true, Ordering::SeqCst);
                Ok(vec![0.0])
            })
            .unwrap_err(),
    );
    assert!(!factory_called.load(Ordering::SeqCst));
    assert_read_only(reader.namespace_copy(&reader).unwrap_err());
    assert_read_only(reader.cleanup_missing_files().unwrap_err());
    assert_read_only(reader.cleanup_expired().unwrap_err());
    assert_read_only(reader.purge_stale_versions().unwrap_err());
    assert_read_only(reader.shrink_database().unwrap_err());
    assert_read_only(reader.create_path_index("bad suffix").unwrap_err());
    assert_read_only(reader.drop_path_index("bad suffix").unwrap_err());

    #[cfg(feature = "encryption")]
    assert_read_only(reader.rotate_encryption_key(&[]).unwrap_err());

    #[cfg(feature = "watching")]
    {
        assert_read_only(reader.watcher().err().unwrap());
        assert_read_only(
            reader
                .debounced_watcher(Duration::from_millis(10))
                .err()
                .unwrap(),
        );
        fs::write(&source, b"after rejected watcher").unwrap();
        assert!(reader.contains(&source).unwrap());
    }

    drop(reader);
    assert_eq!(current_snapshot(&database), before);

    let callback_called = Arc::new(AtomicBool::new(false));
    let callback_called_in_engine = Arc::clone(&callback_called);
    let reader_with_callback: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(&database)
        .namespace("readonly")
        .read_only()
        .max_entries(0)
        .on_evict(move |_| callback_called_in_engine.store(true, Ordering::SeqCst))
        .build()
        .unwrap();
    assert_read_only(reader_with_callback.set(&missing, &vec![0.0]).unwrap_err());
    assert!(!callback_called.load(Ordering::SeqCst));
}

#[test]
fn pool_escape_hatches_and_manual_shared_engine_keep_original_guard() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("pool.sqlite3");
    CacheEngine::<Vec<f32>>::builder()
        .database(&database)
        .build()
        .unwrap();
    let missing = directory.path().join("missing.bin");
    let options = CacheOptions {
        database_path: database,
        read_only: true,
        ..CacheOptions::default()
    };

    let pool = ConnectionPool::<Vec<f32>>::open(options.clone()).unwrap();
    assert_read_only(pool.touch(&missing).unwrap_err());
    assert_read_only(
        pool.with(|engine| engine.set(&missing, &vec![1.0]))
            .unwrap_err(),
    );
    assert_read_only(pool.with_mut(|engine| engine.remove(&missing)).unwrap_err());

    let shared = shared_engine::<Vec<f32>>(options).unwrap();
    let error = shared.lock().unwrap().set(missing, &vec![1.0]).unwrap_err();
    assert_read_only(error);
}

#[cfg(any(feature = "async", feature = "async-std", feature = "smol"))]
async fn async_read_only_parity() {
    use localcache::AsyncCacheEngine;

    let directory = TempDir::new().unwrap();
    let database = directory.path().join("async.sqlite3");
    let source = write_file(&directory, "async.bin", b"async");
    {
        let writer: CacheEngine<Vec<f32>> =
            CacheEngine::builder().database(&database).build().unwrap();
        writer.set(&source, &vec![8.0]).unwrap();
    }
    let reader: AsyncCacheEngine<Vec<f32>> = AsyncCacheEngine::open(CacheOptions {
        database_path: database,
        read_only: true,
        ..CacheOptions::default()
    })
    .await
    .unwrap();
    assert_eq!(
        reader.get(source.clone()).await.unwrap().unwrap().payload,
        vec![8.0]
    );
    assert_read_only(reader.touch(source.clone()).await.unwrap_err());
    assert_read_only(reader.set(source, vec![9.0]).await.unwrap_err());

    let historical = directory.path().join("async-v2.sqlite3");
    create_historical(&historical, 2);
    let before = fs::read(&historical).unwrap();
    let error = match AsyncCacheEngine::<Vec<f32>>::open(CacheOptions {
        database_path: historical.clone(),
        read_only: true,
        ..CacheOptions::default()
    })
    .await
    {
        Ok(_) => panic!("historical schema unexpectedly opened asynchronously"),
        Err(error) => error,
    };
    assert_eq!(error.to_string(), HISTORICAL_READ_ONLY_ERROR);
    assert_eq!(fs::read(historical).unwrap(), before);
}

#[cfg(feature = "async")]
#[tokio::test]
async fn tokio_read_only_contract_matches_sync() {
    async_read_only_parity().await;
}

#[cfg(all(not(feature = "async"), feature = "async-std"))]
#[test]
fn async_std_read_only_contract_matches_sync() {
    async_std::task::block_on(async_read_only_parity());
}

#[cfg(all(not(feature = "async"), not(feature = "async-std"), feature = "smol"))]
#[test]
fn smol_read_only_contract_matches_sync() {
    smol::block_on(async_read_only_parity());
}
