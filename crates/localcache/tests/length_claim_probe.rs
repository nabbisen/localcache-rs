//! RFC 025 Amendment 2, D7 and RFC 028 R6.4–R6.5 — hostile payloads that claim
//! more than they hold, kept as regression tests.
//!
//! A crafted `raw` (legacy `bincode` format) payload whose length prefix claims
//! `u64::MAX / 8` elements but holds a few bytes could make a decoder reserve
//! memory for the claim before it finds the data missing (`bincode` 2 did for a
//! `String`: the Known issue of v0.22.0, fixed in v0.22.1 by the owned codec,
//! RFC 028). Likewise a recursive payload type could be nested one level per
//! four bytes. The tests ask only: does a `get` return an error, within a small
//! memory ceiling and without overflowing the stack?
//!
//! Each payload type runs in a child process under `ulimit -v` (512 MiB of
//! address space) so that an abort or allocation failure is a failed exit
//! status here and not a dead test runner.
#![cfg(target_os = "linux")]

mod common;

use std::collections::HashMap;

use common::{proc_status_kib, store_raw_payload, write_file};
use localcache::{CacheEngine, LocalFileCacheError};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tempfile::TempDir;

/// Recursion through a `Box`: one level per variant index (four bytes).
#[derive(Serialize, Deserialize, Default)]
enum List {
    Cons(Box<List>),
    #[default]
    Nil,
}

const CHILD: &str = "LOCALCACHE_D7_CHILD";
const CEILING_KIB: u64 = 512 * 1024;

/// A legacy-format length prefix of `u64::MAX / 8`, followed by `tail`.
fn claim(tail: &[u8]) -> Vec<u8> {
    let mut bytes = (u64::MAX / 8).to_le_bytes().to_vec();
    bytes.extend_from_slice(tail);
    bytes
}

/// A chain of `levels` `List::Cons` (variant index 0, four bytes each) ending
/// in `List::Nil`.
fn chain(levels: usize) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(levels * 4 + 4);
    for _ in 0..levels {
        bytes.extend_from_slice(&0_u32.to_le_bytes());
    }
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes
}

/// The stored bytes that make type `name` read a hostile claim.
fn forged(name: &str) -> Vec<u8> {
    match name {
        // `Some`, then the claim.
        "Option<String>" => [&[1_u8][..], &claim(&[1, 2, 3, 4])].concat(),
        // 10 MiB of nesting.
        "List" => chain(10 * 1024 * 1024 / 4),
        _ => claim(&[1, 2, 3, 4]),
    }
}

/// Store the hostile bytes as `T`'s payload and `get` it; report what happened.
fn probe<T: Serialize + DeserializeOwned + Default>(name: &str) -> String {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("claim.sqlite3");
    let file = write_file(&directory, "f.txt", b"x");
    let engine: CacheEngine<T> = CacheEngine::builder().database(&database).build().unwrap();
    engine.set(&file, &T::default()).unwrap();
    let stored = engine.keys(None).unwrap().remove(0);
    store_raw_payload(&database, &stored, &forged(name), "raw");

    let before = proc_status_kib("VmHWM:").unwrap();
    let result = engine.get(&file);
    let after = proc_status_kib("VmHWM:").unwrap();
    let outcome = match result {
        Err(LocalFileCacheError::Serialization(message)) => {
            format!("Err(Serialization({message:?}))")
        }
        Err(other) => format!("Err({other:?})"),
        Ok(_) => "Ok".to_owned(),
    };
    format!(
        "D7 type={name} outcome={outcome} rss_growth_kib={}",
        after.saturating_sub(before)
    )
}

/// Run `probe::<T>` for the type named by the child's environment value.
fn probe_by_name(name: &str) -> String {
    match name {
        "Vec<f32>" => probe::<Vec<f32>>(name),
        "Vec<u8>" => probe::<Vec<u8>>(name),
        "String" => probe::<String>(name),
        "Vec<String>" => probe::<Vec<String>>(name),
        "HashMap<u64,u64>" => probe::<HashMap<u64, u64>>(name),
        "Vec<u64>" => probe::<Vec<u64>>(name),
        "HashMap<String,String>" => probe::<HashMap<String, String>>(name),
        "Option<String>" => probe::<Option<String>>(name),
        "Box<str>" => probe::<Box<str>>(name),
        "PathBuf" => probe::<std::path::PathBuf>(name),
        "Vec<()>" => probe::<Vec<()>>(name),
        "HashMap<(),()>" => probe::<HashMap<(), ()>>(name),
        "List" => probe::<List>(name),
        other => panic!("unknown probe type {other}"),
    }
}

/// Types whose length claim is refused with an error. Serde's own sequence
/// visitors never reserve more than a small, fixed amount for a claim, and the
/// owned codec (RFC 028) checks every count and length against the bytes left.
const REFUSED: [&str; 4] = ["Vec<f32>", "Vec<u8>", "Vec<String>", "HashMap<u64,u64>"];

/// Types whose length claim used to abort the process (`bincode` 2's own
/// `Decode` for `String` and byte buffers reserved `len` bytes before checking
/// that they were there); fixed by RFC 028.
const FORMERLY_ABORTING: [&str; 1] = ["String"];

/// Further types that share a decode path with the above, or whose claim is a
/// count of elements that occupy no bytes (RFC 028 R5).
const ALSO_REFUSED: [&str; 8] = [
    "Vec<u64>",
    "HashMap<String,String>",
    "Option<String>",
    "Box<str>",
    "PathBuf",
    "Vec<()>",
    "HashMap<(),()>",
    "String",
];

/// One child per type, so that a type that aborts is reported on its own.
/// Returns one description per type that did **not** end in an error, or whose
/// resident set grew by `max_growth_kib` or more while reading it.
fn children_that_did_not_end_in_an_error(
    test: &str,
    types: &[&str],
    max_growth_kib: u64,
) -> Vec<String> {
    let mut bad = Vec::new();
    for name in types {
        let output = common::run_self_with_address_limit(test, CEILING_KIB, &[(CHILD, name)]);
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        // With `--nocapture` libtest prints our line after "test <name> ... ".
        let line = stdout
            .lines()
            .find_map(|l| l.find("D7 type=").map(|at| l[at..].to_owned()));
        let description = format!(
            "{name}: status={:?} {}{}",
            output.status,
            line.clone().unwrap_or_default(),
            stderr
                .lines()
                .find(|l| l.contains("memory allocation"))
                .map(|l| format!(" stderr={l}"))
                .unwrap_or_default()
        );
        println!("{description}");
        let ended_in_error = output.status.success()
            && line.is_some_and(|l| {
                l.contains("outcome=Err(Serialization(")
                    && l.rsplit("rss_growth_kib=")
                        .next()
                        .and_then(|n| n.trim().parse::<u64>().ok())
                        .is_some_and(|growth| growth < max_growth_kib)
            });
        if !ended_in_error {
            bad.push(description);
        }
    }
    bad
}

/// Growth allowed while reading a few bytes that claim a lot.
const CLAIM_GROWTH_KIB: u64 = 8 * 1024;

#[test]
fn a_length_claim_is_an_error_not_an_allocation() {
    const NAME: &str = "a_length_claim_is_an_error_not_an_allocation";
    if let Ok(name) = std::env::var(CHILD) {
        println!("{}", probe_by_name(&name));
        return;
    }
    let types: Vec<&str> = REFUSED.iter().chain(&ALSO_REFUSED).copied().collect();
    let bad = children_that_did_not_end_in_an_error(NAME, &types, CLAIM_GROWTH_KIB);
    assert!(
        bad.is_empty(),
        "length claims that did not end in an error:\n{}",
        bad.join("\n")
    );
}

/// The Known issue of v0.22.0, closed in v0.22.1 (RFC 028): a forged `String`
/// length used to exit with status 134, "memory allocation of
/// 2305843009213693951 bytes failed".
#[test]
fn a_string_length_claim_is_an_error_not_an_allocation() {
    const NAME: &str = "a_string_length_claim_is_an_error_not_an_allocation";
    if let Ok(name) = std::env::var(CHILD) {
        println!("{}", probe_by_name(&name));
        return;
    }
    let bad = children_that_did_not_end_in_an_error(NAME, &FORMERLY_ABORTING, CLAIM_GROWTH_KIB);
    assert!(
        bad.is_empty(),
        "length claims that did not end in an error:\n{}",
        bad.join("\n")
    );
}

/// RFC 028 R2.2: ten megabytes of variant indices claim 2.6 million levels of
/// nesting. `bincode` recursed once per level and overflowed the stack of every
/// thread that was measured; the owned codec stops at its depth limit.
#[test]
fn a_ten_megabyte_chain_of_variants_is_an_error_not_a_stack_overflow() {
    const NAME: &str = "a_ten_megabyte_chain_of_variants_is_an_error_not_a_stack_overflow";
    if let Ok(name) = std::env::var(CHILD) {
        println!("{}", probe_by_name(&name));
        return;
    }
    // Reading the 10 MiB blob itself is the growth that is expected here.
    let bad = children_that_did_not_end_in_an_error(NAME, &["List"], 64 * 1024);
    assert!(
        bad.is_empty(),
        "a deep chain that did not end in an error:\n{}",
        bad.join("\n")
    );
}

/// `serde_json` cannot be told a size in advance, so there is no claim to
/// forge; the nearest equivalent is nesting, which its recursion limit turns
/// into an error rather than a stack overflow.
#[cfg(feature = "json")]
#[test]
fn a_deeply_nested_json_document_is_an_error_not_a_stack_overflow() {
    use localcache::Codec;

    let directory = TempDir::new().unwrap();
    let database = directory.path().join("nested.sqlite3");
    let file = write_file(&directory, "n.txt", b"x");
    let engine: CacheEngine<serde_json::Value> = CacheEngine::builder()
        .database(&database)
        .codec(Codec::Json)
        .build()
        .unwrap();
    engine.set(&file, &serde_json::json!(1)).unwrap();
    let stored = engine.keys(None).unwrap().remove(0);
    store_raw_payload(&database, &stored, &vec![b'['; 1_000_000], "json");
    assert!(matches!(
        engine.get(&file),
        Err(LocalFileCacheError::Serialization(_))
    ));
}
