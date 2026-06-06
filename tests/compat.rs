//! Compatibility regression tests — RFC 0008.
//!
//! # Wire-format stability (Q3)
//!
//! Opens a golden SQLite fixture written by v0.18.0 and asserts that every
//! payload decodes to its expected value.  If a code change alters the
//! bincode configuration, encoding tags, compression framing, or schema
//! in a non-migrating way, these tests fail in CI **before** any user
//! database is affected.
//!
//! The fixture lives in `tests/fixtures/` and must never be regenerated
//! routinely.  See `tests/fixtures/README.md` for the policy.
//!
//! # Path semantics (Q4)
//!
//! Documents and verifies the path-handling contract:
//! - `set` / `get` / `contains` canonicalize the input path.
//! - Entries for deleted files are still reachable via raw-path fallback.
//! - `cleanup_missing_files` removes exactly the entries whose stored
//!   paths no longer exist on disk.

use std::fs;
use std::path::Path;

use tempfile::TempDir;

use localcache::{CacheEngine, JournalMode};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Copy the committed fixture to `dir` and return the new path.
///
/// We copy rather than opening in-place so no accidental side files are
/// created next to the committed .sqlite3.
fn copy_fixture(dir: &TempDir) -> std::path::PathBuf {
    let src = Path::new("tests/fixtures/compat-v0_18.sqlite3");
    let dst = dir.path().join("compat.sqlite3");
    fs::copy(src, &dst).expect("fixture file missing — regenerate with gen_compat_fixture");
    dst
}

fn open_ns(db: &Path, ns: &str) -> CacheEngine<Vec<f32>> {
    CacheEngine::<Vec<f32>>::builder()
        .database(db)
        .namespace(ns)
        .journal_mode(JournalMode::Delete)
        .build()
        .unwrap()
}

#[cfg(feature = "compression")]
fn open_ns_compressed(db: &Path, ns: &str) -> CacheEngine<Vec<f32>> {
    CacheEngine::<Vec<f32>>::builder()
        .database(db)
        .namespace(ns)
        .journal_mode(JournalMode::Delete)
        .compress()
        .build()
        .unwrap()
}

// ---------------------------------------------------------------------------
// Wire-format stability — plain bincode namespace
// ---------------------------------------------------------------------------

#[test]
fn compat_plain_bincode_entry_a_decodes() {
    let dir = TempDir::new().unwrap();
    let db = copy_fixture(&dir);
    let engine = open_ns(&db, "plain");

    let entries = engine.query().run().unwrap();
    let mut payloads: Vec<Vec<f32>> = entries.into_iter().map(|e| e.payload).collect();
    payloads.sort_by(|a, b| a.partial_cmp(b).unwrap());

    assert_eq!(payloads.len(), 2, "plain namespace must contain 2 entries");
    assert_eq!(
        payloads[0],
        vec![1.0_f32, 2.0, 3.0],
        "first entry payload mismatch"
    );
    assert_eq!(
        payloads[1],
        vec![4.0_f32, 5.0, 6.0],
        "second entry payload mismatch"
    );
}

#[test]
fn compat_plain_bincode_f32_values_exact() {
    // Bit-exact check: f32 values must survive the encode/decode round-trip
    // identically — no precision loss, no byte-order flip.
    let dir = TempDir::new().unwrap();
    let db = copy_fixture(&dir);
    let engine = open_ns(&db, "plain");

    let mut entries = engine.query().run().unwrap();
    entries.sort_by(|a, b| a.path.cmp(&b.path));

    // Entry A: [1.0, 2.0, 3.0]
    let a = &entries[0].payload;
    assert_eq!(a[0].to_bits(), 1.0_f32.to_bits());
    assert_eq!(a[1].to_bits(), 2.0_f32.to_bits());
    assert_eq!(a[2].to_bits(), 3.0_f32.to_bits());
}

// ---------------------------------------------------------------------------
// Wire-format stability — compressed bincode namespace
// ---------------------------------------------------------------------------

#[cfg(feature = "compression")]
#[test]
fn compat_compressed_entry_decodes() {
    let dir = TempDir::new().unwrap();
    let db = copy_fixture(&dir);
    let engine = open_ns_compressed(&db, "compressed");

    let entries = engine.query().run().unwrap();
    assert_eq!(
        entries.len(),
        1,
        "compressed namespace must contain 1 entry"
    );
    assert_eq!(
        entries[0].payload,
        vec![7.0_f32, 8.0, 9.0],
        "compressed payload mismatch"
    );
}

#[cfg(feature = "compression")]
#[test]
fn compat_plain_and_compressed_coexist_in_same_db() {
    // Different encoding tags in one SQLite file — schema isolation check.
    let dir = TempDir::new().unwrap();
    let db = copy_fixture(&dir);
    let plain = open_ns(&db, "plain");
    let compressed = open_ns_compressed(&db, "compressed");

    assert_eq!(plain.entry_count().unwrap(), 2);
    assert_eq!(compressed.entry_count().unwrap(), 1);
}

// ---------------------------------------------------------------------------
// Path semantics — Q4 contract
// ---------------------------------------------------------------------------

#[test]
fn path_relative_and_absolute_resolve_to_same_entry() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("sem.sqlite3");
    let engine: CacheEngine<Vec<f32>> = CacheEngine::builder().database(&db_path).build().unwrap();

    let abs = dir.path().join("file.txt");
    fs::write(&abs, b"data").unwrap();
    engine.set(&abs, &vec![1.0_f32]).unwrap();

    // Store a relative path that resolves to the same file.
    let orig_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();
    let rel = std::path::PathBuf::from("file.txt");
    let result = engine.get(&rel);
    std::env::set_current_dir(&orig_dir).unwrap();

    assert!(
        result.unwrap().is_some(),
        "relative path must resolve to the same stored entry as absolute path"
    );
}

#[test]
fn deleted_file_entry_reachable_by_raw_path_fallback() {
    // After a file is deleted from disk, its cache entry is still
    // accessible via contains() and remove() using the raw stored path.
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();

    let path = dir.path().join("ephemeral.txt");
    fs::write(&path, b"exists").unwrap();
    engine.set(&path, &vec![2.0_f32]).unwrap();

    // Delete the file from disk.
    fs::remove_file(&path).unwrap();
    assert!(!path.exists());

    // contains() must still find the entry (raw-path fallback).
    let found = engine.contains(&path).unwrap();
    assert!(found, "entry must remain after file deletion");

    // remove() must succeed via raw-path fallback.
    let removed = engine.remove(&path).unwrap();
    assert!(removed, "remove() must find and delete the entry");
    assert_eq!(engine.entry_count().unwrap(), 0);
}

#[test]
fn cleanup_missing_files_removes_exactly_absent_entries() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();

    let keep = dir.path().join("keep.txt");
    let gone = dir.path().join("gone.txt");
    fs::write(&keep, b"alive").unwrap();
    fs::write(&gone, b"doomed").unwrap();
    engine.set(&keep, &vec![1.0_f32]).unwrap();
    engine.set(&gone, &vec![2.0_f32]).unwrap();

    // Remove one file — its entry should be cleaned up; the other must survive.
    fs::remove_file(&gone).unwrap();

    let removed = engine.cleanup_missing_files().unwrap();
    assert_eq!(removed, 1, "exactly one missing-file entry removed");
    assert_eq!(engine.entry_count().unwrap(), 1, "surviving entry intact");
    assert!(engine.contains(&keep).unwrap(), "kept entry still present");
}

#[test]
fn cleanup_missing_files_leaves_all_present_entries_intact() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();

    for i in 0..5u32 {
        let p = dir.path().join(format!("f{i}.txt"));
        fs::write(&p, b"x").unwrap();
        engine.set(&p, &vec![i as f32]).unwrap();
    }

    let removed = engine.cleanup_missing_files().unwrap();
    assert_eq!(removed, 0, "no entries removed when all files present");
    assert_eq!(engine.entry_count().unwrap(), 5);
}

// ---------------------------------------------------------------------------
// Path semantics — Unix symlink resolution
// ---------------------------------------------------------------------------

#[cfg(unix)]
#[test]
fn symlink_resolves_to_target_entry() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();

    let target = dir.path().join("target.txt");
    let link = dir.path().join("link.txt");
    fs::write(&target, b"target data").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();

    engine.set(&target, &vec![3.0_f32]).unwrap();

    // Accessing via symlink must resolve to the same stored entry.
    let via_link = engine.get(&link).unwrap();
    assert!(
        via_link.is_some(),
        "symlink lookup must resolve to canonical target entry"
    );
}
