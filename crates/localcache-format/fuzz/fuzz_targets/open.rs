#![no_main]
//! The open path: arbitrary bytes under arbitrary tags and paths, with and
//! without a key. It must never panic; the only acceptable outcomes are `Ok`
//! and an error. The tags are the twelve of the table (selected by a byte) or
//! the arbitrary string.
use libfuzzer_sys::fuzz_target;
use localcache_format::{Codec, Key, PayloadFormat};

const TAGS: [&str; 12] = [
    "raw",
    "zstd",
    "json",
    "json-zstd",
    "raw-aes256gcm",
    "zstd-aes256gcm",
    "json-aes256gcm",
    "json-zstd-aes256gcm",
    "raw-aes256gcm-bound",
    "zstd-aes256gcm-bound",
    "json-aes256gcm-bound",
    "json-zstd-aes256gcm-bound",
];

fuzz_target!(|input: (u8, bool, &str, &str, &[u8])| {
    let (selector, keyed, odd_tag, path, bytes) = input;
    let tag = TAGS.get(selector as usize).copied().unwrap_or(odd_tag);
    let key = Key::from_slice(&[0x42; 32]).expect("32 bytes");
    let key = keyed.then_some(&key);
    // A 1 MiB bound keeps one input from decompressing to the 64 MiB default.
    let format = PayloadFormat::new(Codec::Bincode).max_decompressed_bytes(1 << 20);
    let _ = format.decode::<Vec<u8>>(bytes, tag, path, key);
    let _ = format.decode::<serde_json::Value>(bytes, tag, path, key);
});
