//! RFC 029: the upgrade of a database written by the published v0.23.1 with an
//! encryption key. The input is the committed fixture
//! `tests/fixtures/compat-v0_23-encrypted.sqlite3` (see `tests/fixtures/README.md`
//! for how it was made); it is checked against its digest before it is copied,
//! and every test works on the copy.

#![cfg(all(feature = "encryption", feature = "compression", feature = "json"))]

use std::fs;
use std::path::{Path, PathBuf};

use localcache::{CacheEngine, LocalFileCacheError};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/compat-v0_23-encrypted.sqlite3"
);
const FIXTURE_SHA256: &str = "05dc89a9618a2ca28a6b652055e323418e8371cbe0b21fbe36a2bfcf14f63867";

/// The fixture's key: `0x40 + i`.
fn key() -> Vec<u8> {
    (0..32).map(|i| 0x40 + i as u8).collect()
}

/// (namespace, index, the tag v0.23.1 wrote)
const NAMESPACES: [(&str, usize, &str); 5] = [
    ("raw", 0, "raw-aes256gcm"),
    ("zstd", 1, "zstd-aes256gcm"),
    ("json", 2, "json-aes256gcm"),
    ("json-zstd", 3, "json-zstd-aes256gcm"),
    ("plain", 4, "raw"),
];

/// The generator's payload.
fn payload(namespace_index: usize, entry_index: usize) -> Vec<u8> {
    (0..200)
        .map(|j| ((namespace_index * 61 + entry_index * 13 + j * 3) % 251) as u8)
        .collect()
}

fn stored_path(namespace: &str, entry_index: usize) -> PathBuf {
    PathBuf::from(format!("/fixture/input-{namespace}-{entry_index}.bin"))
}

struct Copy {
    _dir: TempDir,
    db: PathBuf,
}

fn copy_of_the_fixture() -> Copy {
    let bytes = fs::read(FIXTURE).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        FIXTURE_SHA256,
        "immutable fixture digest changed"
    );
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("compat-v0_23-encrypted.sqlite3");
    fs::write(&db, bytes).unwrap();
    Copy { _dir: dir, db }
}

fn engine(db: &Path, namespace: &str, key: Option<Vec<u8>>) -> CacheEngine<Vec<u8>> {
    let mut builder = CacheEngine::<Vec<u8>>::builder()
        .database(db)
        .namespace(namespace);
    if let Some(key) = key {
        builder = builder.encryption_key(key);
    }
    builder.build().unwrap()
}

fn tags(db: &Path, namespace: &str) -> Vec<String> {
    engine(db, namespace, None)
        .list_entries()
        .unwrap()
        .into_iter()
        .map(|e| e.encoding)
        .collect()
}

#[test]
fn a_keyed_engine_refuses_every_entry_of_the_v0_23_database() {
    let copy = copy_of_the_fixture();
    for (namespace, _, tag) in NAMESPACES {
        let keyed = engine(&copy.db, namespace, Some(key()));
        for i in 0..3 {
            let result = keyed.get(stored_path(namespace, i));
            assert!(
                matches!(result, Err(LocalFileCacheError::UnsealedEntry)),
                "{namespace}/{i} ({tag}): {result:?}"
            );
        }
        let report = keyed.query().run_report().unwrap();
        assert!(report.entries.is_empty(), "{namespace}");
        assert_eq!(report.skipped.len(), 3, "{namespace}");
        // and nothing was changed by looking
        assert!(
            tags(&copy.db, namespace).iter().all(|t| t == tag),
            "{namespace}"
        );
    }
}

#[test]
fn a_keyless_engine_reads_the_plain_namespace_and_names_the_missing_key_for_the_rest() {
    let copy = copy_of_the_fixture();
    for (namespace, index, tag) in NAMESPACES {
        let keyless = engine(&copy.db, namespace, None);
        for i in 0..3 {
            let result = keyless.get(stored_path(namespace, i));
            if tag == "raw" {
                assert_eq!(
                    result.unwrap().unwrap().payload,
                    payload(index, i),
                    "{namespace}/{i}"
                );
            } else {
                assert!(
                    matches!(result, Err(LocalFileCacheError::MissingEncryptionKey)),
                    "{namespace}/{i}: {result:?}"
                );
            }
        }
    }
}

#[test]
fn one_rotation_with_the_same_key_brings_every_namespace_forward() {
    let copy = copy_of_the_fixture();
    for (namespace, index, _) in NAMESPACES {
        let keyed = engine(&copy.db, namespace, Some(key()));
        assert_eq!(
            keyed.rotate_encryption_key(&key()).unwrap(),
            3,
            "{namespace}"
        );
        for i in 0..3 {
            let got = keyed.get(stored_path(namespace, i)).unwrap().unwrap();
            assert_eq!(got.payload, payload(index, i), "{namespace}/{i}");
        }
        assert!(
            tags(&copy.db, namespace)
                .iter()
                .all(|t| t.ends_with("-aes256gcm-bound")),
            "{namespace}: {:?}",
            tags(&copy.db, namespace)
        );
    }
    // the plain namespace is sealed now: a keyless engine no longer reads it
    assert!(matches!(
        engine(&copy.db, "plain", None).get(stored_path("plain", 0)),
        Err(LocalFileCacheError::MissingEncryptionKey)
    ));
    // a reopened engine with the key reads everything
    for (namespace, index, _) in NAMESPACES {
        let keyed = engine(&copy.db, namespace, Some(key()));
        assert_eq!(
            keyed
                .get(stored_path(namespace, 2))
                .unwrap()
                .unwrap()
                .payload,
            payload(index, 2)
        );
        assert!(keyed.query().run().unwrap().len() == 3);
    }
}

#[test]
fn a_second_rotation_to_a_new_key_works_and_the_old_key_no_longer_reads() {
    let copy = copy_of_the_fixture();
    let new_key = vec![0xee_u8; 32];
    for (namespace, index, _) in NAMESPACES {
        let keyed = engine(&copy.db, namespace, Some(key()));
        assert_eq!(keyed.rotate_encryption_key(&key()).unwrap(), 3);
        assert_eq!(keyed.rotate_encryption_key(&new_key).unwrap(), 3);
        for i in 0..3 {
            assert_eq!(
                keyed
                    .get(stored_path(namespace, i))
                    .unwrap()
                    .unwrap()
                    .payload,
                payload(index, i)
            );
        }
        let old = engine(&copy.db, namespace, Some(key()));
        assert!(matches!(
            old.get(stored_path(namespace, 0)),
            Err(LocalFileCacheError::EncryptionError(_))
        ));
    }
}

/// A rotation that is cut short leaves the v0.23 database exactly as it was.
#[test]
fn a_rotation_with_the_wrong_key_changes_nothing() {
    let copy = copy_of_the_fixture();
    let before = fs::read(&copy.db).unwrap();
    let wrong = engine(&copy.db, "raw", Some(vec![0x99; 32]));
    assert!(matches!(
        wrong.rotate_encryption_key(&key()),
        Err(LocalFileCacheError::EncryptionError(_))
    ));
    drop(wrong);
    assert_eq!(tags(&copy.db, "raw"), ["raw-aes256gcm"; 3]);
    assert_eq!(
        fs::read(&copy.db).unwrap(),
        before,
        "the database file changed"
    );
}
