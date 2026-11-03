//! RFC 029 R4 and R5 (v0.23.1): the encryption key does not print, and the
//! crate does not keep its own copies of it once they are dropped.
//!
//! What these tests do **not** claim: that the key cannot leak. They cover the
//! options' `Debug` output, the pool and builder paths that hold a copy, and
//! the behaviour that must not change. Erasure itself is tested next to the
//! `Key` type (`src/key.rs`), where the type is visible.

#![cfg(feature = "encryption")]

mod common;

use std::fmt::Write as _;

use localcache::{CacheEngine, CacheOptions};
use tempfile::TempDir;

use common::write_file;

/// A key that is not a repeated byte, so that no run of its bytes can appear
/// in a `Debug` string by accident (a key of `[7; 32]` prints the digit 7
/// everywhere).
const KEY: [u8; 32] = [
    0x3a, 0x91, 0xc4, 0x07, 0xe2, 0x58, 0xb6, 0x1d, 0x7f, 0xa0, 0x4c, 0xd9, 0x26, 0x8b, 0xf3, 0x65,
    0x12, 0xce, 0x9d, 0x30, 0xab, 0x74, 0x59, 0xe8, 0x0f, 0xbd, 0x41, 0x96, 0xc7, 0x2a, 0x83, 0x5e,
];

/// Every run of four consecutive key bytes, as the text a formatter could
/// produce for it: a decimal list (`58, 145, 196, 7`) and hex (`3a91c407`,
/// lower and upper case).
fn windows_as_text(key: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    for window in key.windows(4) {
        let decimal: Vec<String> = window.iter().map(|b| b.to_string()).collect();
        out.push(decimal.join(","));
        let mut hex = String::new();
        for b in window {
            write!(hex, "{b:02x}").unwrap();
        }
        out.push(hex.to_uppercase());
        out.push(hex);
    }
    out
}

/// The formatted text with everything a pretty-printer or a list adds between
/// numbers removed, so `[58, 145,` and `58,\n    145,` both normalise to
/// `58,145,` and `0x3a, 0x91` to `3a91`.
fn normalised(text: &str) -> String {
    text.replace("0x", "")
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '_')
        .collect()
}

fn options_with_key() -> CacheOptions {
    CacheOptions {
        encryption_key: Some(KEY.to_vec()),
        ..Default::default()
    }
}

fn assert_no_run_of_the_key(text: &str, what: &str) {
    let norm = normalised(text);
    for window in windows_as_text(&KEY) {
        assert!(
            !norm.contains(&window),
            "{what} contains a run of the key ({window}):\n{text}"
        );
    }
}

#[test]
fn the_options_debug_output_has_no_run_of_the_key() {
    let options = options_with_key();
    assert_no_run_of_the_key(&format!("{options:?}"), "{:?} of the options");
    assert_no_run_of_the_key(&format!("{options:#?}"), "{:#?} of the options");
}

#[test]
fn the_options_debug_output_says_redacted_when_a_key_is_set_and_none_otherwise() {
    let with = format!("{:?}", options_with_key());
    assert!(
        with.contains("encryption_key: Some(\"[redacted]\")"),
        "{with}"
    );
    let without = format!("{:?}", CacheOptions::default());
    assert!(without.contains("encryption_key: None"), "{without}");
}

/// Everything but the key prints as the derive printed it (v0.23.0): same
/// names, same order. The golden text was taken from the v0.23.0 tree.
#[cfg(all(feature = "compression", feature = "watching"))]
#[test]
fn the_options_debug_output_is_otherwise_what_the_derive_printed() {
    let text = format!("{:?}", CacheOptions::default());
    assert_eq!(
        text,
        "CacheOptions { database_path: \"localcache.sqlite3\", \
         change_detection_mode: MetadataOnly, codec: Bincode, journal_mode: Preserve, \
         synchronous: Normal, ttl: None, namespace: \"default\", read_only: false, \
         payload_version: 0, max_entries: None, encryption_key: None, \
         compress_payloads: false, max_decompressed_bytes: 67108864, watch_dirs: false, \
         shared_cache: false }"
    );
}

#[test]
fn a_read_pool_of_several_slots_opens_and_reads_with_a_key() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("c.sqlite3");
    let source = write_file(&directory, "f.bin", b"x");
    let writer: CacheEngine<Vec<u8>> = CacheEngine::builder()
        .database(&database)
        .encryption_key(KEY.to_vec())
        .build()
        .unwrap();
    writer.set(&source, &vec![1_u8, 2, 3]).unwrap();

    let pool = CacheEngine::<Vec<u8>>::builder()
        .database(&database)
        .encryption_key(KEY.to_vec())
        .build_read_pool(4)
        .unwrap();
    for _ in 0..8 {
        assert_eq!(
            pool.get(&source).unwrap().unwrap().payload,
            vec![1_u8, 2, 3]
        );
    }
}

#[test]
fn the_builder_uses_the_last_key_it_was_given() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("c.sqlite3");
    let source = write_file(&directory, "f.bin", b"x");
    let first = [0x11_u8; 32];
    let engine: CacheEngine<Vec<u8>> = CacheEngine::builder()
        .database(&database)
        .encryption_key(first.to_vec())
        .encryption_key(KEY.to_vec())
        .build()
        .unwrap();
    engine.set(&source, &vec![9_u8]).unwrap();
    drop(engine);

    let reader: CacheEngine<Vec<u8>> = CacheEngine::builder()
        .database(&database)
        .encryption_key(KEY.to_vec())
        .build()
        .unwrap();
    assert_eq!(reader.get(&source).unwrap().unwrap().payload, vec![9_u8]);
    let wrong: CacheEngine<Vec<u8>> = CacheEngine::builder()
        .database(&database)
        .encryption_key(first.to_vec())
        .build()
        .unwrap();
    assert!(wrong.get(&source).is_err());
}

/// A key of the wrong length is still refused, with the same message, and the
/// message carries the length only.
#[test]
fn a_key_of_the_wrong_length_is_refused_with_its_length_only() {
    let error = CacheEngine::<Vec<u8>>::builder()
        .database(":memory:")
        .encryption_key(vec![0xAB; 5])
        .build()
        .err()
        .unwrap();
    let text = format!("{error} {error:?}");
    assert!(text.contains("must be exactly 32 bytes, got 5"), "{text}");
    assert!(!text.contains("171"), "{text}");
}
