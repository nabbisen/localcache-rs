//! RFC 025 Amendment 2, D7 — a probe, kept as a regression test.
//!
//! A crafted `raw` (bincode, legacy configuration) payload whose length prefix
//! claims `u64::MAX / 8` elements but holds a few bytes could, in principle,
//! make the deserializer reserve memory for the claim before it finds the data
//! missing. This is a different threat from decompression (the claim is inside
//! the data, and uncompressed payloads are not limited), so the probe only
//! asks: does a `get` return an error, within a small memory ceiling?
//!
//! Each payload type runs in a child process under `ulimit -v` (512 MiB of
//! address space) so that an abort or allocation failure is a failed exit
//! status here and not a dead test runner.
#![cfg(target_os = "linux")]

mod common;

use std::collections::HashMap;

use common::{proc_status_kib, store_raw_payload, write_file};
use localcache::{CacheEngine, LocalFileCacheError};
use serde::{Serialize, de::DeserializeOwned};
use tempfile::TempDir;

const CHILD: &str = "LOCALCACHE_D7_CHILD";
const CEILING_KIB: u64 = 512 * 1024;

/// A bincode (legacy) length prefix of `u64::MAX / 8`, followed by `tail`.
fn claim(tail: &[u8]) -> Vec<u8> {
    let mut bytes = (u64::MAX / 8).to_le_bytes().to_vec();
    bytes.extend_from_slice(tail);
    bytes
}

/// Store the claim as `T`'s payload and `get` it; report what happened.
fn probe<T: Serialize + DeserializeOwned + Default>(name: &str) -> String {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("claim.sqlite3");
    let file = write_file(&directory, "f.txt", b"x");
    let engine: CacheEngine<T> = CacheEngine::builder().database(&database).build().unwrap();
    engine.set(&file, &T::default()).unwrap();
    let stored = engine.keys(None).unwrap().remove(0);
    store_raw_payload(&database, &stored, &claim(&[1, 2, 3, 4]), "raw");

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
        other => panic!("unknown probe type {other}"),
    }
}

/// Types whose length claim is refused with an error: serde's own sequence
/// visitors never reserve more than a small, fixed amount for a claim.
const REFUSED: [&str; 4] = ["Vec<f32>", "Vec<u8>", "Vec<String>", "HashMap<u64,u64>"];

/// Types whose length claim aborts the process today (a Known issue in
/// v0.22.0, fixed in v0.23.0): bincode 2's own `Decode` for `String` (and for byte
/// buffers) reserves `len` bytes before it has checked that they are there.
const ABORTING: [&str; 1] = ["String"];

/// One child per type, so that a type that aborts is reported on its own.
/// Returns one description per type that did **not** end in an error.
fn children_that_did_not_end_in_an_error(test: &str, types: &[&str]) -> Vec<String> {
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
                        .is_some_and(|growth| growth < 8 * 1024)
            });
        if !ended_in_error {
            bad.push(description);
        }
    }
    bad
}

#[test]
fn a_bincode_length_claim_is_an_error_not_an_allocation() {
    const NAME: &str = "a_bincode_length_claim_is_an_error_not_an_allocation";
    if let Ok(name) = std::env::var(CHILD) {
        println!("{}", probe_by_name(&name));
        return;
    }
    let bad = children_that_did_not_end_in_an_error(NAME, &REFUSED);
    assert!(
        bad.is_empty(),
        "length claims that did not end in an error:\n{}",
        bad.join("\n")
    );
}

/// The desired behaviour for the types in [`ABORTING`], and not the current
/// one: it fails today (exit status 134, "memory allocation of
/// 2305843009213693951 bytes failed"), which is why it is ignored. Run it with
/// `cargo test --test length_claim_probe -- --ignored`. It is the **acceptance
/// test for the v0.23.0 fix** (an owned, bounds-checked decoder for the legacy
/// payload format); remove the `#[ignore]` when that lands.
#[test]
#[ignore = "known issue (ROADMAP deferred register): bincode aborts on a forged String length; fixed in v0.23.0 by an owned decoder"]
fn a_string_length_claim_is_an_error_not_an_allocation() {
    const NAME: &str = "a_string_length_claim_is_an_error_not_an_allocation";
    if let Ok(name) = std::env::var(CHILD) {
        println!("{}", probe_by_name(&name));
        return;
    }
    let bad = children_that_did_not_end_in_an_error(NAME, &ABORTING);
    assert!(
        bad.is_empty(),
        "length claims that did not end in an error:\n{}",
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
