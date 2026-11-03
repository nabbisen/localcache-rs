//! RFC 029 R3: `rotate_encryption_key` seals every entry of the namespace
//! bound to its path, in pages by path, in one `IMMEDIATE` transaction, and
//! leaves the database exactly as it was when anything fails.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use tempfile::TempDir;

use super::*;
use crate::CacheEngine;
use crate::serialization::{seal_without_binding_for_tests, serialize_bincode};

const OLD: [u8; 32] = [0x21; 32];
const NEW: [u8; 32] = [0x22; 32];

struct HookGuard;

impl Drop for HookGuard {
    fn drop(&mut self) {
        TEST_HOOK.with(|slot| *slot.borrow_mut() = None);
    }
}

fn set_hook(hook: impl FnMut(TestPoint) -> Result<(), LocalFileCacheError> + 'static) -> HookGuard {
    TEST_HOOK.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
    HookGuard
}

/// Shrink the rotation's page to `rows` rows for the life of the guard.
struct PageGuard;

impl Drop for PageGuard {
    fn drop(&mut self) {
        ROTATION_PAGE_ROWS_OVERRIDE.with(|c| c.set(None));
    }
}

fn pages_of(rows: usize) -> PageGuard {
    ROTATION_PAGE_ROWS_OVERRIDE.with(|c| c.set(Some(rows)));
    PageGuard
}

struct Namespace {
    dir: TempDir,
    db: PathBuf,
}

impl Namespace {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("c.sqlite3");
        Self { dir, db }
    }

    fn file(&self, name: &str) -> PathBuf {
        let path = self.dir.path().join(name);
        std::fs::write(&path, name).unwrap();
        path
    }

    fn keyed(&self, key: [u8; 32]) -> CacheEngine<Vec<u8>> {
        CacheEngine::builder()
            .database(&self.db)
            .encryption_key(key.to_vec())
            .build()
            .unwrap()
    }

    fn keyless(&self) -> CacheEngine<Vec<u8>> {
        CacheEngine::builder().database(&self.db).build().unwrap()
    }
}

fn payload(i: usize) -> Vec<u8> {
    (0..40).map(|j| (i * 7 + j) as u8).collect()
}

/// Every row of `payloads`: `(file_id, tag, content)`.
fn snapshot(engine: &CacheEngine<Vec<u8>>) -> Vec<(i64, String, Vec<u8>)> {
    let mut stmt = engine
        .conn
        .prepare("SELECT file_id, encoding, content FROM payloads ORDER BY file_id")
        .unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

/// Entries `0..n` in one namespace, in all three states in turn: bound (written
/// under `OLD`), plain (written without a key), sealed without binding (as
/// v0.23 sealed them). Returns the paths.
fn mixed_namespace(ns: &Namespace, n: usize) -> Vec<PathBuf> {
    let keyed = ns.keyed(OLD);
    let keyless = ns.keyless();
    let paths: Vec<PathBuf> = (0..n)
        .map(|i| ns.file(&format!("entry-{i:04}.bin")))
        .collect();
    for (i, path) in paths.iter().enumerate() {
        match i % 3 {
            0 => keyed.set(path, &payload(i)).unwrap(),
            1 => keyless.set(path, &payload(i)).unwrap(),
            _ => {
                keyless.set(path, &payload(i)).unwrap();
                let sealed =
                    seal_without_binding_for_tests(&serialize_bincode(&payload(i)).unwrap(), &OLD);
                keyless
                    .conn
                    .execute(
                        "UPDATE payloads SET content = ?1, encoding = 'raw-aes256gcm' \
                         WHERE file_id = (SELECT id FROM files WHERE path = ?2)",
                        rusqlite::params![
                            sealed,
                            std::fs::canonicalize(path).unwrap().to_str().unwrap()
                        ],
                    )
                    .unwrap();
            }
        }
    }
    paths
}

fn assert_all_bound_and_readable(ns: &Namespace, key: [u8; 32], paths: &[PathBuf]) {
    let engine = ns.keyed(key);
    for (i, path) in paths.iter().enumerate() {
        assert_eq!(
            engine.get(path).unwrap().unwrap().payload,
            payload(i),
            "entry {i}"
        );
    }
    for (_, tag, _) in snapshot(&engine) {
        assert_eq!(tag, "raw-aes256gcm-bound");
    }
}

/// Plain, sealed-without-binding and bound rows all end bound and read, for
/// several page sizes (one row, a size that divides the count, one that does
/// not, one larger than the namespace).
#[test]
fn every_state_ends_bound_and_readable_whatever_the_page_size() {
    for rows in [1, 2, 3, 4, 100] {
        let ns = Namespace::new();
        let paths = mixed_namespace(&ns, 7);
        let _pages = pages_of(rows);
        let engine = ns.keyed(OLD);
        assert_eq!(
            engine.rotate_encryption_key(&NEW).unwrap(),
            7,
            "page of {rows}"
        );
        drop(engine);
        assert_all_bound_and_readable(&ns, NEW, &paths);
    }
}

/// The same key is allowed: that is the upgrade of a v0.23 database.
#[test]
fn rotating_to_the_same_key_is_the_upgrade_and_a_second_rotation_works() {
    let ns = Namespace::new();
    let paths = mixed_namespace(&ns, 6);
    let engine = ns.keyed(OLD);
    assert_eq!(engine.rotate_encryption_key(&OLD).unwrap(), 6);
    drop(engine);
    assert_all_bound_and_readable(&ns, OLD, &paths);
    let engine = ns.keyed(OLD);
    assert_eq!(engine.rotate_encryption_key(&NEW).unwrap(), 6);
    drop(engine);
    assert_all_bound_and_readable(&ns, NEW, &paths);
    // a keyless engine now meets sealed tags
    assert!(matches!(
        ns.keyless().get(&paths[0]),
        Err(LocalFileCacheError::MissingEncryptionKey)
    ));
}

/// The page boundary at exactly the real `ROTATION_PAGE_ROWS`: 1000, 1001 and
/// 2000 rows (imported as plain records, which is how a database written
/// without a key is brought forward).
#[test]
fn the_real_page_size_has_no_boundary_error() {
    use crate::ExportRecord;
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    for n in [
        ROTATION_PAGE_ROWS - 1,
        ROTATION_PAGE_ROWS,
        ROTATION_PAGE_ROWS + 1,
        2 * ROTATION_PAGE_ROWS,
    ] {
        let ns = Namespace::new();
        let records: Vec<ExportRecord> = (0..n)
            .map(|i| ExportRecord {
                path: format!("/imported/{i:05}"),
                payload_b64: STANDARD.encode(serialize_bincode(&payload(i)).unwrap()),
                encoding: "raw".to_owned(),
                mtime: 1,
                file_size: 1,
                hash: None,
                payload_version: 0,
                updated_at: 1,
                last_accessed_at: 1,
            })
            .collect();
        ns.keyless().import_entries(&records).unwrap();
        let engine = ns.keyed(OLD);
        assert_eq!(engine.rotate_encryption_key(&NEW).unwrap(), n, "n = {n}");
        drop(engine);
        let engine = ns.keyed(NEW);
        for i in [0, n / 2, n - 1] {
            let got = engine.get(format!("/imported/{i:05}")).unwrap().unwrap();
            assert_eq!(got.payload, payload(i), "n = {n}, entry {i}");
        }
        assert!(
            snapshot(&engine)
                .iter()
                .all(|(_, t, _)| t == "raw-aes256gcm-bound")
        );
    }
}

/// An empty namespace rotates to 0 and the engine still switches key.
#[test]
fn an_empty_namespace_rotates_to_zero_and_still_switches_key() {
    let ns = Namespace::new();
    let engine = ns.keyed(OLD);
    assert_eq!(engine.rotate_encryption_key(&NEW).unwrap(), 0);
    let p = ns.file("after.bin");
    engine.set(&p, &payload(1)).unwrap();
    drop(engine);
    assert_eq!(ns.keyed(NEW).get(&p).unwrap().unwrap().payload, payload(1));
}

/// A failure after a page, in the middle of a page, or at the commit leaves the
/// database as it was and the engine on its old key.
#[test]
fn a_failure_anywhere_leaves_the_database_byte_identical_and_the_old_key() {
    for point in [
        TestPoint::AfterLoad,
        TestPoint::MidPage,
        TestPoint::AfterPage,
        TestPoint::BeforeCommit,
    ] {
        for rows in [1, 2] {
            let ns = Namespace::new();
            let paths = mixed_namespace(&ns, 6);
            let _pages = pages_of(rows);
            let engine = ns.keyed(OLD);
            let before = snapshot(&engine);

            // inject on the second time the point is reached (the first for the
            // points that happen once), so some pages are already written
            let seen = Rc::new(Cell::new(0_usize));
            let seen_in_hook = Rc::clone(&seen);
            let hook = set_hook(move |p| {
                if p == point {
                    seen_in_hook.set(seen_in_hook.get() + 1);
                    let nth = if matches!(point, TestPoint::MidPage | TestPoint::AfterPage) {
                        2
                    } else {
                        1
                    };
                    if seen_in_hook.get() == nth {
                        return Err(LocalFileCacheError::DatabaseState("injected".into()));
                    }
                }
                Ok(())
            });
            let error = engine.rotate_encryption_key(&NEW).unwrap_err();
            drop(hook);
            assert!(
                matches!(error, LocalFileCacheError::DatabaseState(ref m) if m == "injected"),
                "{point:?}, page of {rows}: {error:?}"
            );
            assert!(seen.get() >= 1, "{point:?} was never reached");

            assert_eq!(
                snapshot(&engine),
                before,
                "{point:?}, page of {rows}: the database changed"
            );
            // the engine keeps reading and writing with the old key
            for (i, path) in paths.iter().enumerate() {
                let got = engine.get(path);
                match i % 3 {
                    0 => assert_eq!(got.unwrap().unwrap().payload, payload(i)),
                    _ => assert!(
                        matches!(got, Err(LocalFileCacheError::UnsealedEntry)),
                        "entry {i}"
                    ),
                }
            }
            let fresh = ns.file("written-after.bin");
            engine.set(&fresh, &payload(99)).unwrap();
            drop(engine);
            // and a reader with the new key cannot open what it wrote
            assert!(ns.keyed(NEW).get(&fresh).is_err());
            assert_eq!(
                ns.keyed(OLD).get(&fresh).unwrap().unwrap().payload,
                payload(99)
            );
        }
    }
}

/// An unknown tag, or a wrong old key, aborts the whole rotation.
#[test]
fn an_unknown_tag_or_a_wrong_key_aborts_and_changes_nothing() {
    let ns = Namespace::new();
    let paths = mixed_namespace(&ns, 6);
    let engine = ns.keyed(OLD);
    let before = snapshot(&engine);

    // the last row (in path order) gets a tag nothing can decode
    engine
        .conn
        .execute(
            "UPDATE payloads SET encoding = 'bogus-aes256gcm' \
             WHERE file_id = (SELECT id FROM files ORDER BY path DESC LIMIT 1)",
            [],
        )
        .unwrap();
    let damaged = snapshot(&engine);
    let _pages = pages_of(2);
    assert!(matches!(
        engine.rotate_encryption_key(&NEW),
        Err(LocalFileCacheError::UnknownEncoding(_))
    ));
    assert_eq!(snapshot(&engine), damaged);
    assert_ne!(damaged, before);

    // an engine whose key is not the one the entries were sealed under
    let ns2 = Namespace::new();
    let paths2 = mixed_namespace(&ns2, 6);
    let wrong = ns2.keyed([0x99; 32]);
    let before2 = snapshot(&wrong);
    assert!(matches!(
        wrong.rotate_encryption_key(&NEW),
        Err(LocalFileCacheError::EncryptionError(_))
    ));
    assert_eq!(snapshot(&wrong), before2);
    let _ = (paths, paths2);
}

/// Rows of another namespace are left alone.
#[test]
fn another_namespace_is_not_touched() {
    let ns = Namespace::new();
    let mine = ns.file("mine.bin");
    let theirs = ns.file("theirs.bin");
    let engine = ns.keyed(OLD);
    engine.set(&mine, &payload(1)).unwrap();
    let other: CacheEngine<Vec<u8>> = CacheEngine::builder()
        .database(&ns.db)
        .namespace("other")
        .encryption_key(OLD.to_vec())
        .build()
        .unwrap();
    other.set(&theirs, &payload(2)).unwrap();
    let theirs_before = snapshot(&other);
    assert_eq!(engine.rotate_encryption_key(&NEW).unwrap(), 1);
    assert_eq!(
        snapshot(&other)
            .into_iter()
            .filter(|(_, tag, _)| tag.ends_with("-bound"))
            .count(),
        2
    );
    // the other namespace's row is byte-identical to before
    let after = snapshot(&other);
    let their_row = |s: &[(i64, String, Vec<u8>)]| s.iter().find(|r| r.0 == 2).cloned();
    assert_eq!(their_row(&after), their_row(&theirs_before));
    assert_eq!(other.get(&theirs).unwrap().unwrap().payload, payload(2));
}
