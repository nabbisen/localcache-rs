//! Integration tests — portability.

mod common;
use common::write_file;
use std::fs;

#[allow(unused_imports)]
use base64::Engine as _;
use tempfile::TempDir;

#[allow(unused_imports)]
use localcache::{CacheEngine, CacheOptions, ScanOptions};

#[test]
fn glob_nested_braces_within_alternative() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("nested_brace");
    fs::create_dir(&root).unwrap();

    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();

    // "{a,{b,c}}.txt" should expand to ["a.txt", "b.txt", "c.txt"]
    for name in &["a.txt", "b.txt", "c.txt", "d.txt"] {
        fs::write(root.join(name), b"").unwrap();
    }

    let opts = ScanOptions {
        recursive: false,
        glob_pattern: Some("{a,{b,c}}.txt".into()),
        ..ScanOptions::default()
    };
    let results = engine.scan_dir_filtered(&root, opts).unwrap();
    assert_eq!(results.len(), 3, "should match a, b, c but not d");
    let names: Vec<_> = results
        .iter()
        .map(|(p, _)| p.file_name().unwrap().to_str().unwrap().to_owned())
        .collect();
    assert!(names.contains(&"a.txt".to_owned()));
    assert!(names.contains(&"b.txt".to_owned()));
    assert!(names.contains(&"c.txt".to_owned()));
    assert!(!names.contains(&"d.txt".to_owned()));
}

#[test]
fn glob_nested_plus_outer_group() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("nested_outer");
    fs::create_dir(&root).unwrap();

    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();

    // "{pre,{mid,post}}_{x,y}.txt" → 6 combinations
    for prefix in &["pre", "mid", "post"] {
        for suffix in &["x", "y"] {
            fs::write(root.join(format!("{prefix}_{suffix}.txt")), b"").unwrap();
        }
    }
    // one extra that should not match
    fs::write(root.join("other.txt"), b"").unwrap();

    let opts = ScanOptions {
        recursive: false,
        glob_pattern: Some("{pre,{mid,post}}_{x,y}.txt".into()),
        ..ScanOptions::default()
    };
    let results = engine.scan_dir_filtered(&root, opts).unwrap();
    assert_eq!(results.len(), 6);
}

// ====================================================================
// Phase 9 — export_entries / import_entries
// ====================================================================

#[test]
fn export_import_roundtrip() {
    let dir = TempDir::new().unwrap();

    // Source engine.
    let src: CacheEngine<Vec<f32>> = CacheEngine::builder().database(":memory:").build().unwrap();

    let p1 = write_file(&dir, "exp1.txt", b"data1");
    let p2 = write_file(&dir, "exp2.txt", b"data2");
    src.set(&p1, &vec![1.0_f32, 2.0]).unwrap();
    src.set(&p2, &vec![3.0_f32, 4.0]).unwrap();

    // Export.
    let records = src.export_entries().unwrap();
    assert_eq!(records.len(), 2);

    // Import into a fresh engine.
    let dst: CacheEngine<Vec<f32>> = CacheEngine::builder().database(":memory:").build().unwrap();

    let imported = dst.import_entries(&records).unwrap();
    assert_eq!(imported, 2);
    assert_eq!(dst.entry_count().unwrap(), 2);
}

#[test]
fn export_preserves_metadata() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(":memory:")
        .payload_version(5)
        .build()
        .unwrap();

    let path = write_file(&dir, "meta.txt", b"content");
    engine.set(&path, &vec![9.9_f32]).unwrap();

    let records = engine.export_entries().unwrap();
    assert_eq!(records.len(), 1);

    let r = &records[0];
    assert_eq!(r.payload_version, 5);
    assert_eq!(r.encoding, "raw");
    assert!(r.updated_at > 0);
}

#[test]
fn import_replaces_existing_entry() {
    let dir = TempDir::new().unwrap();

    let src: CacheEngine<Vec<f32>> = CacheEngine::builder().database(":memory:").build().unwrap();
    let p = write_file(&dir, "replace.txt", b"data");
    src.set(&p, &vec![1.0_f32]).unwrap();

    let mut records = src.export_entries().unwrap();

    // Modify the payload in the record (simulate updated data).
    let new_payload: Vec<f32> = vec![99.0_f32];
    let new_bytes = bincode::serde::encode_to_vec(&new_payload, bincode::config::legacy()).unwrap();
    records[0].payload_b64 = base64::engine::general_purpose::STANDARD.encode(&new_bytes);

    let dst: CacheEngine<Vec<f32>> = CacheEngine::builder().database(":memory:").build().unwrap();

    // First import.
    dst.import_entries(&records).unwrap();
    // Second import should replace.
    let imported = dst.import_entries(&records).unwrap();
    assert_eq!(imported, 1);
    assert_eq!(dst.entry_count().unwrap(), 1);
}

#[test]
fn import_from_cross_engine() {
    let dir = TempDir::new().unwrap();
    let db1 = dir.path().join("db1.sqlite3");
    let db2 = dir.path().join("db2.sqlite3");

    let eng1: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(db1)
        .namespace("source")
        .build()
        .unwrap();

    let eng2: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(db2)
        .namespace("dest")
        .build()
        .unwrap();

    let p1 = write_file(&dir, "cp1.txt", b"x");
    let p2 = write_file(&dir, "cp2.txt", b"y");
    eng1.set(&p1, &vec![1.0_f32]).unwrap();
    eng1.set(&p2, &vec![2.0_f32]).unwrap();

    let copied = eng2.import_from(&eng1).unwrap();
    assert_eq!(copied, 2);
    assert_eq!(eng2.entry_count().unwrap(), 2);
}

#[test]
fn import_from_different_namespace_same_db() {
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("shared.sqlite3");

    let eng_a: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(db.clone())
        .namespace("alpha")
        .build()
        .unwrap();

    let eng_b: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(db)
        .namespace("beta")
        .build()
        .unwrap();

    let p = write_file(&dir, "ns_copy.txt", b"data");
    eng_a.set(&p, &vec![7.0_f32]).unwrap();

    let copied = eng_b.import_from(&eng_a).unwrap();
    assert_eq!(copied, 1);

    // Both namespaces now have the entry.
    assert_eq!(eng_a.entry_count().unwrap(), 1);
    assert_eq!(eng_b.entry_count().unwrap(), 1);
}

// ====================================================================
// Phase 9 — Async export / import
// ====================================================================

#[cfg(feature = "async")]
mod async_phase9_tests {
    use super::*;
    use localcache::AsyncCacheEngine;

    #[tokio::test]
    async fn async_export_import_roundtrip() {
        let dir = TempDir::new().unwrap();

        let src: AsyncCacheEngine<Vec<f32>> = AsyncCacheEngine::open(CacheOptions {
            database_path: ":memory:".into(),
            ..CacheOptions::default()
        })
        .await
        .unwrap();

        let p = write_file(&dir, "async_exp.txt", b"hi");
        src.set(p.clone(), vec![5.0_f32]).await.unwrap();

        let records = src.export_entries().await.unwrap();
        assert_eq!(records.len(), 1);

        let dst: AsyncCacheEngine<Vec<f32>> = AsyncCacheEngine::open(CacheOptions {
            database_path: ":memory:".into(),
            ..CacheOptions::default()
        })
        .await
        .unwrap();

        let imported = dst.import_entries(records).await.unwrap();
        assert_eq!(imported, 1);
        assert_eq!(dst.entry_count().await.unwrap(), 1);
    }
}

// ====================================================================
// Phase 10 — contains() and keys()
