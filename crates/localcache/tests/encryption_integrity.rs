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

// ---------------------------------------------------------------------------
// R2: with a key, only bound entries are read
// ---------------------------------------------------------------------------

/// A keyless engine, on the same database and namespace, stores a plain entry.
fn write_plain(f: &Fixture, name: &str, payload: Vec<u8>) -> PathBuf {
    let source = f._dir.path().join(name);
    std::fs::write(&source, name).unwrap();
    let plain: CacheEngine<Vec<u8>> = CacheEngine::builder().database(&f.db).build().unwrap();
    plain.set(&source, &payload).unwrap();
    source
}

/// Motivation 2 (downgrade): an entry written by an engine with no key must
/// not be returned, as `Ok`, by an engine configured with a key.
#[test]
fn a_plain_entry_is_refused_by_a_keyed_engine() {
    let f = Fixture::new();
    let plain = write_plain(&f, "plain.bin", vec![9, 9, 9]);
    let e = f.engine();
    assert!(matches!(
        e.get(&plain),
        Err(LocalFileCacheError::UnsealedEntry)
    ));
    assert!(matches!(
        e.get_if_fresh(&plain),
        Err(LocalFileCacheError::UnsealedEntry)
    ));
    // the bound entries next to it still read
    assert_eq!(e.get(&f.a).unwrap().unwrap().payload, vec![1_u8, 1, 1]);
    // the batch reads report it per path and leave the others alone
    let batch = e.batch_get(&[&f.a, &plain, &f.b]);
    assert!(batch[0].as_ref().unwrap().is_some());
    assert!(matches!(batch[1], Err(LocalFileCacheError::UnsealedEntry)));
    assert!(batch[2].as_ref().unwrap().is_some());
    let fresh = e.batch_get_fresh(&[&plain]);
    assert!(matches!(fresh[0], Err(LocalFileCacheError::UnsealedEntry)));
}

/// A query: `run` stops at the first refusal, `run_report` lists it and still
/// returns the entries it could read.
#[test]
fn queries_report_an_unsealed_entry_and_run_fails_on_it() {
    let f = Fixture::new();
    let plain = write_plain(&f, "plain.bin", vec![9, 9, 9]);
    let e = f.engine();
    assert!(matches!(
        e.query().run(),
        Err(LocalFileCacheError::UnsealedEntry)
    ));
    let report = e.query().run_report().unwrap();
    assert_eq!(report.entries.len(), 2);
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(
        report.skipped[0].path,
        std::fs::canonicalize(&plain).unwrap()
    );
    assert!(matches!(
        report.skipped[0].error,
        LocalFileCacheError::UnsealedEntry
    ));
}

/// Editing a bound entry's tag to a plain or an unbound one is a downgrade
/// attempt: refused as `UnsealedEntry`, before anything is decrypted.
#[test]
fn a_bound_tag_edited_down_to_unbound_or_plain_is_refused() {
    let f = Fixture::new();
    let pa = canonical(&f.a);
    for edited in ["raw-aes256gcm", "raw"] {
        f.sql(&format!(
            "UPDATE payloads SET encoding = '{edited}' \
             WHERE file_id = (SELECT id FROM files WHERE path = '{pa}')"
        ));
        let result = f.engine().get(&f.a);
        assert!(
            matches!(result, Err(LocalFileCacheError::UnsealedEntry)),
            "{edited}: {result:?}"
        );
    }
}

/// Without a key the behaviour is today's: a sealed tag is `MissingEncryptionKey`
/// and a plain tag is read.
#[test]
fn an_engine_without_a_key_reads_plain_and_names_a_missing_key_for_sealed() {
    let f = Fixture::new();
    let plain = write_plain(&f, "plain.bin", vec![9, 9, 9]);
    let keyless: CacheEngine<Vec<u8>> = CacheEngine::builder().database(&f.db).build().unwrap();
    assert!(matches!(
        keyless.get(&f.a),
        Err(LocalFileCacheError::MissingEncryptionKey)
    ));
    assert_eq!(
        keyless.get(&plain).unwrap().unwrap().payload,
        vec![9_u8, 9, 9]
    );
}

#[test]
fn the_refusals_name_their_remedy_and_the_decryption_text_mentions_a_moved_entry() {
    let text = LocalFileCacheError::UnsealedEntry.to_string();
    assert!(
        text.contains("rotate_encryption_key") && text.contains("purge"),
        "{text}"
    );
    let f = Fixture::new();
    let wrong: CacheEngine<Vec<u8>> = keyed(&f.db, [0x77; 32]);
    let error = wrong.get(&f.a).unwrap_err().to_string();
    assert!(
        error.contains("wrong key, corrupted data, or the entry was moved"),
        "{error}"
    );
}

/// Step 0 C3: a keyed engine never filters payloads in SQL. Over plain `json`
/// entries it takes the decoding tier and is refused; an engine without a key
/// still takes the SQL tier.
#[cfg(feature = "json")]
mod pushdown {
    use super::*;
    use localcache::Codec;

    fn open(db: &Path, key: Option<[u8; 32]>) -> CacheEngine<serde_json::Value> {
        let mut b = CacheEngine::<serde_json::Value>::builder()
            .database(db)
            .codec(Codec::Json);
        if let Some(key) = key {
            b = b.encryption_key(key.to_vec());
        }
        b.build().unwrap()
    }

    #[test]
    fn a_keyed_engine_never_uses_the_sql_tier_and_refuses_plain_json_entries() {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("c.sqlite3");
        let keyless = open(&db, None);
        for (i, name) in ["a", "b", "c"].iter().enumerate() {
            let p = write_file(&dir, name, name.as_bytes());
            keyless
                .set(&p, &serde_json::json!({ "score": i as f64 * 10.0 }))
                .unwrap();
        }
        // an engine without a key: SQL tier, results
        let plan = keyless.query().field_gt("score", 5.0).dry_run().unwrap();
        assert!(plan.contains("tier 2"), "{plan}");
        assert_eq!(
            keyless.query().field_gt("score", 5.0).run().unwrap().len(),
            2
        );

        // the same namespace through an engine with a key: decoding tier, refused
        let keyed = open(&db, Some(KEY));
        let plan = keyed.query().field_gt("score", 5.0).dry_run().unwrap();
        assert!(plan.contains("tier 3"), "{plan}");
        assert!(matches!(
            keyed.query().field_gt("score", 5.0).run(),
            Err(LocalFileCacheError::UnsealedEntry)
        ));
        let report = keyed.query().field_gt("score", 5.0).run_report().unwrap();
        assert!(report.entries.is_empty());
        assert_eq!(report.skipped.len(), 3);
    }

    #[test]
    fn a_keyed_engine_over_bound_json_entries_filters_after_decrypting() {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("c.sqlite3");
        let keyed = open(&db, Some(KEY));
        for (i, name) in ["a", "b", "c"].iter().enumerate() {
            let p = write_file(&dir, name, name.as_bytes());
            keyed
                .set(&p, &serde_json::json!({ "score": i as f64 * 10.0 }))
                .unwrap();
        }
        let plan = keyed.query().field_gt("score", 5.0).dry_run().unwrap();
        assert!(plan.contains("tier 3"), "{plan}");
        assert_eq!(keyed.query().field_gt("score", 5.0).run().unwrap().len(), 2);
    }
}
