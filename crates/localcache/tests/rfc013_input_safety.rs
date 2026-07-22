mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use localcache::{
    CacheEngine, CacheOptions, CacheStatus, ChangeDetectionMode, ConnectionPool,
    LocalFileCacheError, ReadPool, ScanOptions,
};
use tempfile::TempDir;

use common::write_file;

const MALFORMED: &str = "invalid glob pattern: malformed brace syntax";
const SAFETY: &str = "invalid glob pattern: safety limit exceeded";

fn engine() -> CacheEngine<Vec<u8>> {
    CacheEngine::builder()
        .database(":memory:")
        .change_detection(ChangeDetectionMode::MetadataOnly)
        .build()
        .unwrap()
}

fn assert_malformed<T>(result: Result<T, LocalFileCacheError>) {
    assert!(matches!(
        result,
        Err(LocalFileCacheError::UnsupportedFeature(message)) if message == MALFORMED
    ));
}

fn assert_safety<T>(result: Result<T, LocalFileCacheError>) {
    assert!(matches!(
        result,
        Err(LocalFileCacheError::UnsupportedFeature(message)) if message == SAFETY
    ));
}

fn names(paths: impl IntoIterator<Item = PathBuf>) -> Vec<String> {
    let mut names = paths
        .into_iter()
        .map(|path| path.file_name().unwrap().to_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    names.sort();
    names
}

#[test]
fn unicode_scan_and_query_globs_have_equivalent_scalar_semantics() {
    let dir = TempDir::new().unwrap();
    let engine = engine();
    for name in [
        "a.txt",
        "é.txt",
        "e\u{301}.txt",
        "東.txt",
        "🙂.txt",
        "[x].txt",
        "AA.txt",
        "aa.txt",
    ] {
        let path = write_file(&dir, name, b"x");
        engine.set(path, &vec![1]).unwrap();
    }

    let root = dir.path().canonicalize().unwrap();
    let mut prefix = root.to_str().unwrap().to_owned();
    prefix.push(std::path::MAIN_SEPARATOR);

    for pattern in ["?.txt", "?*.txt", "{é,東,🙂}.txt", "[x].txt", "AA.txt"] {
        let scanned = engine
            .scan_dir_filtered(
                &root,
                ScanOptions {
                    glob_pattern: Some(pattern.to_owned()),
                    ..ScanOptions::default()
                },
            )
            .unwrap()
            .into_iter()
            .map(|(path, _)| path)
            .collect::<Vec<_>>();
        let queried = engine
            .query()
            .path_glob(format!("{prefix}{pattern}"))
            .run()
            .unwrap()
            .into_iter()
            .map(|entry| entry.path)
            .collect::<Vec<_>>();
        assert_eq!(names(scanned), names(queried), "pattern {pattern:?}");
    }
}

#[test]
fn malformed_globs_fail_at_scan_and_query_terminals() {
    let dir = TempDir::new().unwrap();
    let engine = engine();

    assert_malformed(engine.scan_dir_filtered(
        dir.path(),
        ScanOptions {
            glob_pattern: Some("?*é{".to_owned()),
            ..ScanOptions::default()
        },
    ));
    assert_malformed(engine.query().path_glob("}").run());
    assert_malformed(engine.query().path_glob("}").dry_run());
    let over_limit = "a".repeat(16_385);
    assert_safety(engine.query().path_glob(&over_limit).run());
    assert_safety(engine.query().path_glob(&over_limit).dry_run());
    assert_safety(engine.scan_dir_filtered(
        dir.path(),
        ScanOptions {
            glob_pattern: Some(over_limit),
            ..ScanOptions::default()
        },
    ));

    let missing = dir.path().join("missing");
    assert!(matches!(
        engine.scan_dir_filtered(
            missing,
            ScanOptions {
                glob_pattern: Some("}".to_owned()),
                ..ScanOptions::default()
            }
        ),
        Err(LocalFileCacheError::Io(_))
    ));
}

#[test]
fn invalid_preload_glob_never_invokes_the_payload_factory() {
    let dir = TempDir::new().unwrap();
    let engine = engine();
    write_file(&dir, "candidate.bin", b"x");
    let called = AtomicBool::new(false);

    let result = engine.preload(
        dir.path(),
        ScanOptions {
            glob_pattern: Some("{".to_owned()),
            ..ScanOptions::default()
        },
        false,
        |_| {
            called.store(true, Ordering::SeqCst);
            Ok(Vec::new())
        },
    );

    assert_malformed(result);
    assert!(!called.load(Ordering::SeqCst));
    assert_eq!(engine.entry_count().unwrap(), 0);
}

#[test]
fn directory_resolution_precedes_glob_compilation_and_propagates_io_errors() {
    let dir = TempDir::new().unwrap();
    let engine = engine();
    let file = write_file(&dir, "not-a-directory", b"x");
    let invalid_child = file.join("child");

    assert!(matches!(
        engine
            .query()
            .path_in_dir(&invalid_child, true)
            .path_glob("}")
            .run(),
        Err(LocalFileCacheError::Io(_))
    ));
    assert!(matches!(
        engine
            .query()
            .path_in_dir(&invalid_child, true)
            .path_glob("}")
            .dry_run(),
        Err(LocalFileCacheError::Io(_))
    ));
}

#[test]
fn deleted_sources_use_only_the_exact_stored_key() {
    let dir = TempDir::new().unwrap();
    let engine = engine();
    let a = dir.path().join("a");
    let b = dir.path().join("b");
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();
    let first = fs::canonicalize(write_file_in(&a, "report.txt")).unwrap();
    let second = fs::canonicalize(write_file_in(&b, "report.txt")).unwrap();
    engine.set(&first, &vec![1]).unwrap();
    engine.set(&second, &vec![2]).unwrap();
    fs::remove_file(&first).unwrap();
    fs::remove_file(&second).unwrap();

    assert_eq!(engine.get(&first).unwrap().unwrap().payload, vec![1]);
    assert!(engine.contains(&first).unwrap());
    let diagnosis = engine.explain(&first).unwrap();
    assert!(diagnosis.entry_exists);
    assert!(!diagnosis.file_exists);
    assert_eq!(diagnosis.status, CacheStatus::Missing);
    assert!(engine.get_if_fresh(&first).unwrap().is_none());
    assert_eq!(engine.check_status(&first).unwrap(), CacheStatus::Missing);
    assert!(!engine.touch(&first).unwrap());

    assert!(!engine.remove("report.txt").unwrap());
    assert_eq!(engine.entry_count().unwrap(), 2);
    assert!(engine.remove(&first).unwrap());
    assert!(!engine.contains(&first).unwrap());
    assert!(engine.contains(&second).unwrap());
}

#[test]
fn imported_portable_key_remains_an_exact_missing_source_key() {
    let dir = TempDir::new().unwrap();
    let source = engine();
    let path = write_file(&dir, "source.bin", b"x");
    source.set(&path, &vec![7]).unwrap();
    let mut record = source.export_entries().unwrap().remove(0);
    record.path = "portable/missing.bin".to_owned();

    let destination = engine();
    destination.import_entries(&[record]).unwrap();
    assert_eq!(
        destination
            .get("portable/missing.bin")
            .unwrap()
            .unwrap()
            .payload,
        vec![7]
    );
    assert!(destination.contains("portable/missing.bin").unwrap());
}

#[cfg(unix)]
#[test]
fn former_symlink_alias_does_not_guess_after_deletion() {
    use std::os::unix::fs::symlink;

    let dir = TempDir::new().unwrap();
    let engine = engine();
    let target = write_file(&dir, "target.bin", b"x");
    let canonical = target.canonicalize().unwrap();
    let alias = dir.path().join("alias.bin");
    symlink(&target, &alias).unwrap();
    engine.set(&alias, &vec![9]).unwrap();
    fs::remove_file(&alias).unwrap();
    fs::remove_file(&target).unwrap();

    assert!(engine.get(&alias).unwrap().is_none());
    assert!(engine.get(&canonical).unwrap().is_some());
}

#[cfg(unix)]
#[test]
fn real_non_utf8_paths_fail_without_database_mutation() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let dir = TempDir::new().unwrap();
    let engine = engine();
    let filename = OsString::from_vec(vec![b'n', b'o', b'n', 0xff]);
    let path = dir.path().join(filename);
    fs::write(&path, b"x").unwrap();

    assert!(matches!(
        engine.set(&path, &vec![1]),
        Err(LocalFileCacheError::InvalidPath { .. })
    ));
    assert_eq!(engine.entry_count().unwrap(), 0);
    assert!(matches!(
        engine.scan_dir_filtered(
            dir.path(),
            ScanOptions {
                glob_pattern: Some("*".to_owned()),
                ..ScanOptions::default()
            }
        ),
        Err(LocalFileCacheError::InvalidPath { .. })
    ));
    assert_eq!(engine.entry_count().unwrap(), 0);

    let missing = PathBuf::from(OsString::from_vec(vec![b'm', 0xff]));
    assert!(matches!(
        engine.get(&missing),
        Err(LocalFileCacheError::InvalidPath { .. })
    ));
    assert!(matches!(
        engine.contains(&missing),
        Err(LocalFileCacheError::InvalidPath { .. })
    ));
    assert!(matches!(
        engine.query().path_in_dir(&path, true).run(),
        Err(LocalFileCacheError::InvalidPath { .. })
    ));
    assert!(matches!(
        engine.query().path_in_dir(&path, true).dry_run(),
        Err(LocalFileCacheError::InvalidPath { .. })
    ));
}

#[cfg(unix)]
#[test]
fn non_utf8_filename_is_rejected_before_extension_exclusion() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let dir = TempDir::new().unwrap();
    let engine = engine();
    let filename = OsString::from_vec(vec![b'b', b'a', b'd', 0xff, b'.', b't', b'x', b't']);
    let path = dir.path().join(filename);
    fs::write(&path, b"x").unwrap();
    let options = ScanOptions {
        extensions: vec!["md".to_owned()],
        ..ScanOptions::default()
    };

    assert!(matches!(
        engine.scan_dir_filtered(dir.path(), options.clone()),
        Err(LocalFileCacheError::InvalidPath { path: rejected }) if rejected == path
    ));
    let factory_called = AtomicBool::new(false);
    assert!(matches!(
        engine.preload(dir.path(), options, false, |_| {
            factory_called.store(true, Ordering::SeqCst);
            Ok(Vec::new())
        }),
        Err(LocalFileCacheError::InvalidPath { path: rejected }) if rejected == path
    ));
    assert!(!factory_called.load(Ordering::SeqCst));
    assert_eq!(engine.entry_count().unwrap(), 0);
}

#[test]
fn sync_and_read_pools_share_terminal_glob_validation() {
    let pool = ConnectionPool::<Vec<u8>>::open(CacheOptions {
        database_path: ":memory:".into(),
        ..CacheOptions::default()
    })
    .unwrap();
    assert_malformed(pool.query_run(|query| query.path_glob("}")));
    assert_safety(pool.query_run(|query| query.path_glob("a".repeat(16_385))));

    let dir = TempDir::new().unwrap();
    let database = dir.path().join("pool.sqlite3");
    let writer = CacheEngine::<Vec<u8>>::open(CacheOptions {
        database_path: database.clone(),
        ..CacheOptions::default()
    })
    .unwrap();
    drop(writer);
    let read_pool = ReadPool::<Vec<u8>>::open(
        CacheOptions {
            database_path: database,
            ..CacheOptions::default()
        },
        1,
    )
    .unwrap();
    assert_malformed(read_pool.query_run(|query| query.path_glob("}")));
    assert_malformed(read_pool.query_dry_run(|query| query.path_glob("}")));
    assert_safety(read_pool.query_run(|query| query.path_glob("a".repeat(16_385))));
    assert_safety(read_pool.query_dry_run(|query| query.path_glob("a".repeat(16_385))));
}

#[test]
fn sync_pools_preserve_exact_deleted_key_outcomes() {
    let dir = TempDir::new().unwrap();
    let database = dir.path().join("deleted-pool.sqlite3");
    let source = write_file(&dir, "deleted-pool.bin", b"x")
        .canonicalize()
        .unwrap();
    {
        let writer = CacheEngine::<Vec<u8>>::open(CacheOptions {
            database_path: database.clone(),
            ..CacheOptions::default()
        })
        .unwrap();
        writer.set(&source, &vec![4]).unwrap();
    }
    fs::remove_file(&source).unwrap();

    let pool = ConnectionPool::<Vec<u8>>::open(CacheOptions {
        database_path: database.clone(),
        ..CacheOptions::default()
    })
    .unwrap();
    let read_pool = ReadPool::<Vec<u8>>::open(
        CacheOptions {
            database_path: database,
            ..CacheOptions::default()
        },
        1,
    )
    .unwrap();

    assert_eq!(pool.get(&source).unwrap().unwrap().payload, vec![4]);
    assert!(pool.get_if_fresh(&source).unwrap().is_none());
    assert_eq!(pool.check_status(&source).unwrap(), CacheStatus::Missing);
    assert_eq!(
        pool.batch_get(std::slice::from_ref(&source))[0]
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .payload,
        vec![4]
    );
    assert!(
        pool.batch_get_fresh(std::slice::from_ref(&source))[0]
            .as_ref()
            .unwrap()
            .is_none()
    );
    assert_eq!(
        *pool.check_status_batch(std::slice::from_ref(&source))[0]
            .as_ref()
            .unwrap(),
        CacheStatus::Missing
    );
    assert!(pool.explain(&source).unwrap().entry_exists);
    assert_eq!(read_pool.get(&source).unwrap().unwrap().payload, vec![4]);
    assert!(read_pool.get_if_fresh(&source).unwrap().is_none());
    assert_eq!(
        read_pool.check_status(&source).unwrap(),
        CacheStatus::Missing
    );
    assert!(read_pool.contains(&source).unwrap());
    assert_eq!(
        read_pool.batch_get(std::slice::from_ref(&source))[0]
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .payload,
        vec![4]
    );
    assert!(
        read_pool.batch_get_fresh(std::slice::from_ref(&source))[0]
            .as_ref()
            .unwrap()
            .is_none()
    );
    assert_eq!(
        *read_pool.check_status_batch(std::slice::from_ref(&source))[0]
            .as_ref()
            .unwrap(),
        CacheStatus::Missing
    );
    assert!(read_pool.explain(&source).unwrap().entry_exists);
    assert!(pool.remove(&source).unwrap());
}

#[cfg(any(feature = "async", feature = "async-std", feature = "smol"))]
async fn assert_async_deleted_key_outcomes(
    engine: &localcache::AsyncCacheEngine<Vec<u8>>,
    directory: &TempDir,
) {
    let source = write_file(directory, "deleted-async.bin", b"x")
        .canonicalize()
        .unwrap();
    engine.set(source.clone(), vec![5]).await.unwrap();
    fs::remove_file(&source).unwrap();
    assert_eq!(
        engine.get(source.clone()).await.unwrap().unwrap().payload,
        vec![5]
    );
    assert!(engine.get_if_fresh(source.clone()).await.unwrap().is_none());
    assert_eq!(
        engine.check_status(source.clone()).await.unwrap(),
        CacheStatus::Missing
    );
    assert!(engine.contains(source.clone()).await.unwrap());
    assert_eq!(
        engine.batch_get(vec![source.clone()]).await[0]
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .payload,
        vec![5]
    );
    assert!(
        engine.batch_get_fresh(vec![source.clone()]).await[0]
            .as_ref()
            .unwrap()
            .is_none()
    );
    assert_eq!(
        *engine.check_status_batch(vec![source.clone()]).await[0]
            .as_ref()
            .unwrap(),
        CacheStatus::Missing
    );
    assert!(engine.explain(source.clone()).await.unwrap().entry_exists);
    assert!(!engine.touch(source.clone()).await.unwrap());
    assert!(engine.remove(source).await.unwrap());
}

#[cfg(feature = "async")]
#[tokio::test]
async fn tokio_wrapper_shares_terminal_glob_validation() {
    let directory = TempDir::new().unwrap();
    let engine = localcache::AsyncCacheEngine::<Vec<u8>>::open(CacheOptions {
        database_path: ":memory:".into(),
        ..CacheOptions::default()
    })
    .await
    .unwrap();
    assert_malformed(
        engine
            .query_run::<_, Vec<u8>>(|query| query.path_glob("}"))
            .await,
    );
    assert_malformed(engine.query_dry_run(|query| query.path_glob("}")).await);
    assert_safety(
        engine
            .query_run::<_, Vec<u8>>(|query| query.path_glob("a".repeat(16_385)))
            .await,
    );
    assert_async_deleted_key_outcomes(&engine, &directory).await;
}

#[cfg(all(not(feature = "async"), feature = "async-std"))]
#[test]
fn async_std_wrapper_shares_terminal_glob_validation() {
    async_std::task::block_on(async {
        let directory = TempDir::new().unwrap();
        let engine = localcache::AsyncCacheEngine::<Vec<u8>>::open(CacheOptions {
            database_path: ":memory:".into(),
            ..CacheOptions::default()
        })
        .await
        .unwrap();
        assert_malformed(
            engine
                .query_run::<_, Vec<u8>>(|query| query.path_glob("}"))
                .await,
        );
        assert_malformed(engine.query_dry_run(|query| query.path_glob("}")).await);
        assert_safety(
            engine
                .query_run::<_, Vec<u8>>(|query| query.path_glob("a".repeat(16_385)))
                .await,
        );
        assert_async_deleted_key_outcomes(&engine, &directory).await;
    });
}

#[cfg(all(not(feature = "async"), not(feature = "async-std"), feature = "smol"))]
#[test]
fn smol_wrapper_shares_terminal_glob_validation() {
    smol::block_on(async {
        let directory = TempDir::new().unwrap();
        let engine = localcache::AsyncCacheEngine::<Vec<u8>>::open(CacheOptions {
            database_path: ":memory:".into(),
            ..CacheOptions::default()
        })
        .await
        .unwrap();
        assert_malformed(
            engine
                .query_run::<_, Vec<u8>>(|query| query.path_glob("}"))
                .await,
        );
        assert_malformed(engine.query_dry_run(|query| query.path_glob("}")).await);
        assert_safety(
            engine
                .query_run::<_, Vec<u8>>(|query| query.path_glob("a".repeat(16_385)))
                .await,
        );
        assert_async_deleted_key_outcomes(&engine, &directory).await;
    });
}

fn write_file_in(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, b"x").unwrap();
    path
}
