//! RFC 029 (v0.24.0): with a key, an entry is readable only if it is sealed
//! under that key and bound to its row. Every test here goes through the
//! public API, plus SQL on the database file to play the part of someone who
//! can write it. The tests of the attacks fail on the v0.23.1 tree.

#![cfg(feature = "encryption")]

mod common;

use std::path::{Path, PathBuf};

use localcache::{CacheEngine, LocalFileCacheError};
use rusqlite::Connection;
use tempfile::TempDir;

use common::write_file;

const KEY: [u8; 32] = [0x5c; 32];

struct Fixture {
    _dir: TempDir,
    db: PathBuf,
    a: PathBuf,
    b: PathBuf,
}

impl Fixture {
    /// A database with two entries under one key, sealed as the current code seals.
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("c.sqlite3");
        let a = write_file(&dir, "a.bin", b"a");
        let b = write_file(&dir, "b.bin", b"b");
        let engine = keyed(&db, KEY);
        engine.set(&a, &vec![1_u8, 1, 1]).unwrap();
        engine.set(&b, &vec![2_u8, 2, 2]).unwrap();
        Self {
            _dir: dir,
            db,
            a,
            b,
        }
    }

    fn engine(&self) -> CacheEngine<Vec<u8>> {
        keyed(&self.db, KEY)
    }

    fn sql(&self, sql: &str) {
        let conn = Connection::open(&self.db).unwrap();
        conn.execute_batch(sql).unwrap();
    }

    fn tag_of(&self, path: &Path) -> String {
        let conn = Connection::open(&self.db).unwrap();
        conn.query_row(
            "SELECT p.encoding FROM payloads p JOIN files f ON f.id = p.file_id WHERE f.path = ?1",
            [canonical(path)],
            |r| r.get(0),
        )
        .unwrap()
    }
}

fn canonical(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned()
}

fn keyed(db: &Path, key: [u8; 32]) -> CacheEngine<Vec<u8>> {
    CacheEngine::builder()
        .database(db)
        .encryption_key(key.to_vec())
        .build()
        .unwrap()
}

/// Motivation 1 (swap): one `UPDATE` copies the first ciphertext, and its tag,
/// onto the second row. `get` of the second must not return the first's secret.
#[test]
fn a_ciphertext_copied_onto_another_row_does_not_open() {
    let f = Fixture::new();
    let (pa, pb) = (canonical(&f.a), canonical(&f.b));
    f.sql(&format!(
        "UPDATE payloads SET \
           content  = (SELECT p.content  FROM payloads p JOIN files f ON f.id = p.file_id WHERE f.path = '{pa}'), \
           encoding = (SELECT p.encoding FROM payloads p JOIN files f ON f.id = p.file_id WHERE f.path = '{pa}') \
         WHERE file_id = (SELECT id FROM files WHERE path = '{pb}')"
    ));
    let result = f.engine().get(&f.b);
    assert!(
        matches!(result, Err(LocalFileCacheError::EncryptionError(_))),
        "the second entry returned the first's payload or failed otherwise: {result:?}"
    );
    // the first entry is untouched
    assert_eq!(
        f.engine().get(&f.a).unwrap().unwrap().payload,
        vec![1_u8, 1, 1]
    );
}

/// Motivation 3: the stored tag tells the reader how to decode the bytes after
/// decryption; it is authenticated, so an edited tag fails to open as an
/// `EncryptionError`, not as a decoding error after a successful decryption.
#[cfg(feature = "compression")]
#[test]
fn an_edited_tag_does_not_open() {
    let f = Fixture::new();
    let tag = f.tag_of(&f.a);
    assert!(tag.starts_with("raw-aes256gcm"), "{tag}");
    let edited = tag.replacen("raw", "zstd", 1);
    f.sql(&format!(
        "UPDATE payloads SET encoding = '{edited}' \
         WHERE file_id = (SELECT id FROM files WHERE path = '{}')",
        canonical(&f.a)
    ));
    let result = f.engine().get(&f.a);
    assert!(
        matches!(result, Err(LocalFileCacheError::EncryptionError(_))),
        "{result:?}"
    );
}

/// Entries written by this build are bound, and still round-trip.
#[test]
fn entries_written_with_a_key_carry_the_bound_tag_and_round_trip() {
    let f = Fixture::new();
    assert!(
        f.tag_of(&f.a).ends_with("-aes256gcm-bound"),
        "{}",
        f.tag_of(&f.a)
    );
    let e = f.engine();
    assert_eq!(e.get(&f.a).unwrap().unwrap().payload, vec![1_u8, 1, 1]);
    assert_eq!(e.get(&f.b).unwrap().unwrap().payload, vec![2_u8, 2, 2]);
}
