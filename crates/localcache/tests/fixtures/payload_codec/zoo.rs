//! The type zoo of RFC 028 R6.1: one value for every row of the wire-format
//! table (RFC 028 R1) and for the nested combinations a payload can have.
//!
//! This file is the single source for two users, so that the vectors and the
//! test can never describe different values:
//!
//! * `generate.rs` runs it against `bincode` 2.0.1 once, offline, to write
//!   `vectors.txt` (see `README.md`);
//! * `src/serialization/payload_codec/tests.rs` runs it against the owned codec
//!   and compares with those vectors.
//!
//! It depends on `serde` (with `derive`) and `std` only. A value's identity is
//! its `Debug` text, which is also `NaN`-safe; the NaN payload bits are compared
//! through the re-encoded bytes.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fmt::Debug;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// A codec under test: encode a value, decode a value.
pub trait ZooCodec {
    fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, String>;
    fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, String>;
}

/// What decoding some bytes into a case's type produced.
pub struct Decoded {
    /// The decoded value prints exactly like the case's value.
    pub same_value: bool,
    /// The decoded value, encoded again.
    pub reencoded: Result<Vec<u8>, String>,
}

/// One zoo entry, for one codec.
pub struct Case<'a> {
    pub name: &'static str,
    /// The case's value, encoded.
    pub encoded: Result<Vec<u8>, String>,
    /// Decode arbitrary bytes into the case's type.
    pub decode: &'a dyn Fn(&[u8]) -> Result<Decoded, String>,
}

fn case<C: ZooCodec, T>(visit: &mut dyn FnMut(Case<'_>), name: &'static str, value: T)
where
    T: Serialize + DeserializeOwned + Debug,
{
    let expected = format!("{value:?}");
    let decode = |bytes: &[u8]| -> Result<Decoded, String> {
        let decoded: T = C::decode(bytes)?;
        Ok(Decoded {
            same_value: format!("{decoded:?}") == expected,
            reencoded: C::encode(&decoded),
        })
    };
    visit(Case {
        name,
        encoded: C::encode(&value),
        decode: &decode,
    });
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Debug)]
pub struct UnitStruct;

#[derive(Serialize, Deserialize, Debug)]
pub struct Newtype(pub u16);

#[derive(Serialize, Deserialize, Debug)]
pub struct TupleStruct(pub u8, pub u32);

#[derive(Serialize, Deserialize, Debug)]
pub struct Named {
    pub a: u8,
    pub b: String,
    pub c: Option<u16>,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Shape {
    Unit,
    Newtype(u8),
    Tuple(u8, u16),
    Struct { x: u8, y: String },
}

/// Recursion through an enum (`Box`), as in RFC 028 R2.2.
#[derive(Serialize, Deserialize, Debug)]
pub enum List {
    Cons(Box<List>),
    Nil,
}

/// Recursion through `Option<Box<_>>`.
#[derive(Serialize, Deserialize, Debug)]
pub struct Node {
    pub value: u32,
    pub next: Option<Box<Node>>,
}

/// Recursion through a `Vec`.
#[derive(Serialize, Deserialize, Debug)]
pub struct Tree {
    pub label: String,
    pub children: Vec<Tree>,
}

/// `serialize_bytes` / `deserialize_byte_buf`, which `Vec<u8>` does not use.
#[derive(Debug)]
pub struct ByteBuf(pub Vec<u8>);

impl Serialize for ByteBuf {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&self.0)
    }
}

impl<'de> Deserialize<'de> for ByteBuf {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl serde::de::Visitor<'_> for Visitor {
            type Value = ByteBuf;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a byte buffer")
            }
            fn visit_byte_buf<E: serde::de::Error>(self, v: Vec<u8>) -> Result<ByteBuf, E> {
                Ok(ByteBuf(v))
            }
        }
        deserializer.deserialize_byte_buf(Visitor)
    }
}

/// Everything at once, as an application payload might.
#[derive(Serialize, Deserialize, Debug)]
pub struct Kitchen {
    pub id: u64,
    pub name: String,
    pub tags: Vec<String>,
    pub scores: BTreeMap<String, Vec<f32>>,
    pub shape: Shape,
    pub maybe: Option<Option<Named>>,
    pub pair: (i8, char),
    pub flags: [bool; 3],
    pub nested: Vec<Option<(u8, String)>>,
}

fn kitchen() -> Kitchen {
    let mut scores = BTreeMap::new();
    scores.insert("a".to_string(), vec![1.5, -2.5]);
    scores.insert("b".to_string(), vec![]);
    Kitchen {
        id: 0x0102_0304_0506_0708,
        name: "caf\u{e9} \u{20ac} \u{1f600}".to_string(),
        tags: vec!["x".to_string(), String::new()],
        scores,
        shape: Shape::Struct {
            x: 9,
            y: "why".to_string(),
        },
        maybe: Some(Some(Named {
            a: 1,
            b: "b".to_string(),
            c: None,
        })),
        pair: (-3, '\u{e9}'),
        flags: [true, false, true],
        nested: vec![Some((7, "seven".to_string())), None],
    }
}

fn list(levels: usize) -> List {
    let mut list = List::Nil;
    for _ in 0..levels {
        list = List::Cons(Box::new(list));
    }
    list
}

fn nodes(count: u32) -> Option<Box<Node>> {
    let mut next = None;
    for value in (0..count).rev() {
        next = Some(Box::new(Node { value, next }));
    }
    next
}

/// A small deterministic pseudo-random sequence for the bulky cases.
fn pseudo(seed: u32, count: usize) -> Vec<u32> {
    let mut state = seed;
    (0..count)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            state
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The zoo
// ---------------------------------------------------------------------------

/// Visit every case of the zoo, in a fixed order.
pub fn each<C: ZooCodec>(visit: &mut dyn FnMut(Case<'_>)) {
    let v = visit;

    // Scalars.
    case::<C, _>(v, "bool/false", false);
    case::<C, _>(v, "bool/true", true);
    case::<C, _>(v, "u8", 0xabu8);
    case::<C, _>(v, "u8/max", u8::MAX);
    case::<C, _>(v, "i8/-2", -2i8);
    case::<C, _>(v, "i8/min", i8::MIN);
    case::<C, _>(v, "u16", 0x0102u16);
    case::<C, _>(v, "u16/max", u16::MAX);
    case::<C, _>(v, "i16/-2", -2i16);
    case::<C, _>(v, "i16/min", i16::MIN);
    case::<C, _>(v, "u32", 1u32);
    case::<C, _>(v, "u32/max", u32::MAX);
    case::<C, _>(v, "i32/-2", -2i32);
    case::<C, _>(v, "i32/min", i32::MIN);
    case::<C, _>(v, "u64", 0x0102_0304_0506_0708u64);
    case::<C, _>(v, "u64/max", u64::MAX);
    case::<C, _>(v, "i64/-2", -2i64);
    case::<C, _>(v, "i64/min", i64::MIN);
    case::<C, _>(v, "u128/1", 1u128);
    case::<C, _>(v, "u128/max", u128::MAX);
    case::<C, _>(
        v,
        "u128/mixed",
        0x0102_0304_0506_0708_090a_0b0c_0d0e_0f10u128,
    );
    case::<C, _>(v, "i128/-2", -2i128);
    case::<C, _>(v, "i128/min", i128::MIN);
    case::<C, _>(v, "usize", 5usize);
    case::<C, _>(v, "usize/u32-max", 0xffff_ffffusize);
    case::<C, _>(v, "isize/-1", -1isize);
    case::<C, _>(v, "f32/1.0", 1.0f32);
    case::<C, _>(v, "f32/-0.0", -0.0f32);
    case::<C, _>(v, "f32/infinity", f32::INFINITY);
    case::<C, _>(v, "f32/min-subnormal", f32::from_bits(1));
    case::<C, _>(v, "f32/nan-payload", f32::from_bits(0x7fc1_2345));
    case::<C, _>(v, "f32/signalling-nan", f32::from_bits(0x7f80_0001));
    case::<C, _>(v, "f64/1.0", 1.0f64);
    case::<C, _>(v, "f64/-1.5e300", -1.5e300f64);
    case::<C, _>(v, "f64/nan-payload", f64::from_bits(0x7ff8_0000_dead_beef));

    // `char`: one to four bytes, and the boundaries between the lengths.
    case::<C, _>(v, "char/a", 'a');
    case::<C, _>(v, "char/U+0000", '\0');
    case::<C, _>(v, "char/U+007F", '\u{7f}');
    case::<C, _>(v, "char/U+0080", '\u{80}');
    case::<C, _>(v, "char/U+00E9", '\u{e9}');
    case::<C, _>(v, "char/U+07FF", '\u{7ff}');
    case::<C, _>(v, "char/U+0800", '\u{800}');
    case::<C, _>(v, "char/U+20AC", '\u{20ac}');
    case::<C, _>(v, "char/U+D7FF", '\u{d7ff}');
    case::<C, _>(v, "char/U+E000", '\u{e000}');
    case::<C, _>(v, "char/U+FFFF", '\u{ffff}');
    case::<C, _>(v, "char/U+10000", '\u{10000}');
    case::<C, _>(v, "char/U+1F600", '\u{1f600}');
    case::<C, _>(v, "char/U+10FFFF", '\u{10ffff}');

    // Strings and bytes.
    case::<C, _>(v, "string/empty", String::new());
    case::<C, _>(v, "string/ab", "ab".to_string());
    case::<C, _>(v, "string/multibyte", "\u{e9}\u{20ac}\u{1f600}".to_string());
    case::<C, _>(v, "string/300-ascii", "0123456789".repeat(30));
    case::<C, _>(v, "string/box-str", "hi".to_string().into_boxed_str());
    case::<C, _>(
        v,
        "string/cow",
        std::borrow::Cow::<'static, str>::Owned("q".to_string()),
    );
    case::<C, _>(v, "string/path-buf", std::path::PathBuf::from("/a/b"));
    case::<C, _>(v, "bytes/serialize-bytes", ByteBuf(vec![1, 2, 3]));
    case::<C, _>(v, "bytes/serialize-bytes-empty", ByteBuf(vec![]));
    case::<C, _>(v, "bytes/vec-u8", vec![1u8, 2, 3]);
    case::<C, _>(
        v,
        "bytes/vec-u8-1000",
        (0..1000u32).map(|i| (i % 251) as u8).collect::<Vec<u8>>(),
    );

    // Option, unit, newtype, tuples, arrays, structs.
    case::<C, _>(v, "option/none", None::<u8>);
    case::<C, _>(v, "option/some", Some(7u8));
    case::<C, _>(v, "option/none-of-option", None::<Option<u8>>);
    case::<C, _>(v, "option/some-none", Some(None::<u8>));
    case::<C, _>(v, "option/some-some", Some(Some(3u8)));
    case::<C, _>(v, "option/some-string", Some("s".to_string()));
    case::<C, _>(v, "unit", ());
    case::<C, _>(v, "unit-struct", UnitStruct);
    case::<C, _>(v, "newtype", Newtype(0x0102));
    case::<C, _>(v, "newtype/box", Box::new(0x0102u16));
    case::<C, _>(v, "tuple/u8-u16", (1u8, 2u16));
    case::<C, _>(v, "tuple/one", (9u8,));
    case::<C, _>(v, "tuple-struct", TupleStruct(1, 2));
    case::<C, _>(v, "array/u8-3", [1u8, 2, 3]);
    case::<C, _>(v, "array/u16-2", [1u16, 2]);
    case::<C, _>(v, "array/u8-0", [0u8; 0]);
    case::<C, _>(v, "array/f32-4", [1.0f32, 2.0, 3.0, 4.0]);
    case::<C, _>(
        v,
        "struct/named",
        Named {
            a: 1,
            b: "x".to_string(),
            c: Some(2),
        },
    );
    case::<C, _>(
        v,
        "struct/named-none",
        Named {
            a: 0,
            b: String::new(),
            c: None,
        },
    );

    // Enums.
    case::<C, _>(v, "enum/unit-variant", Shape::Unit);
    case::<C, _>(v, "enum/newtype-variant", Shape::Newtype(9));
    case::<C, _>(v, "enum/tuple-variant", Shape::Tuple(1, 2));
    case::<C, _>(
        v,
        "enum/struct-variant",
        Shape::Struct {
            x: 1,
            y: "z".to_string(),
        },
    );
    case::<C, _>(v, "enum/result-ok", Ok::<u8, u8>(1));
    case::<C, _>(v, "enum/result-err", Err::<u8, u8>(2));
    case::<C, _>(v, "enum/option-of-enum", Some(Shape::Tuple(1, 2)));
    case::<C, _>(v, "enum/vec-of-enum", vec![Shape::Unit, Shape::Newtype(1)]);

    // Sequences and maps.
    case::<C, _>(v, "seq/vec-u16", vec![1u16, 2]);
    case::<C, _>(v, "seq/vec-empty", Vec::<u16>::new());
    case::<C, _>(v, "seq/vec-string-empty", Vec::<String>::new());
    case::<C, _>(v, "seq/vec-f32-3", vec![1.0f32, 2.0, 3.0]);
    case::<C, _>(
        v,
        "seq/vec-f32-384",
        pseudo(7, 384)
            .into_iter()
            .map(|n| n as f32 / 4096.0)
            .collect::<Vec<f32>>(),
    );
    case::<C, _>(
        v,
        "seq/vec-u64-100",
        pseudo(11, 100)
            .into_iter()
            .map(u64::from)
            .collect::<Vec<u64>>(),
    );
    case::<C, _>(v, "seq/vec-vec-u8", vec![vec![1u8], vec![]]);
    case::<C, _>(
        v,
        "seq/vec-option-tuple",
        vec![Some((1u8, "a".to_string())), None],
    );
    case::<C, _>(
        v,
        "seq/vec-string",
        vec!["a".to_string(), "\u{e9}".to_string()],
    );
    case::<C, _>(
        v,
        "seq/vec-tuple-string-u64",
        vec![("k".to_string(), 1u64), ("l".to_string(), 2)],
    );
    case::<C, _>(v, "seq/vec-deque", VecDeque::from(vec![1u8, 2, 3]));
    case::<C, _>(v, "seq/btree-set", BTreeSet::from([3u8, 1, 2]));
    case::<C, _>(v, "seq/hash-set-one", HashSet::from([5u8]));
    case::<C, _>(v, "map/btree-u8-u8", BTreeMap::from([(1u8, 2u8), (3, 4)]));
    case::<C, _>(v, "map/btree-empty", BTreeMap::<u8, u8>::new());
    case::<C, _>(v, "map/hash-one", HashMap::from([(1u8, "a".to_string())]));
    case::<C, _>(v, "map/hash-u64-u64-one", HashMap::from([(7u64, 8u64)]));
    case::<C, _>(
        v,
        "map/btree-string-vec",
        BTreeMap::from([("a".to_string(), vec![1u32, 2]), ("b".to_string(), vec![])]),
    );

    // Standard library types with their own serde shapes.
    case::<C, _>(v, "std/duration", std::time::Duration::new(5, 7));
    case::<C, _>(v, "std/ipv4", std::net::Ipv4Addr::new(1, 2, 3, 4));
    case::<C, _>(
        v,
        "std/ipv6",
        std::net::Ipv6Addr::new(1, 0, 0, 0, 0, 0, 0, 2),
    );
    case::<C, _>(
        v,
        "std/ip-addr-v4",
        std::net::IpAddr::V4(std::net::Ipv4Addr::new(1, 2, 3, 4)),
    );
    case::<C, _>(v, "std/wrapping", std::num::Wrapping(3u8));
    case::<C, _>(v, "std/range", 1u8..3u8);

    // Nesting and bulk.
    case::<C, _>(v, "nested/kitchen-sink", kitchen());
    case::<C, _>(v, "nested/list-0", list(0));
    case::<C, _>(v, "nested/list-3", list(3));
    case::<C, _>(v, "nested/list-40", list(40));
    case::<C, _>(v, "nested/nodes-0", nodes(0));
    case::<C, _>(v, "nested/nodes-5", nodes(5));
    case::<C, _>(
        v,
        "nested/tree",
        Tree {
            label: "root".to_string(),
            children: vec![
                Tree {
                    label: "a".to_string(),
                    children: vec![],
                },
                Tree {
                    label: "b".to_string(),
                    children: vec![Tree {
                        label: "c".to_string(),
                        children: vec![],
                    }],
                },
            ],
        },
    );
}
