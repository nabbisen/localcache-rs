//! RFC 029: the CLI takes no key, lists and exports sealed entries verbatim,
//! and imports and copies them verbatim. That stays valid because the
//! namespace is not part of what an entry is bound to: a keyed engine reads
//! the copy under the same key.

use std::path::Path;
use std::process::{Command, Output};

use localcache::CacheEngine;
use tempfile::TempDir;

const KEY: [u8; 32] = [0x6d; 32];

fn localcache(database: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_localcache"))
        .arg("--database")
        .arg(database)
        .args(arguments)
        .output()
        .unwrap()
}

fn ok(output: &Output) {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn keyed(database: &Path, namespace: &str) -> CacheEngine<Vec<u8>> {
    CacheEngine::builder()
        .database(database)
        .namespace(namespace)
        .encryption_key(KEY.to_vec())
        .build()
        .unwrap()
}

#[test]
fn export_import_and_copy_of_a_sealed_namespace_read_under_the_same_key() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("c.sqlite3");
    let source = directory.path().join("f.bin");
    std::fs::write(&source, b"x").unwrap();
    keyed(&database, "a")
        .set(&source, &vec![1_u8, 2, 3])
        .unwrap();

    // list shows the bound tag; the CLI never decodes
    let listed = localcache(&database, &["--namespace", "a", "list"]);
    ok(&listed);
    assert!(
        String::from_utf8_lossy(&listed.stdout).contains("raw-aes256gcm-bound"),
        "{}",
        String::from_utf8_lossy(&listed.stdout)
    );

    // export, then import into another namespace of the same database
    let file = directory.path().join("a.jsonl");
    ok(&localcache(
        &database,
        &["--namespace", "a", "export", "-o", file.to_str().unwrap()],
    ));
    ok(&localcache(
        &database,
        &["--namespace", "z", "import", "-i", file.to_str().unwrap()],
    ));
    // and copy
    ok(&localcache(
        &database,
        &["copy", "--from", "a", "--to", "y"],
    ));

    for namespace in ["a", "z", "y"] {
        let entry = keyed(&database, namespace).get(&source).unwrap().unwrap();
        assert_eq!(entry.payload, vec![1_u8, 2, 3], "namespace {namespace}");
    }
}
