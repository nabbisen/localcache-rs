#![no_main]
//! The bincode-1 decoder, through the public API: arbitrary bytes under the
//! tag `"raw"`, decoded into types that have every shape the decoder handles.
//!
//! Two properties. A decode never panics, aborts or overflows the stack (any
//! outcome but a crash is acceptable). And a value that decoded can be encoded
//! again, and that encoding decodes to a value that encodes to the same bytes.
use std::collections::BTreeMap;

use libfuzzer_sys::fuzz_target;
use localcache_format::{Codec, PayloadFormat};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
enum Kind {
    Unit,
    New(u32),
    Tuple(i8, String),
    Struct { a: Vec<u16>, b: Option<Box<Kind>> },
}

#[derive(Debug, Serialize, Deserialize)]
struct Zoo {
    flag: bool,
    small: i8,
    wide: u128,
    float: f64,
    ch: char,
    text: String,
    bytes: Vec<u8>,
    seq: Vec<Vec<String>>,
    opt: Option<Option<u32>>,
    tup: (u8, i64, String),
    arr: [u16; 4],
    map: BTreeMap<String, Vec<u32>>,
    keyed: BTreeMap<u8, Kind>,
    kinds: Vec<Kind>,
    unit: (),
}

fn decode<T: for<'de> Deserialize<'de>>(format: &PayloadFormat, data: &[u8]) -> Option<T> {
    format
        .decode(data, "raw", "/p", None)
        .ok()
}

/// A value that decoded must encode, and encoding must be stable.
fn settles<T: Serialize + for<'de> Deserialize<'de>>(format: &PayloadFormat, value: &T) {
    let first = format
        .encode(value, "/p", None)
        .expect("a value that decoded must encode");
    let back: T = format
        .decode(&first.bytes, first.tag, "/p", None)
        .expect("what was just encoded must decode");
    let second = format
        .encode(&back, "/p", None)
        .expect("a value that decoded must encode");
    assert_eq!(first.bytes, second.bytes, "the encoding is not stable");
}

fuzz_target!(|data: &[u8]| {
    let format = PayloadFormat::new(Codec::Bincode);
    if let Some(zoo) = decode::<Zoo>(&format, data) {
        settles(&format, &zoo);
    }
    if let Some(deep) = decode::<Vec<Vec<Vec<u8>>>>(&format, data) {
        settles(&format, &deep);
    }
    if let Some(kind) = decode::<Kind>(&format, data) {
        settles(&format, &kind);
    }
});
