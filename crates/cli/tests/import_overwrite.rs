//! Residual pre-RC correction B — `import --overwrite=false` must actually
//! skip existing entries instead of silently overwriting them.
//!
//! `ExportRecord.path` is the exact stored key `import_entries`'s
//! `ON CONFLICT(namespace, path)` upsert conflicts on, so every fixture here
//! builds records against real on-disk paths and compares outcomes by that
//! same exact string, not by `CacheEngine::contains`'s canonicalising lookup.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use localcache::CacheEngine;
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

/// Build one `ExportRecord` (as a JSON line) for `path` holding `payload`,
/// using a throwaway in-memory engine so the record carries a real,
/// correctly encoded metadata/hash/payload — not a hand-rolled fixture.
fn export_line(path: &Path, payload: &[u8]) -> String {
    let engine: CacheEngine<Vec<u8>> = CacheEngine::builder().database(":memory:").build().unwrap();
    engine.set(path, &payload.to_vec()).unwrap();
    let records = engine.export_entries().unwrap();
    assert_eq!(records.len(), 1);
    serde_json::to_string(&records[0]).unwrap()
}

fn write_import_file(directory: &TempDir, name: &str, lines: &[String]) -> PathBuf {
    write_file(directory, name, lines.join("\n").as_bytes())
}

fn stored_payload(database: &Path, path: &Path) -> Option<Vec<u8>> {
    let engine: CacheEngine<Vec<u8>> = CacheEngine::builder()
        .database(database)
        .read_only()
        .build()
        .unwrap();
    engine.get(path).unwrap().map(|e| e.payload)
}

#[test]
fn overwrite_default_replaces_existing_entry() {
    // Regression guard: `--overwrite` (the default) behaviour is unchanged.
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("db.sqlite3");
    let existing = write_file(&directory, "existing.bin", b"seed");

    {
        let writer: CacheEngine<Vec<u8>> =
            CacheEngine::builder().database(&database).build().unwrap();
        writer.set(&existing, &b"original".to_vec()).unwrap();
    }

    let import_file = write_import_file(
        &directory,
        "in.jsonl",
        &[export_line(&existing, b"replacement")],
    );

    let output = localcache(&database, &["import", "-i", import_file.to_str().unwrap()]);
    assert!(output.status.success());
    assert_eq!(
        stored_payload(&database, &existing),
        Some(b"replacement".to_vec())
    );
}

#[test]
fn overwrite_false_skips_existing_entry() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("db.sqlite3");
    let existing = write_file(&directory, "existing.bin", b"seed");

    {
        let writer: CacheEngine<Vec<u8>> =
            CacheEngine::builder().database(&database).build().unwrap();
        writer.set(&existing, &b"original".to_vec()).unwrap();
    }

    let import_file = write_import_file(
        &directory,
        "in.jsonl",
        &[export_line(&existing, b"replacement")],
    );

    let output = localcache(
        &database,
        &[
            "import",
            "--overwrite=false",
            "-i",
            import_file.to_str().unwrap(),
        ],
    );
    assert!(output.status.success());
    assert_eq!(
        stored_payload(&database, &existing),
        Some(b"original".to_vec()),
        "existing entry must be left untouched"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("skipped 1 existing"),
        "stderr was: {stderr}"
    );
}

#[test]
fn overwrite_false_mixed_batch_imports_new_and_skips_existing() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("db.sqlite3");
    let existing = write_file(&directory, "existing.bin", b"seed");
    let new_path = write_file(&directory, "new.bin", b"seed");

    {
        let writer: CacheEngine<Vec<u8>> =
            CacheEngine::builder().database(&database).build().unwrap();
        writer.set(&existing, &b"original".to_vec()).unwrap();
    }

    let import_file = write_import_file(
        &directory,
        "in.jsonl",
        &[
            export_line(&existing, b"replacement"),
            export_line(&new_path, b"brand-new"),
        ],
    );

    let output = localcache(
        &database,
        &[
            "import",
            "--overwrite=false",
            "-i",
            import_file.to_str().unwrap(),
        ],
    );
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Imported 1 entry, skipped 1 existing"),
        "stderr was: {stderr}"
    );
    assert_eq!(
        stored_payload(&database, &existing),
        Some(b"original".to_vec())
    );
    assert_eq!(
        stored_payload(&database, &new_path),
        Some(b"brand-new".to_vec())
    );
}

#[test]
fn overwrite_false_all_existing_imports_nothing() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("db.sqlite3");
    let a = write_file(&directory, "a.bin", b"seed-a");
    let b = write_file(&directory, "b.bin", b"seed-b");

    {
        let writer: CacheEngine<Vec<u8>> =
            CacheEngine::builder().database(&database).build().unwrap();
        writer.set(&a, &b"a-original".to_vec()).unwrap();
        writer.set(&b, &b"b-original".to_vec()).unwrap();
    }

    let import_file = write_import_file(
        &directory,
        "in.jsonl",
        &[export_line(&a, b"a-new"), export_line(&b, b"b-new")],
    );

    let output = localcache(
        &database,
        &[
            "import",
            "--overwrite=false",
            "-i",
            import_file.to_str().unwrap(),
        ],
    );
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Imported 0 entries, skipped 2 existing"),
        "stderr was: {stderr}"
    );
    assert_eq!(stored_payload(&database, &a), Some(b"a-original".to_vec()));
    assert_eq!(stored_payload(&database, &b), Some(b"b-original".to_vec()));
}

#[test]
fn overwrite_false_round_trip_leaves_populated_database_unchanged() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("db.sqlite3");
    let a = write_file(&directory, "a.bin", b"seed-a");
    let b = write_file(&directory, "b.bin", b"seed-b");

    {
        let writer: CacheEngine<Vec<u8>> =
            CacheEngine::builder().database(&database).build().unwrap();
        writer.set(&a, &b"a-payload".to_vec()).unwrap();
        writer.set(&b, &b"b-payload".to_vec()).unwrap();
    }
    let before = fs::read(&database).unwrap();

    // Export the database's own current state, then re-import it into
    // itself with --overwrite=false: every record already exists, so
    // nothing should change.
    let export_output = localcache(&database, &["export"]);
    assert!(export_output.status.success());
    let export_file = write_file(&directory, "roundtrip.jsonl", &export_output.stdout);

    let import_output = localcache(
        &database,
        &[
            "import",
            "--overwrite=false",
            "-i",
            export_file.to_str().unwrap(),
        ],
    );
    assert!(import_output.status.success());
    assert_eq!(fs::read(&database).unwrap(), before);
}
