//! RFC 029 R1: the tag table and the associated data. Property-style tests
//! use a small deterministic generator, because the crate has no random-test
//! dependency and a failing case must be reproducible from its seed.

use super::*;

const BOUND_TAGS: [&str; 4] = [
    "raw-aes256gcm-bound",
    "zstd-aes256gcm-bound",
    "json-aes256gcm-bound",
    "json-zstd-aes256gcm-bound",
];

/// xorshift64*: a fixed seed gives the same cases every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next() as u8).collect()
    }

    /// A path of random length made of ASCII, a separator, and non-ASCII text.
    fn path(&mut self) -> String {
        const PARTS: [&str; 8] = ["/", "a", "Z", "0", "é", "日本", ".", " "];
        let n = 1 + self.below(120);
        (0..n).map(|_| PARTS[self.below(PARTS.len())]).collect()
    }
}

#[test]
fn every_tag_of_the_table_parses_one_way() {
    let table = [
        ("raw", Sealing::None, "raw"),
        ("zstd", Sealing::None, "zstd"),
        ("json", Sealing::None, "json"),
        ("json-zstd", Sealing::None, "json-zstd"),
        ("raw-aes256gcm", Sealing::Legacy, "raw"),
        ("zstd-aes256gcm", Sealing::Legacy, "zstd"),
        ("json-aes256gcm", Sealing::Legacy, "json"),
        ("json-zstd-aes256gcm", Sealing::Legacy, "json-zstd"),
        ("raw-aes256gcm-bound", Sealing::Bound, "raw"),
        ("zstd-aes256gcm-bound", Sealing::Bound, "zstd"),
        ("json-aes256gcm-bound", Sealing::Bound, "json"),
        ("json-zstd-aes256gcm-bound", Sealing::Bound, "json-zstd"),
    ];
    for (tag, sealing, base) in table {
        assert_eq!(parse_tag(tag), (base, sealing), "{tag}");
        // no tag matches both suffixes
        assert!(
            !(tag.ends_with(BOUND_SUFFIX) && tag.ends_with(LEGACY_SUFFIX)),
            "{tag}"
        );
    }
}

#[test]
fn tags_are_matched_exactly_and_case_sensitively() {
    // Not a sealed tag in any way: the whole string is the (unknown) base.
    for tag in [
        "RAW-AES256GCM",
        "raw-AES256GCM-bound",
        "raw-aes256gcm-BOUND",
        "raw-aes256gcm-bound ",
        "raw-aes256gcm-boun",
        "raw-aes256gcm-bounds",
        "raw-aes256gcmbound",
        "-bound",
    ] {
        assert_eq!(parse_tag(tag), (tag, Sealing::None), "{tag:?}");
    }
    // A tag that nests the suffixes is the outer one, with an unknown base.
    assert_eq!(
        parse_tag("raw-aes256gcm-bound-aes256gcm"),
        ("raw-aes256gcm-bound", Sealing::Legacy)
    );
    assert!(!is_known_base("raw-aes256gcm-bound"));
    assert_eq!(
        parse_tag("raw-aes256gcm-aes256gcm-bound"),
        ("raw-aes256gcm", Sealing::Bound)
    );
    assert!(!is_known_base("raw-aes256gcm"));
}

#[test]
fn a_bound_tag_exists_for_every_known_base_and_only_for_those() {
    // `raw` always; the others exist only in a build that has their layers.
    assert_eq!(bound_tag("raw"), Some("raw-aes256gcm-bound"));
    assert!(is_known_base("raw"));
    for (base, tag, built) in [
        (
            "zstd",
            "zstd-aes256gcm-bound",
            cfg!(feature = "compression"),
        ),
        ("json", "json-aes256gcm-bound", cfg!(feature = "json")),
        (
            "json-zstd",
            "json-zstd-aes256gcm-bound",
            cfg!(all(feature = "json", feature = "compression")),
        ),
    ] {
        assert_eq!(bound_tag(base), built.then_some(tag), "{base}");
        assert_eq!(is_known_base(base), built, "{base}");
    }
    for base in ["nope", "", "RAW", "raw-aes256gcm"] {
        assert_eq!(bound_tag(base), None, "{base:?}");
        assert!(!is_known_base(base), "{base:?}");
    }
}

#[test]
fn the_associated_data_has_the_documented_layout() {
    let aad = associated_data("raw-aes256gcm-bound", "/data/a.txt");
    let mut expected = b"localcache/seal/1".to_vec();
    expected.push(0);
    expected.extend_from_slice(b"raw-aes256gcm-bound");
    expected.push(0);
    expected.extend_from_slice(b"/data/a.txt");
    assert_eq!(aad, expected);
}

/// For random path, tag, key and plaintext: `open(seal(x)) = x`, and a change
/// of the path, of the tag, of the key or of one bit of the ciphertext fails.
#[test]
fn property_seal_then_open_round_trips_and_any_change_fails() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    // the bound tags this build has a codec for (the property needs at least two)
    let tags: Vec<&str> = BOUND_TAGS
        .iter()
        .copied()
        .filter(|t| is_known_base(parse_tag(t).0))
        .collect();
    assert!(!tags.is_empty());
    for case in 0..200 {
        let path = rng.path();
        let tag = tags[rng.below(tags.len())];
        let mut key = [0_u8; 32];
        key.copy_from_slice(&rng.bytes(32));
        let len = rng.below(300);
        let plain = rng.bytes(len);
        let sealed = encrypt_bytes(&plain, &key, &associated_data(tag, &path)).unwrap();
        let open = |key: &[u8; 32], tag: &str, path: &str, data: &[u8]| {
            decrypt_bytes(data, key, &associated_data(tag, path))
        };
        let ctx = format!("case {case}: path {path:?}, tag {tag}");

        assert_eq!(open(&key, tag, &path, &sealed).unwrap(), plain, "{ctx}");

        // another path (a longer one, and one that differs in its last byte)
        assert!(
            open(&key, tag, &format!("{path}x"), &sealed).is_err(),
            "{ctx}"
        );
        let mut other_path = path.clone().into_bytes();
        *other_path.last_mut().unwrap() ^= 1;
        if let Ok(other_path) = String::from_utf8(other_path) {
            assert!(open(&key, tag, &other_path, &sealed).is_err(), "{ctx}");
        }
        // another tag
        // another tag (a different string even when this build knows only one)
        let other_tag = tags
            .iter()
            .copied()
            .find(|t| *t != tag)
            .unwrap_or("raw-aes256gcm");
        assert!(open(&key, other_tag, &path, &sealed).is_err(), "{ctx}");
        // another key (one bit)
        let mut other_key = key;
        other_key[rng.below(32)] ^= 1 << rng.below(8);
        assert!(open(&other_key, tag, &path, &sealed).is_err(), "{ctx}");
        // one bit of the ciphertext, anywhere: nonce, body or tag
        let mut flipped = sealed.clone();
        let at = rng.below(flipped.len());
        flipped[at] ^= 1 << rng.below(8);
        assert!(
            open(&key, tag, &path, &flipped).is_err(),
            "{ctx}, byte {at}"
        );
        // and no associated data at all
        assert!(decrypt_bytes(&sealed, &key, &[]).is_err(), "{ctx}");
    }
}

#[test]
fn encode_returns_a_bound_tag_and_decode_needs_the_same_path() {
    let key = [0x42_u8; 32];
    let value: Vec<u8> = (0..100).collect();
    let (bytes, tag) = encode_payload(
        &value,
        "/data/a.txt",
        false,
        Codec::Bincode,
        #[cfg(feature = "compression")]
        crate::DEFAULT_MAX_DECOMPRESSED_BYTES,
        Some(&key),
    )
    .unwrap();
    assert_eq!(tag, "raw-aes256gcm-bound");
    let decode = |path: &str, tag: &str| {
        decode_payload::<Vec<u8>>(
            &bytes,
            tag,
            path,
            #[cfg(feature = "compression")]
            crate::DEFAULT_MAX_DECOMPRESSED_BYTES,
            Some(&key),
        )
    };
    assert_eq!(decode("/data/a.txt", tag).unwrap(), value);
    assert!(matches!(
        decode("/data/b.txt", tag),
        Err(FormatError::EncryptionError(_))
    ));
    // the tag the payload is read under is part of what was sealed
    #[cfg(feature = "compression")]
    assert!(matches!(
        decode("/data/a.txt", "zstd-aes256gcm-bound"),
        Err(FormatError::EncryptionError(_))
    ));
}

/// RFC 029 R2: with a key, a payload that is not bound is refused, whether it
/// is stored plain or was sealed before binding; without a key a sealed tag is
/// `MissingEncryptionKey` and a plain tag is read, as before.
#[test]
fn with_a_key_only_a_bound_payload_is_read() {
    let key = [0x42_u8; 32];
    let value: Vec<u8> = (0..100).collect();
    let plain = payload_codec_bytes(&value);
    let legacy = encrypt_bytes(&plain, &key, &[]).unwrap();
    let decode = |bytes: &[u8], tag: &str, key: Option<&[u8; 32]>| {
        decode_payload::<Vec<u8>>(
            bytes,
            tag,
            "/anywhere",
            #[cfg(feature = "compression")]
            crate::DEFAULT_MAX_DECOMPRESSED_BYTES,
            key,
        )
    };
    // with a key
    assert!(matches!(
        decode(&plain, "raw", Some(&key)),
        Err(FormatError::UnsealedEntry)
    ));
    assert!(matches!(
        decode(&legacy, "raw-aes256gcm", Some(&key)),
        Err(FormatError::UnsealedEntry)
    ));
    // a bound tag on legacy bytes is an authentication failure, not a read
    assert!(matches!(
        decode(&legacy, "raw-aes256gcm-bound", Some(&key)),
        Err(FormatError::EncryptionError(_))
    ));
    // a tag this build cannot decode stays `UnknownEncoding`, whatever the key
    for tag in ["bogus", "bogus-aes256gcm", "bogus-aes256gcm-bound"] {
        assert!(
            matches!(
                decode(&plain, tag, Some(&key)),
                Err(FormatError::UnknownEncoding(_))
            ),
            "{tag}"
        );
    }
    // without a key
    assert_eq!(decode(&plain, "raw", None).unwrap(), value);
    for tag in ["raw-aes256gcm", "raw-aes256gcm-bound"] {
        assert!(
            matches!(
                decode(&legacy, tag, None),
                Err(FormatError::MissingEncryptionKey)
            ),
            "{tag}"
        );
    }
    // the refusal text names the remedy
    let text = FormatError::UnsealedEntry.to_string();
    assert!(text.contains("rotate_encryption_key"), "{text}");
    assert!(text.contains("purge"), "{text}");
}

fn payload_codec_bytes(value: &Vec<u8>) -> Vec<u8> {
    serialize_bincode(value).unwrap()
}

#[test]
fn reseal_takes_a_payload_from_any_state_to_bound_under_the_new_key() {
    let old = [0x11_u8; 32];
    let new = [0x22_u8; 32];
    let path = "/data/a.txt";
    let plain = serialize_bincode(&vec![1_u8, 2, 3]).unwrap();

    let legacy = encrypt_bytes(&plain, &old, &[]).unwrap();
    let bound = encrypt_bytes(&plain, &old, &associated_data("raw-aes256gcm-bound", path)).unwrap();
    for (content, tag) in [
        (plain.clone(), "raw"),
        (legacy, "raw-aes256gcm"),
        (bound, "raw-aes256gcm-bound"),
    ] {
        let (sealed, new_tag) = reseal(&content, tag, path, &old, &new).unwrap();
        assert_eq!(new_tag, "raw-aes256gcm-bound", "from {tag}");
        let back = decrypt_bytes(&sealed, &new, &associated_data(new_tag, path)).unwrap();
        assert_eq!(back, plain, "from {tag}");
    }

    // a bound payload resealed for another path is refused
    let bound = encrypt_bytes(&plain, &old, &associated_data("raw-aes256gcm-bound", path)).unwrap();
    assert!(reseal(&bound, "raw-aes256gcm-bound", "/data/b.txt", &old, &new).is_err());
    // a tag this build cannot decode
    assert!(matches!(
        reseal(&plain, "bogus", path, &old, &new),
        Err(FormatError::UnknownEncoding(_))
    ));
    assert!(matches!(
        reseal(&plain, "bogus-aes256gcm-bound", path, &old, &new),
        Err(FormatError::UnknownEncoding(_))
    ));
}
