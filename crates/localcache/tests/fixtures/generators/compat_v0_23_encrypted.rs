//! Fixture generator compiled in an isolated localcache 0.23.1 checkout
//! (RFC 029): a database written by the published release with an encryption
//! key, before entries were bound to their path.
//!
//! One namespace per encrypted tag variant, three entries each, and one
//! namespace of plain entries (written by an engine with no key). Every
//! payload is `payload(namespace_index, entry_index)`, which the tests
//! recompute.

use std::fs;
use std::path::Path;

use localcache::{CacheEngine, Codec, JournalMode};

const DATABASE: &str = "/fixture/compat-v0_23-encrypted.sqlite3";
const DIRECTORY: &str = "/fixture";

fn key() -> Vec<u8> {
    (0..32).map(|i| 0x40 + i as u8).collect()
}

fn payload(namespace_index: usize, entry_index: usize) -> Vec<u8> {
    (0..200)
        .map(|j| ((namespace_index * 61 + entry_index * 13 + j * 3) % 251) as u8)
        .collect()
}

fn input(namespace: &str, entry_index: usize) -> String {
    format!("{DIRECTORY}/input-{namespace}-{entry_index}.bin")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if Path::new(DATABASE).exists() {
        fs::remove_file(DATABASE)?;
    }
    // (namespace, codec, compressed, encrypted)
    let variants = [
        ("raw", Codec::Bincode, false, true),
        ("zstd", Codec::Bincode, true, true),
        ("json", Codec::Json, false, true),
        ("json-zstd", Codec::Json, true, true),
        ("plain", Codec::Bincode, false, false),
    ];
    for (namespace_index, (namespace, codec, compressed, encrypted)) in
        variants.into_iter().enumerate()
    {
        let mut builder = CacheEngine::<Vec<u8>>::builder()
            .database(DATABASE)
            .journal_mode(JournalMode::Delete)
            .namespace(namespace)
            .codec(codec);
        if compressed {
            builder = builder.compress();
        }
        if encrypted {
            builder = builder.encryption_key(key());
        }
        let engine = builder.build()?;
        for entry_index in 0..3 {
            let path = input(namespace, entry_index);
            fs::write(&path, format!("synthetic input {namespace} {entry_index}\n"))?;
            engine.set(&path, &payload(namespace_index, entry_index))?;
        }
    }
    for (namespace, ..) in variants {
        for entry_index in 0..3 {
            fs::remove_file(input(namespace, entry_index))?;
        }
    }
    Ok(())
}
