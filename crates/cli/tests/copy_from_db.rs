//! `copy --from-db`, `--upgrade-source`, and the deprecated `migrate`
//! (RFC 024 R10).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use localcache::{CacheEngine, CacheOptions};
use tempfile::TempDir;

const OLD_SCHEMA_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/compat-v0_18.sqlite3"
);
const OLD_SCHEMA_FIXTURE_SHA256: &str =
    "9046c0d81ac51ba59ca45de0849a7955d3d4f04a92ff39adfd0042e35c9b31bb";

const SCHEMA_REFUSAL: &str = "read-only open requires the current database schema";
const MIGRATE_WARNING: &str =
    "warning: `migrate` is deprecated and will be removed in 0.22.0; use `copy --from-db`";

fn localcache(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_localcache"))
        .args(arguments)
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn path_text(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// A current-schema database holding `count` entries in namespace `namespace`.
fn current_database(directory: &TempDir, name: &str, namespace: &str, count: usize) -> PathBuf {
    let database = directory.path().join(name);
    let engine: CacheEngine<Vec<u8>> = CacheEngine::builder()
        .database(&database)
        .namespace(namespace)
        .build()
        .unwrap();
    for index in 0..count {
        let source = directory.path().join(format!("{namespace}-{index}.bin"));
        fs::write(&source, b"source").unwrap();
        engine.set(&source, &vec![index as u8]).unwrap();
    }
    database
}

fn entry_count(database: &Path, namespace: &str) -> usize {
    let engine: CacheEngine<Vec<u8>> = CacheEngine::open(CacheOptions {
        database_path: database.to_path_buf(),
        namespace: namespace.to_owned(),
        read_only: true,
        ..CacheOptions::default()
    })
    .unwrap();
    engine.entry_count().unwrap()
}

/// A temporary copy of the old-schema fixture, after checking the committed
/// file is the one its README describes.
fn old_schema_copy(directory: &TempDir) -> PathBuf {
    let bytes = fs::read(OLD_SCHEMA_FIXTURE).unwrap();
    assert_eq!(
        sha256_hex(&bytes),
        OLD_SCHEMA_FIXTURE_SHA256,
        "crates/cli/tests/fixtures/compat-v0_18.sqlite3 must not change"
    );
    let copy = directory.path().join("old-schema.sqlite3");
    fs::write(&copy, bytes).unwrap();
    copy
}

#[test]
fn copy_from_db_copies_a_namespace_across_databases() {
    let directory = TempDir::new().unwrap();
    let source = current_database(&directory, "source.sqlite3", "src", 3);
    let destination = directory.path().join("destination.sqlite3");
    let source_before = fs::read(&source).unwrap();

    let output = localcache(&[
        "-d",
        path_text(&destination),
        "-n",
        "dst",
        "copy",
        "--from-db",
        path_text(&source),
        "--from",
        "src",
    ]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(text(&output.stderr).contains("Copied 3 entries from namespace 'src' → 'dst'"));

    assert_eq!(entry_count(&destination, "dst"), 3);
    assert_eq!(
        fs::read(&source).unwrap(),
        source_before,
        "a current-schema source is opened read-only and never modified"
    );
}

#[test]
fn copy_without_from_db_still_copies_within_one_database() {
    let directory = TempDir::new().unwrap();
    let database = current_database(&directory, "one.sqlite3", "old", 2);

    let output = localcache(&[
        "-d",
        path_text(&database),
        "copy",
        "--from",
        "old",
        "--to",
        "new",
    ]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(text(&output.stderr).contains("Copied 2 entries from namespace 'old' → 'new'"));
    assert_eq!(entry_count(&database, "new"), 2);
    assert_eq!(entry_count(&database, "old"), 2);
}

#[test]
fn an_old_schema_source_is_refused_and_left_unchanged() {
    let directory = TempDir::new().unwrap();
    let source = old_schema_copy(&directory);
    let destination = directory.path().join("destination.sqlite3");
    let before = fs::read(&source).unwrap();

    let output = localcache(&[
        "-d",
        path_text(&destination),
        "copy",
        "--from-db",
        path_text(&source),
        "--from",
        "plain",
        "--to",
        "plain",
    ]);

    assert!(!output.status.success());
    let stderr = text(&output.stderr);
    assert!(stderr.contains(SCHEMA_REFUSAL), "stderr: {stderr}");
    assert!(
        stderr.contains("database was not modified"),
        "the library's own message comes first: {stderr}"
    );
    assert!(stderr.contains("--upgrade-source"), "stderr: {stderr}");
    assert_eq!(
        sha256_hex(&fs::read(&source).unwrap()),
        sha256_hex(&before),
        "the refused source's SHA-256 is unchanged"
    );
    assert_eq!(fs::read(&source).unwrap(), before);
    assert!(
        !destination.exists(),
        "a refused source leaves no destination file behind"
    );
}

#[test]
fn upgrade_source_permits_the_upgrade_and_the_copy_succeeds() {
    let directory = TempDir::new().unwrap();
    let source = old_schema_copy(&directory);
    let destination = directory.path().join("destination.sqlite3");
    let before = fs::read(&source).unwrap();

    let output = localcache(&[
        "-d",
        path_text(&destination),
        "copy",
        "--from-db",
        path_text(&source),
        "--from",
        "plain",
        "--to",
        "plain",
        "--upgrade-source",
    ]);

    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(text(&output.stderr).contains("Copied 2 entries from namespace 'plain' → 'plain'"));
    assert_eq!(entry_count(&destination, "plain"), 2);
    assert_ne!(
        fs::read(&source).unwrap(),
        before,
        "the user asked for the source to be upgraded, and it was"
    );
    // The upgraded source now opens read-only.
    assert_eq!(entry_count(&source, "plain"), 2);
}

/// `--from-db` naming the `-d` file is the same operation as omitting it, so
/// on an old-schema database it succeeds exactly as plain `copy` does, without
/// `--upgrade-source`: writing that file is inherent to the copy.
#[test]
fn from_db_naming_the_destination_file_behaves_like_a_same_database_copy() {
    let plain_directory = TempDir::new().unwrap();
    let plain = old_schema_copy(&plain_directory);
    let plain_output = localcache(&[
        "-d",
        path_text(&plain),
        "copy",
        "--from",
        "plain",
        "--to",
        "copied",
    ]);
    assert!(
        plain_output.status.success(),
        "{}",
        text(&plain_output.stderr)
    );

    let explicit_directory = TempDir::new().unwrap();
    let explicit = old_schema_copy(&explicit_directory);
    let explicit_output = localcache(&[
        "-d",
        path_text(&explicit),
        "copy",
        "--from-db",
        path_text(&explicit),
        "--from",
        "plain",
        "--to",
        "copied",
    ]);
    assert!(
        explicit_output.status.success(),
        "{}",
        text(&explicit_output.stderr)
    );
    assert!(!text(&explicit_output.stderr).contains("--upgrade-source"));

    for (database, output) in [(&plain, &plain_output), (&explicit, &explicit_output)] {
        assert!(
            text(&output.stderr).contains("Copied 2 entries from namespace 'plain' → 'copied'")
        );
        assert_eq!(entry_count(database, "copied"), 2);
        assert_eq!(entry_count(database, "plain"), 2);
    }
}

/// The same file spelled differently (a `.` component) is still the same file.
#[test]
fn from_db_naming_the_destination_file_by_another_spelling_is_still_the_same_file() {
    let directory = TempDir::new().unwrap();
    let database = old_schema_copy(&directory);
    let respelled = directory.path().join(".").join("old-schema.sqlite3");

    let output = localcache(&[
        "-d",
        path_text(&database),
        "copy",
        "--from-db",
        path_text(&respelled),
        "--from",
        "plain",
        "--to",
        "copied",
    ]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert_eq!(entry_count(&database, "copied"), 2);
}

#[test]
fn upgrade_source_does_not_touch_a_source_that_is_already_current() {
    let directory = TempDir::new().unwrap();
    let source = current_database(&directory, "source.sqlite3", "src", 1);
    let destination = directory.path().join("destination.sqlite3");
    let before = fs::read(&source).unwrap();

    let output = localcache(&[
        "-d",
        path_text(&destination),
        "copy",
        "--from-db",
        path_text(&source),
        "--from",
        "src",
        "--to",
        "src",
        "--upgrade-source",
    ]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert_eq!(fs::read(&source).unwrap(), before);
    assert_eq!(entry_count(&destination, "src"), 1);
}

#[test]
fn migrate_still_works_and_warns_first() {
    let directory = TempDir::new().unwrap();
    let source = current_database(&directory, "source.sqlite3", "src", 2);
    let destination = directory.path().join("destination.sqlite3");

    let output = localcache(&[
        "migrate",
        "--src-db",
        path_text(&source),
        "--src-ns",
        "src",
        "--dst-db",
        path_text(&destination),
        "--dst-ns",
        "dst",
    ]);
    assert!(output.status.success(), "{}", text(&output.stderr));

    let stderr = text(&output.stderr);
    assert_eq!(
        stderr.lines().next(),
        Some(MIGRATE_WARNING),
        "the warning is the first thing printed: {stderr}"
    );
    assert!(stderr.contains("Migrated 2 entries"), "stderr: {stderr}");
    assert_eq!(entry_count(&destination, "dst"), 2);
}

#[test]
fn migrate_help_says_it_is_deprecated() {
    let output = localcache(&["migrate", "--help"]);
    assert!(output.status.success());
    let help = text(&output.stdout);
    assert!(
        help.contains("Deprecated: will be removed in 0.22.0; use `copy --from-db`"),
        "help: {help}"
    );
}

// ---------------------------------------------------------------------------
// SHA-256 (FIPS 180-4), so that the fixture's digest can be checked without a
// new dependency. The fixture digest below being right is also what proves
// this implementation right.
// ---------------------------------------------------------------------------

fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());

    for block in message.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }
    state.iter().map(|word| format!("{word:08x}")).collect()
}

#[test]
fn the_sha256_helper_matches_the_standard_test_vectors() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}
