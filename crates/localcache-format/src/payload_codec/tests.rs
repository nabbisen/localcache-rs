//! RFC 028 R6: the golden vectors, the hostile inputs and a round-trip property.
//!
//! The vectors were written by `bincode` 2.0.1 (see
//! `tests/fixtures/payload_codec/README.md`); this module never depends on it.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use super::{DEPTH_LIMIT, Error, from_slice, to_vec};

#[path = "../../tests/fixtures/payload_codec/zoo.rs"]
mod zoo;

const VECTORS: &str = include_str!("../../tests/fixtures/payload_codec/vectors.txt");

struct Owned;

impl zoo::ZooCodec for Owned {
    fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
        to_vec(value).map_err(|e| e.to_string())
    }

    fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
        from_slice(bytes).map_err(|e| e.to_string())
    }
}

fn from_hex(text: &str) -> Vec<u8> {
    assert!(
        text.len().is_multiple_of(2),
        "odd number of hex digits in {text:?}"
    );
    (0..text.len())
        .step_by(2)
        .map(|at| {
            u8::from_str_radix(&text[at..at + 2], 16)
                .unwrap_or_else(|e| panic!("bad hex in {text:?}: {e}"))
        })
        .collect()
}

fn vectors() -> BTreeMap<String, Vec<u8>> {
    let mut vectors = BTreeMap::new();
    for line in VECTORS.lines() {
        let (name, hex) = line
            .split_once(" =")
            .unwrap_or_else(|| panic!("malformed vector line {line:?}"));
        let previous = vectors.insert(name.to_owned(), from_hex(hex.trim()));
        assert!(previous.is_none(), "duplicate vector {name}");
    }
    vectors
}

// ---------------------------------------------------------------------------
// R6.1 — the golden vectors
// ---------------------------------------------------------------------------

#[test]
fn the_zoo_and_the_vectors_name_the_same_cases() {
    let mut zoo_names = Vec::new();
    zoo::each::<Owned>(&mut |case| zoo_names.push(case.name.to_owned()));
    let vector_names: Vec<String> = vectors().into_keys().collect();
    zoo_names.sort();
    assert_eq!(zoo_names, vector_names);
}

#[test]
fn every_case_encodes_to_its_golden_vector() {
    let vectors = vectors();
    let mut wrong = Vec::new();
    zoo::each::<Owned>(&mut |case| {
        let expected = &vectors[case.name];
        match &case.encoded {
            Ok(bytes) if bytes == expected => {}
            other => wrong.push(format!("{}: {other:?} != {expected:?}", case.name)),
        }
    });
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn every_golden_vector_decodes_to_its_value_and_back() {
    let vectors = vectors();
    let mut wrong = Vec::new();
    zoo::each::<Owned>(&mut |case| {
        let expected = &vectors[case.name];
        match (case.decode)(expected) {
            // `reencoded` is what compares a NaN's payload bits.
            Ok(decoded) if decoded.same_value && decoded.reencoded.as_ref() == Ok(expected) => {}
            Ok(decoded) => wrong.push(format!(
                "{}: same_value={} reencoded={:?}",
                case.name, decoded.same_value, decoded.reencoded
            )),
            Err(error) => wrong.push(format!("{}: {error}", case.name)),
        }
    });
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn every_proper_prefix_of_every_golden_vector_is_an_error() {
    let vectors = vectors();
    let mut accepted = Vec::new();
    zoo::each::<Owned>(&mut |case| {
        let bytes = &vectors[case.name];
        for end in 0..bytes.len() {
            if (case.decode)(&bytes[..end]).is_ok() {
                accepted.push(format!("{}: {end} of {} bytes", case.name, bytes.len()));
            }
        }
    });
    assert!(
        accepted.is_empty(),
        "prefixes accepted:\n{}",
        accepted.join("\n")
    );
}

#[test]
fn a_mutated_golden_vector_never_panics_or_aborts() {
    // A mutated length prefix is the usual way to ask for a huge allocation; an
    // abort here would take the test process down with it.
    let vectors = vectors();
    zoo::each::<Owned>(&mut |case| {
        let bytes = &vectors[case.name];
        // Under Miri (about a hundred times slower) every seventh position and
        // the two extreme bytes: the same property, a sample of the same cases.
        let (step, replacements): (usize, &[u8]) = if cfg!(miri) {
            (7, &[0x00, 0xff])
        } else {
            (1, &[0x00, 0x01, 0x7f, 0x80, 0xff])
        };
        for at in (0..bytes.len()).step_by(step) {
            for &replacement in replacements {
                let mut mutated = bytes.clone();
                mutated[at] = replacement;
                let _ = (case.decode)(&mutated);
            }
        }
    });
}

// ---------------------------------------------------------------------------
// R2 — hostile input
// ---------------------------------------------------------------------------

fn claim(count: u64, tail: &[u8]) -> Vec<u8> {
    let mut bytes = count.to_le_bytes().to_vec();
    bytes.extend_from_slice(tail);
    bytes
}

#[test]
fn a_length_that_claims_more_than_remains_is_an_error_not_an_allocation() {
    // The Known issue of v0.22.0: `u64::MAX / 8` as a `String`'s length aborted.
    for count in [u64::MAX / 8, u64::MAX, 1 << 60, 1 << 32, 5] {
        let bytes = claim(count, &[1, 2, 3, 4]);
        assert!(
            from_slice::<String>(&bytes).is_err(),
            "String, claim {count}"
        );
        assert!(
            from_slice::<Box<str>>(&bytes).is_err(),
            "Box<str>, claim {count}"
        );
        assert!(
            from_slice::<std::path::PathBuf>(&bytes).is_err(),
            "PathBuf, claim {count}"
        );
        assert!(
            from_slice::<zoo::ByteBuf>(&bytes).is_err(),
            "byte buffer, claim {count}"
        );
        assert!(
            from_slice::<Vec<u8>>(&bytes).is_err(),
            "Vec<u8>, claim {count}"
        );
        assert!(
            from_slice::<Vec<u64>>(&bytes).is_err(),
            "Vec<u64>, claim {count}"
        );
        assert!(
            from_slice::<Vec<String>>(&bytes).is_err(),
            "Vec<String>, claim {count}"
        );
        assert!(
            from_slice::<HashMap<u64, u64>>(&bytes).is_err(),
            "HashMap, claim {count}"
        );
        assert!(
            from_slice::<BTreeMap<String, String>>(&bytes).is_err(),
            "BTreeMap, claim {count}"
        );
        let option = [&[1_u8][..], &bytes].concat();
        assert!(
            from_slice::<Option<String>>(&option).is_err(),
            "Option<String>, claim {count}"
        );
    }
}

#[test]
fn a_count_just_over_what_remains_is_an_error_and_a_count_that_fits_is_not() {
    // Three one-byte elements need three bytes.
    assert_eq!(
        from_slice::<Vec<u8>>(&claim(3, &[7, 8, 9])).ok(),
        Some(vec![7, 8, 9])
    );
    assert!(matches!(
        from_slice::<Vec<u8>>(&claim(4, &[7, 8, 9])),
        Err(Error::CountExceedsInput {
            claimed: 4,
            remaining: 3
        })
    ));
    assert!(matches!(
        from_slice::<String>(&claim(4, b"abc")),
        Err(Error::UnexpectedEnd {
            needed: 4,
            remaining: 3
        })
    ));
}

#[test]
fn a_length_that_does_not_fit_usize_is_an_error() {
    // On a 64-bit target every `u64` fits, so only the count check applies; on a
    // 32-bit one this is `LengthOverflow`. Either way: an error, no allocation.
    assert!(from_slice::<Vec<u8>>(&claim(u64::MAX, &[])).is_err());
    assert!(from_slice::<String>(&claim(u64::MAX, &[])).is_err());
}

#[test]
fn zero_sized_elements_are_refused_when_the_count_exceeds_the_bytes_left() {
    // RFC 028 R5: `bincode` accepted this and, for a forged count, looped for
    // 2^64 iterations.
    assert!(matches!(
        from_slice::<Vec<()>>(&claim(10, &[])),
        Err(Error::CountExceedsInput {
            claimed: 10,
            remaining: 0
        })
    ));
    assert!(from_slice::<Vec<()>>(&claim(u64::MAX, &[])).is_err());
    assert!(from_slice::<HashMap<(), ()>>(&claim(u64::MAX, &[])).is_err());
    assert!(from_slice::<Vec<zoo::UnitStruct>>(&claim(1 << 40, &[0; 8])).is_err());
    // A count the remaining bytes cover is accepted, and so is none at all.
    assert_eq!(
        from_slice::<Vec<()>>(&claim(3, &[0, 0, 0])).ok(),
        Some(vec![(); 3])
    );
    assert_eq!(from_slice::<Vec<()>>(&claim(0, &[])).ok(), Some(vec![]));
}

#[test]
fn invalid_tags_characters_strings_and_variants_are_errors() {
    assert!(matches!(
        from_slice::<bool>(&[2]),
        Err(Error::InvalidBool(2))
    ));
    assert!(matches!(
        from_slice::<Option<u8>>(&[2, 0]),
        Err(Error::InvalidOptionTag(2))
    ));
    for bytes in [
        &[0x80_u8][..],
        &[0xff],
        &[0xc0, 0x80],
        &[0xc1, 0xbf],
        &[0xe0, 0x80, 0x80],
        &[0xed, 0xa0, 0x80],
        &[0xf4, 0x90, 0x80, 0x80],
        &[0xf8],
        &[0xc3, 0x28],
        &[0xc3],
        &[0xe2, 0x82],
        &[],
    ] {
        assert!(from_slice::<char>(bytes).is_err(), "char from {bytes:02x?}");
    }
    assert_eq!(from_slice::<char>(&[0x61, 0x62]).ok(), Some('a'));
    for bytes in [&[0xff_u8][..], &[0xc0, 0xaf], &[0xed, 0xa0, 0x80]] {
        let text = claim(bytes.len() as u64, bytes);
        assert!(
            matches!(from_slice::<String>(&text), Err(Error::InvalidUtf8)),
            "{bytes:02x?}"
        );
    }
    // `Shape` has four variants.
    assert!(from_slice::<zoo::Shape>(&4_u32.to_le_bytes()).is_err());
    assert!(from_slice::<zoo::Shape>(&u32::MAX.to_le_bytes()).is_err());
    assert!(
        from_slice::<zoo::Shape>(&3_u32.to_le_bytes()).is_err(),
        "a truncated struct variant"
    );
}

#[test]
fn bytes_after_a_complete_value_are_ignored() {
    // RFC 028 R2.5 / D2: as `bincode`'s `decode_from_slice` always did.
    assert_eq!(from_slice::<u8>(&[1, 2, 3]).ok(), Some(1));
    assert_eq!(
        from_slice::<zoo::Newtype>(&[1, 2, 0xff, 0xff])
            .map(|n| n.0)
            .ok(),
        Some(0x0201)
    );
    assert_eq!(
        from_slice::<Vec<u8>>(&claim(1, &[7, 0xee])).ok(),
        Some(vec![7])
    );
}

#[test]
fn a_format_that_is_not_self_describing_says_so() {
    #[derive(Deserialize, Debug)]
    #[serde(untagged)]
    enum Untagged {
        Number(u8),
        Text(String),
    }
    #[derive(Deserialize, Debug)]
    struct Flattened {
        x: u8,
        #[serde(flatten)]
        rest: HashMap<String, u8>,
    }
    #[derive(Deserialize, Debug)]
    #[serde(tag = "kind")]
    enum Internally {
        A { v: u8 },
    }
    // Nothing decodes, so nothing is constructed; these only make the fields
    // and variants count as used.
    let _ = |value: Untagged| match value {
        Untagged::Number(n) => format!("{n}"),
        Untagged::Text(t) => t,
    };
    let _ = |value: Flattened| (value.x, value.rest);
    let _ = |value: Internally| match value {
        Internally::A { v } => v,
    };
    assert!(matches!(
        from_slice::<Untagged>(&[1]),
        Err(Error::Unsupported(_))
    ));
    assert!(from_slice::<Flattened>(&[0; 32]).is_err());
    assert!(from_slice::<Internally>(&[0; 32]).is_err());
}

#[test]
fn serde_attributes_behave_as_they_did_under_bincode() {
    // RFC 028 step 1: `bincode` writes what `skip_serializing_if` leaves and then
    // cannot read it back, and never applies `#[serde(default)]` to a missing
    // field, because the format has no field names. The owned codec must do the
    // same, not "improve" it.
    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Skip {
        a: u8,
        #[serde(skip_serializing_if = "Option::is_none")]
        b: Option<u8>,
        c: u8,
    }
    let written = to_vec(&Skip {
        a: 1,
        b: None,
        c: 3,
    })
    .unwrap_or_default();
    assert_eq!(written, [1, 3], "the skipped field is not written");
    assert!(
        from_slice::<Skip>(&written).is_err(),
        "and its tag is then read from the next field"
    );
    assert_eq!(
        from_slice::<Skip>(&[1, 0, 3]).ok(),
        Some(Skip {
            a: 1,
            b: None,
            c: 3
        })
    );

    #[derive(Deserialize, Debug, PartialEq)]
    struct Defaulted {
        a: u8,
        #[serde(default)]
        b: u8,
    }
    assert!(
        from_slice::<Defaulted>(&[1]).is_err(),
        "a missing field is truncation"
    );
    assert_eq!(
        from_slice::<Defaulted>(&[1, 2]).ok(),
        Some(Defaulted { a: 1, b: 2 })
    );
}

#[cfg(feature = "json")]
#[test]
fn a_json_value_cannot_be_stored_in_the_binary_format() {
    // It needs `deserialize_any`. It encodes (a `Null` is no bytes), and cannot
    // be read back; the engine's tests rely on this (`cache/query/tests.rs`).
    assert_eq!(
        to_vec(&serde_json::Value::Null).unwrap_or_default(),
        Vec::<u8>::new()
    );
    assert!(matches!(
        from_slice::<serde_json::Value>(&[0]),
        Err(Error::Unsupported(_))
    ));
}

#[test]
fn a_sequence_or_map_of_unknown_length_cannot_be_written() {
    struct Sequence;
    impl Serialize for Sequence {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            use serde::ser::SerializeSeq;
            let mut sequence = serializer.serialize_seq(None)?;
            sequence.serialize_element(&1_u8)?;
            sequence.end()
        }
    }
    struct Map;
    impl Serialize for Map {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            use serde::ser::SerializeMap;
            let mut map = serializer.serialize_map(None)?;
            map.serialize_entry(&1_u8, &2_u8)?;
            map.end()
        }
    }
    struct Lying;
    impl Serialize for Lying {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            use serde::ser::SerializeSeq;
            let mut sequence = serializer.serialize_seq(Some(3))?;
            sequence.serialize_element(&1_u8)?;
            sequence.end()
        }
    }
    assert!(matches!(to_vec(&Sequence), Err(Error::UnknownLength)));
    assert!(matches!(to_vec(&Map), Err(Error::UnknownLength)));
    // A length prefix that disagrees with the elements could never be read back.
    assert!(matches!(
        to_vec(&Lying),
        Err(Error::LengthMismatch {
            declared: 3,
            written: 1
        })
    ));
}

#[test]
fn an_error_message_from_a_deserialize_implementation_is_bounded() {
    struct Loud;
    impl<'de> Deserialize<'de> for Loud {
        fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            let text = String::deserialize(deserializer)?;
            Err(serde::de::Error::custom(format!("rejected {text}")))
        }
    }
    let text = "é".repeat(5000);
    let bytes = claim(text.len() as u64, text.as_bytes());
    let message = from_slice::<Loud>(&bytes)
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
    assert!(message.starts_with("rejected é"), "{message}");
    assert!(
        message.len() <= super::MESSAGE_LIMIT + '…'.len_utf8(),
        "{} bytes",
        message.len()
    );
}

// ---------------------------------------------------------------------------
// R2.2 — depth
// ---------------------------------------------------------------------------

fn list_chain(levels: usize) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(levels * 4 + 4);
    for _ in 0..levels {
        bytes.extend_from_slice(&0_u32.to_le_bytes()); // Cons
    }
    bytes.extend_from_slice(&1_u32.to_le_bytes()); // Nil
    bytes
}

#[test]
fn nesting_is_limited_to_the_depth_limit() {
    // `List` is one enum per level, and the final `Nil` is one more.
    let deepest = DEPTH_LIMIT - 1;
    assert!(from_slice::<zoo::List>(&list_chain(deepest)).is_ok());
    assert!(matches!(
        from_slice::<zoo::List>(&list_chain(deepest + 1)),
        Err(Error::TooDeep)
    ));
}

#[test]
fn a_ten_megabyte_chain_of_variant_indices_is_an_error_not_a_stack_overflow() {
    // `bincode` recursed once per four bytes here and overflowed every stack
    // that was measured (RFC 028 step 1).
    // Under Miri 64 KiB: 16,384 levels are still more than 128 times the limit.
    let size = if cfg!(miri) {
        64 * 1024
    } else {
        10 * 1024 * 1024
    };
    let bytes = list_chain(size / 4);
    assert!(matches!(
        from_slice::<zoo::List>(&bytes),
        Err(Error::TooDeep)
    ));
}

/// `count` nodes of `Node { value: u32, next: Option<Box<Node>> }`.
fn node_chain(count: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    for at in 0..count {
        bytes.extend_from_slice(&0_u32.to_le_bytes()); // value
        bytes.push(u8::from(at + 1 < count)); // Some(next), or None for the last
    }
    bytes
}

/// `count` levels of `Tree { label: String, children: Vec<Tree> }`, each with
/// an empty label and one child, and a leaf at the bottom.
fn tree_chain(count: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    for _ in 0..count {
        bytes.extend_from_slice(&0_u64.to_le_bytes()); // empty label
        bytes.extend_from_slice(&1_u64.to_le_bytes()); // one child
    }
    bytes.extend_from_slice(&0_u64.to_le_bytes()); // empty label
    bytes.extend_from_slice(&0_u64.to_le_bytes()); // no children
    bytes
}

#[test]
fn every_kind_of_recursion_is_limited_and_the_limit_is_what_the_docs_say() {
    // A level is a struct, a sequence, an enum, an `Option` that holds a value,
    // and so on, nested in another. A node is a struct and an `Option`, so
    // 64 nodes (127 levels) decode and 65 do not.
    assert!(from_slice::<zoo::Node>(&node_chain(64)).is_ok());
    assert!(matches!(
        from_slice::<zoo::Node>(&node_chain(65)),
        Err(Error::TooDeep)
    ));
    // A tree level is a struct and a sequence: 63 below the root decode.
    assert!(from_slice::<zoo::Tree>(&tree_chain(63)).is_ok());
    assert!(matches!(
        from_slice::<zoo::Tree>(&tree_chain(64)),
        Err(Error::TooDeep)
    ));
    // An enum's variant fields are not a level of their own: a struct variant that
    // holds the next value costs one level, like a newtype variant.
    #[derive(Deserialize, Debug)]
    enum Chain {
        Next { inner: Box<Chain> },
        End,
    }
    let _ = |value: Chain| match value {
        Chain::Next { inner } => Some(inner),
        Chain::End => None,
    };
    assert!(from_slice::<Chain>(&list_chain(DEPTH_LIMIT - 1)).is_ok());
    assert!(matches!(
        from_slice::<Chain>(&list_chain(DEPTH_LIMIT)),
        Err(Error::TooDeep)
    ));
    // However long the chain claims to be, the answer is an error.
    assert!(matches!(
        from_slice::<zoo::Node>(&node_chain(100_000)),
        Err(Error::TooDeep)
    ));
    assert!(matches!(
        from_slice::<zoo::Tree>(&tree_chain(100_000)),
        Err(Error::TooDeep)
    ));
    // A newtype wrapped around an option is one level each.
    #[derive(Deserialize, Debug)]
    struct Wrapped(Option<Box<Wrapped>>);
    let _ = |value: Wrapped| value.0; // only to count the field as used
    let wrapped = |count: usize| {
        let mut bytes = vec![1_u8; count];
        bytes.push(0);
        bytes
    };
    assert!(from_slice::<Wrapped>(&wrapped(63)).is_ok());
    assert!(matches!(
        from_slice::<Wrapped>(&wrapped(100_000)),
        Err(Error::TooDeep)
    ));
}

/// A value of each recursive zoo type with `count` levels of recursion.
fn chain_values(count: usize) -> (zoo::List, zoo::Node, zoo::Tree) {
    let mut list = zoo::List::Nil;
    for _ in 0..count {
        list = zoo::List::Cons(Box::new(list));
    }
    let mut node = zoo::Node {
        value: 0,
        next: None,
    };
    for value in 1..count {
        node = zoo::Node {
            value: value as u32,
            next: Some(Box::new(node)),
        };
    }
    let mut tree = zoo::Tree {
        label: String::new(),
        children: vec![],
    };
    for _ in 1..count {
        tree = zoo::Tree {
            label: String::new(),
            children: vec![tree],
        };
    }
    (list, node, tree)
}

/// Write `value` and, if that succeeded, read it back; otherwise the error must
/// be the depth error. Records the deepest `count` that could be written.
fn written_exactly_when_readable<T: Serialize + serde::de::DeserializeOwned>(
    name: &str,
    count: usize,
    value: &T,
    deepest: &mut usize,
) {
    match to_vec(value) {
        Ok(bytes) => {
            assert!(
                from_slice::<T>(&bytes).is_ok(),
                "{name} of {count} was written but not read"
            );
            *deepest = count;
        }
        Err(error) => assert!(
            matches!(error, Error::TooDeep),
            "{name} of {count}: {error}"
        ),
    }
}

#[test]
fn the_encoder_refuses_what_the_decoder_would_refuse() {
    // RFC 028 Amendment 1, F1: an engine never stores what it would refuse to
    // read. Whatever depth a value has, it can be written exactly when it can be
    // read back, and the error is the decoder's.
    let (mut deepest_list, mut deepest_node, mut deepest_tree) = (0, 0, 0);
    for count in 1..=140 {
        let (list, node, tree) = chain_values(count);
        written_exactly_when_readable("List", count, &list, &mut deepest_list);
        written_exactly_when_readable("Node", count, &node, &mut deepest_node);
        written_exactly_when_readable("Tree", count, &tree, &mut deepest_tree);
    }
    // 128 enums, 64 nodes (a struct and an option each), 64 tree levels.
    assert_eq!((deepest_list, deepest_node, deepest_tree), (127, 64, 64));
}

#[test]
fn every_kind_of_enum_level_counts_when_writing() {
    // Unit, newtype, tuple and struct variants: the enum is the level, and the
    // variant's fields add none (the decoder's rule, mirrored).
    #[derive(Serialize)]
    enum Every {
        Unit,
        Newtype(Box<Every>),
        Tuple(u8, Box<Every>),
        Struct { next: Box<Every> },
    }
    let wrapped = |levels: usize| {
        let mut value = Every::Unit;
        for level in 0..levels {
            value = match level % 3 {
                0 => Every::Newtype(Box::new(value)),
                1 => Every::Tuple(0, Box::new(value)),
                _ => Every::Struct {
                    next: Box::new(value),
                },
            };
        }
        value
    };
    // `levels` wrappers around a unit variant are `levels + 1` enums.
    assert!(to_vec(&wrapped(DEPTH_LIMIT - 1)).is_ok());
    assert!(matches!(to_vec(&wrapped(DEPTH_LIMIT)), Err(Error::TooDeep)));
    // A lone unit variant at the limit is a level too, as the decoder counts it.
    #[derive(Serialize)]
    enum Leaf {
        Unit,
        Next(Box<Leaf>),
    }
    let mut value = Leaf::Unit;
    for _ in 0..DEPTH_LIMIT {
        value = Leaf::Next(Box::new(value));
    }
    assert!(matches!(to_vec(&value), Err(Error::TooDeep)));
}

#[test]
fn counts_are_budgeted_across_the_whole_decode() {
    // RFC 028 Amendment 1, F2. `Vec<Vec<()>>`: n inner headers, each claiming the
    // most the bytes left allow. Each count alone passes the per-sequence check;
    // together they would take n^2 iterations.
    let nested = |n: usize| {
        let mut bytes = (n as u64).to_le_bytes().to_vec();
        for at in 0..n {
            bytes.extend_from_slice(&(((n - at - 1) * 8) as u64).to_le_bytes());
        }
        bytes
    };
    let started = std::time::Instant::now();
    let n = if cfg!(miri) { 2_000 } else { 16_000 };
    let refused = from_slice::<Vec<Vec<()>>>(&nested(n));
    let took = started.elapsed();
    assert!(
        matches!(refused, Err(Error::CountExceedsInput { .. })),
        "{refused:?}"
    );
    // A wall-clock bound means nothing under an interpreter; the refusal above is
    // the property, and the quadratic cost it prevents is measured natively.
    if !cfg!(miri) {
        assert!(took.as_secs() < 1, "refusing 128 KB took {took:?}");
    }
    // Honest nested payloads stay inside the budget: a count is at least one
    // byte per element, and each header is bytes of its own.
    let honest: Vec<Vec<u8>> = (0..50).map(|n| vec![n; n as usize]).collect();
    let bytes = to_vec(&honest).unwrap_or_default();
    assert_eq!(from_slice::<Vec<Vec<u8>>>(&bytes).ok(), Some(honest));
    // Zero-sized elements are allowed as long as the bytes left cover the
    // counts: three empties, backed by three bytes, in a payload of 19.
    let mut bytes = 1_u64.to_le_bytes().to_vec();
    bytes.extend_from_slice(&3_u64.to_le_bytes());
    bytes.extend_from_slice(&[0, 0, 0]);
    assert_eq!(
        from_slice::<Vec<Vec<()>>>(&bytes).ok(),
        Some(vec![vec![(); 3]])
    );
    // The budget is the input's length, not the bytes left after each header.
    let mut bytes = 2_u64.to_le_bytes().to_vec();
    bytes.extend_from_slice(&14_u64.to_le_bytes());
    bytes.extend_from_slice(&14_u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 14]);
    // 2 + 14 + 14 = 30 claimed, 38 bytes in all.
    assert_eq!(
        from_slice::<Vec<Vec<()>>>(&bytes).ok(),
        Some(vec![vec![(); 14], vec![(); 14]])
    );
}

#[test]
fn the_decoder_stack_cost_of_the_depth_limit_is_small() {
    // 128 levels must fit the smallest stack the crate runs on: a 2 MiB thread
    // (the std default, and what every `spawn_blocking` pool uses) with a good
    // margin. Run it on a 256 KiB stack, an eighth of that.
    let outcome = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let deepest = DEPTH_LIMIT - 1;
            let ok = from_slice::<zoo::List>(&list_chain(deepest)).is_ok();
            let refused = from_slice::<zoo::List>(&list_chain(deepest + 1)).is_err();
            (ok, refused)
        })
        .map(|handle| handle.join());
    assert!(matches!(outcome, Ok(Ok((true, true)))), "{outcome:?}");
}

// ---------------------------------------------------------------------------
// R6.6 — a deterministic round-trip property
// ---------------------------------------------------------------------------

/// A value of the serde data model, as a payload might nest it.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
enum Value {
    Unit,
    Flag(bool),
    Small(i8),
    Medium(u32),
    Large(i64),
    Huge(u128),
    Letter(char),
    Text(String),
    Bytes(Vec<u8>),
    Maybe(Option<Box<Value>>),
    List(Vec<Value>),
    Pair(Box<Value>, Box<Value>),
    Map(BTreeMap<String, Value>),
    Record {
        id: u64,
        body: Box<Value>,
        tags: Vec<String>,
    },
}

/// xorshift64*: small, deterministic and good enough to vary shapes.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % bound as u64).unwrap_or(0)
    }

    fn text(&mut self) -> String {
        const ALPHABET: [char; 8] = ['a', 'Z', '0', ' ', 'é', '€', '😀', '\u{0}'];
        (0..self.below(12))
            .map(|_| ALPHABET[self.below(ALPHABET.len())])
            .collect()
    }

    fn value(&mut self, depth: usize) -> Value {
        let leaf_only = depth == 0;
        match self.below(if leaf_only { 8 } else { 14 }) {
            0 => Value::Unit,
            1 => Value::Flag(self.next() & 1 == 1),
            2 => Value::Small(self.next() as i8),
            3 => Value::Medium(self.next() as u32),
            4 => Value::Large(self.next() as i64),
            5 => Value::Huge(u128::from(self.next()) << 64 | u128::from(self.next())),
            6 => Value::Letter(char::from_u32(self.below(0xd800) as u32).unwrap_or('?')),
            7 => Value::Text(self.text()),
            8 => Value::Bytes((0..self.below(20)).map(|_| self.next() as u8).collect()),
            9 => Value::Maybe(if self.next() & 1 == 1 {
                Some(Box::new(self.value(depth - 1)))
            } else {
                None
            }),
            10 => Value::List((0..self.below(5)).map(|_| self.value(depth - 1)).collect()),
            11 => Value::Pair(
                Box::new(self.value(depth - 1)),
                Box::new(self.value(depth - 1)),
            ),
            12 => Value::Map(
                (0..self.below(4))
                    .map(|_| (self.text(), self.value(depth - 1)))
                    .collect(),
            ),
            _ => Value::Record {
                id: self.next(),
                body: Box::new(self.value(depth - 1)),
                tags: (0..self.below(3)).map(|_| self.text()).collect(),
            },
        }
    }
}

#[test]
fn generated_values_round_trip_and_every_prefix_of_their_encoding_is_an_error() {
    let mut random = Random(0x9e37_79b9_7f4a_7c15);
    let cases = if cfg!(miri) { 40 } else { 3000 };
    for case in 0..cases {
        let value = random.value(6);
        let bytes = to_vec(&value).unwrap_or_else(|e| panic!("case {case}: {e}"));
        let decoded = from_slice::<Value>(&bytes).unwrap_or_else(|e| panic!("case {case}: {e}"));
        assert_eq!(decoded, value, "case {case}");
        for end in 0..bytes.len() {
            assert!(
                from_slice::<Value>(&bytes[..end]).is_err(),
                "case {case}: a prefix of {end} of {} bytes decoded",
                bytes.len()
            );
        }
    }
}
