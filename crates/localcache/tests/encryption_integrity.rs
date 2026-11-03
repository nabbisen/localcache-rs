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

// ---------------------------------------------------------------------------
// R2, import: a keyed import accepts only bound records
// ---------------------------------------------------------------------------

/// Records of three kinds, from one database: bound (sealed by the keyed
/// engine), plain (from a keyless engine), and a bound one with its tag edited
/// to the old suffix (sealed without binding, as v0.23 sealed them).
fn records(f: &Fixture) -> (Vec<localcache::ExportRecord>, Vec<localcache::ExportRecord>) {
    let bound = f.engine().export_entries().unwrap();
    assert!(
        bound
            .iter()
            .all(|r| r.encoding.ends_with("-aes256gcm-bound"))
    );
    let plain_path = write_plain(f, "plain.bin", vec![9, 9, 9]);
    let plain: Vec<_> = f
        .engine_namespace_plain_export()
        .into_iter()
        .filter(|r| r.path == canonical(&plain_path))
        .collect();
    assert_eq!(plain.len(), 1);
    (bound, plain)
}

impl Fixture {
    fn engine_namespace_plain_export(&self) -> Vec<localcache::ExportRecord> {
        let keyless: CacheEngine<Vec<u8>> =
            CacheEngine::builder().database(&self.db).build().unwrap();
        keyless.export_entries().unwrap()
    }

    fn target(&self, namespace: &str, key: Option<[u8; 32]>) -> CacheEngine<Vec<u8>> {
        let mut b = CacheEngine::<Vec<u8>>::builder()
            .database(&self.db)
            .namespace(namespace);
        if let Some(key) = key {
            b = b.encryption_key(key.to_vec());
        }
        b.build().unwrap()
    }
}

fn records_refusal(result: Result<usize, LocalFileCacheError>) -> String {
    match result {
        Err(LocalFileCacheError::InvalidArgument { argument, reason }) => {
            assert_eq!(argument, "records");
            reason
        }
        other => panic!("expected InvalidArgument records, got {other:?}"),
    }
}

#[test]
fn a_keyed_import_of_a_plain_record_is_refused_whole_before_writing() {
    let f = Fixture::new();
    let (bound, plain) = records(&f);
    let target = f.target("copy", Some(KEY));
    let mixed: Vec<_> = bound.iter().chain(plain.iter()).cloned().collect();
    let reason = records_refusal(target.import_entries(&mixed));
    assert!(reason.contains("1 of 3 records"), "{reason}");
    assert!(reason.contains("rotate_encryption_key"), "{reason}");
    // nothing was written, not even the bound records of the same call
    assert!(target.keys(None).unwrap().is_empty());
}

#[test]
fn a_keyed_import_of_a_record_sealed_without_binding_is_refused() {
    let f = Fixture::new();
    let (mut bound, _) = records(&f);
    for r in &mut bound {
        r.encoding = r.encoding.trim_end_matches("-bound").to_owned();
    }
    let target = f.target("copy", Some(KEY));
    let reason = records_refusal(target.import_entries(&bound));
    assert!(reason.contains("2 of 2 records"), "{reason}");
    assert!(target.keys(None).unwrap().is_empty());
}

#[test]
fn a_keyed_import_of_bound_records_succeeds_and_reads() {
    let f = Fixture::new();
    let (bound, _) = records(&f);
    let target = f.target("copy", Some(KEY));
    assert_eq!(target.import_entries(&bound).unwrap(), 2);
    // the namespace is not bound: the copy reads under the same key
    assert_eq!(target.get(&f.a).unwrap().unwrap().payload, vec![1_u8, 1, 1]);
    assert_eq!(target.get(&f.b).unwrap().unwrap().payload, vec![2_u8, 2, 2]);
    // the authenticity is not checked at import: a record sealed under another
    // key is imported and fails closed on read
    let other = f.target("other", Some([0x66; 32]));
    assert_eq!(other.import_entries(&bound).unwrap(), 2);
    assert!(matches!(
        other.get(&f.a),
        Err(LocalFileCacheError::EncryptionError(_))
    ));
}

#[test]
fn a_keyless_import_stores_sealed_and_plain_records_verbatim() {
    let f = Fixture::new();
    let (bound, plain) = records(&f);
    let target = f.target("copy", None);
    let all: Vec<_> = bound.iter().chain(plain.iter()).cloned().collect();
    assert_eq!(target.import_entries(&all).unwrap(), 3);
    let exported = target.export_entries().unwrap();
    for original in &all {
        let copy = exported.iter().find(|r| r.path == original.path).unwrap();
        assert_eq!(copy.encoding, original.encoding);
        assert_eq!(copy.payload_b64, original.payload_b64);
    }
    // and a keyed engine then reads the bound ones of that namespace
    let keyed = f.target("copy", Some(KEY));
    assert_eq!(keyed.get(&f.a).unwrap().unwrap().payload, vec![1_u8, 1, 1]);
}

#[test]
fn import_from_follows_the_same_rule() {
    let f = Fixture::new();
    let _ = records(&f); // adds the plain entry to the default namespace
    let source_plain = f.target("src-plain", None);
    let p = write_plain(&f, "p2.bin", vec![4]);
    source_plain.set(&p, &vec![4_u8]).unwrap();
    let keyed_dest = f.target("dest", Some(KEY));
    // from an engine without a key: refused
    let reason = records_refusal(keyed_dest.import_from(&source_plain));
    assert!(reason.contains("1 of 1 records"), "{reason}");
    assert!(keyed_dest.keys(None).unwrap().is_empty());
    // from a keyed engine with the same key: copied (bound)
    let source_bound = f.target("src-bound", Some(KEY));
    source_bound.set(&f.a, &vec![5_u8]).unwrap();
    assert_eq!(keyed_dest.import_from(&source_bound).unwrap(), 1);
    assert_eq!(keyed_dest.get(&f.a).unwrap().unwrap().payload, vec![5_u8]);
    // an engine without a key can copy from anything
    let keyless_dest = f.target("dest2", None);
    assert_eq!(keyless_dest.import_from(&source_plain).unwrap(), 1);
}

/// The upgrade path the error message names, end to end: records that a keyed
/// engine refuses are imported into an engine without a key, and one rotation
/// with the key makes them readable.
#[test]
fn the_remedy_the_message_names_works() {
    let f = Fixture::new();
    let (_, plain) = records(&f);
    let keyed = f.target("up", Some(KEY));
    records_refusal(keyed.import_entries(&plain));
    let keyless = f.target("up", None);
    assert_eq!(keyless.import_entries(&plain).unwrap(), 1);
    assert_eq!(keyed.rotate_encryption_key(&KEY).unwrap(), 1);
    let source = canonical_from_record(&plain[0]);
    assert_eq!(
        keyed.get(&source).unwrap().unwrap().payload,
        vec![9_u8, 9, 9]
    );
}

fn canonical_from_record(r: &localcache::ExportRecord) -> PathBuf {
    PathBuf::from(&r.path)
}

// ---------------------------------------------------------------------------
// Recovery: a rotation that fails because one entry cannot be opened
// ---------------------------------------------------------------------------

/// `rotate_encryption_key` is all-or-nothing, and its error names no entry (a
/// stored path is never echoed into an error). The book's recovery: run
/// `query().run_report()`, `remove` the entries it lists with an
/// `EncryptionError`, rotate again. This is that sequence, end to end.
#[test]
fn a_rotation_that_fails_on_one_entry_is_recovered_with_run_report_and_remove() {
    let f = Fixture::new();
    // a third, plain entry, which the rotation has to seal as well
    let plain = write_plain(&f, "plain.bin", vec![9, 9, 9]);
    // swap: b's row now holds a's ciphertext, which does not open at b's path
    let (pa, pb) = (canonical(&f.a), canonical(&f.b));
    f.sql(&format!(
        "UPDATE payloads SET \
           content  = (SELECT p.content  FROM payloads p JOIN files f ON f.id = p.file_id WHERE f.path = '{pa}'), \
           encoding = (SELECT p.encoding FROM payloads p JOIN files f ON f.id = p.file_id WHERE f.path = '{pa}') \
         WHERE file_id = (SELECT id FROM files WHERE path = '{pb}')"
    ));
    let engine = f.engine();
    let before = rows_of(&f);

    // the rotation fails, whole, and changes nothing; the error names no path
    let error = engine.rotate_encryption_key(&KEY).unwrap_err();
    assert!(
        matches!(error, LocalFileCacheError::EncryptionError(_)),
        "{error:?}"
    );
    assert!(!error.to_string().contains("b.bin"), "{error}");
    assert_eq!(
        rows_of(&f),
        before,
        "a failed rotation changed the database"
    );
    assert!(matches!(
        engine.get(&plain),
        Err(LocalFileCacheError::UnsealedEntry)
    ));

    // run_report lists the entry, with its path and an EncryptionError; the
    // unsealed entry is listed too, with UnsealedEntry, which needs nothing
    let report = engine.query().run_report().unwrap();
    assert_eq!(report.entries.len(), 1, "only a still reads");
    let mut seen: Vec<(PathBuf, String)> = report
        .skipped
        .iter()
        .map(|s| {
            let kind = match s.error {
                LocalFileCacheError::EncryptionError(_) => "EncryptionError",
                LocalFileCacheError::UnsealedEntry => "UnsealedEntry",
                _ => "other",
            };
            (s.path.clone(), kind.to_owned())
        })
        .collect();
    seen.sort();
    let mut expected = vec![
        (PathBuf::from(&pb), "EncryptionError".to_owned()),
        (PathBuf::from(canonical(&plain)), "UnsealedEntry".to_owned()),
    ];
    expected.sort();
    assert_eq!(seen, expected);

    // remove the entries listed with an EncryptionError, then rotate again
    for (path, kind) in &seen {
        if kind == "EncryptionError" {
            assert!(engine.remove(path).unwrap());
        }
    }
    assert_eq!(engine.rotate_encryption_key(&KEY).unwrap(), 2);
    assert_eq!(engine.get(&f.a).unwrap().unwrap().payload, vec![1_u8, 1, 1]);
    assert_eq!(
        engine.get(&plain).unwrap().unwrap().payload,
        vec![9_u8, 9, 9]
    );
    assert!(engine.get(&f.b).unwrap().is_none());

    // and an engine without a key now gets MissingEncryptionKey for EVERY entry
    // of the namespace, the formerly plain one included
    let keyless: CacheEngine<Vec<u8>> = CacheEngine::builder().database(&f.db).build().unwrap();
    for path in [&f.a, &plain] {
        assert!(matches!(
            keyless.get(path),
            Err(LocalFileCacheError::MissingEncryptionKey)
        ));
    }
}

/// Every row of `payloads` as `(file_id, tag, content)`.
fn rows_of(f: &Fixture) -> Vec<(i64, String, Vec<u8>)> {
    let conn = Connection::open(&f.db).unwrap();
    let mut stmt = conn
        .prepare("SELECT file_id, encoding, content FROM payloads ORDER BY file_id")
        .unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}
