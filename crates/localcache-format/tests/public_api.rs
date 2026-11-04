//! The crate's public API, used the way `localcache` uses it: one
//! `PayloadFormat`, `encode`, `decode`, `seal_state`, and (with `encryption`)
//! `Key` and `reseal`. The unit tests inside the crate reach the private
//! functions; these do not.

use localcache_format::{Codec, FormatError, PayloadFormat, SealState, seal_state};

fn sample() -> Vec<String> {
    vec!["alpha".to_owned(), "βeta".to_owned(), String::new()]
}

/// `(codec, compressed, the tag an unsealed payload gets)`.
fn rows() -> Vec<(Codec, bool, &'static str)> {
    vec![
        (Codec::Bincode, false, "raw"),
        #[cfg(feature = "compression")]
        (Codec::Bincode, true, "zstd"),
        #[cfg(feature = "json")]
        (Codec::Json, false, "json"),
        #[cfg(all(feature = "json", feature = "compression"))]
        (Codec::Json, true, "json-zstd"),
    ]
}

fn format_of(codec: Codec, compressed: bool) -> PayloadFormat {
    let format = PayloadFormat::new(codec);
    #[cfg(feature = "compression")]
    let format = format.compressed(compressed);
    #[cfg(not(feature = "compression"))]
    let _ = compressed;
    format
}

#[test]
fn every_row_round_trips_under_its_tag_without_a_key() {
    for (codec, compressed, tag) in rows() {
        let format = format_of(codec, compressed);
        let encoded = format
            .encode(
                &sample(),
                "/p",
                #[cfg(feature = "encryption")]
                None,
            )
            .unwrap();
        assert_eq!(encoded.tag, tag);
        assert_eq!(seal_state(encoded.tag), SealState::Plain);
        let back: Vec<String> = format
            .decode(
                &encoded.bytes,
                encoded.tag,
                "/p",
                #[cfg(feature = "encryption")]
                None,
            )
            .unwrap();
        assert_eq!(back, sample(), "{tag}");
    }
}

#[test]
fn an_unknown_tag_is_refused_whatever_else_is_true() {
    let format = PayloadFormat::new(Codec::Bincode);
    let error = format
        .decode::<Vec<String>>(
            &[],
            "no-such-tag",
            "/p",
            #[cfg(feature = "encryption")]
            None,
        )
        .unwrap_err();
    assert!(matches!(error, FormatError::UnknownEncoding(ref t) if t == "no-such-tag"));
    assert_eq!(error.to_string(), "unknown payload encoding: no-such-tag");
}

#[test]
fn the_suffixes_are_matched_exactly_and_case_sensitively() {
    assert_eq!(seal_state("raw-aes256gcm-bound"), SealState::Bound);
    assert_eq!(seal_state("raw-aes256gcm"), SealState::Unbound);
    assert_eq!(seal_state("raw-AES256GCM-bound"), SealState::Plain);
    assert_eq!(seal_state("raw"), SealState::Plain);
}

#[cfg(feature = "compression")]
#[test]
fn the_bound_is_enforced_on_write_and_on_read() {
    let big = vec![0_u8; 4096];
    let format = PayloadFormat::new(Codec::Bincode)
        .compressed(true)
        .max_decompressed_bytes(1024);
    let refused = format
        .encode(
            &big,
            "/p",
            #[cfg(feature = "encryption")]
            None,
        )
        .unwrap_err();
    assert!(matches!(
        refused,
        FormatError::PayloadTooLarge { limit: 1024 }
    ));

    let unbounded = PayloadFormat::new(Codec::Bincode).compressed(true);
    let encoded = unbounded
        .encode(
            &big,
            "/p",
            #[cfg(feature = "encryption")]
            None,
        )
        .unwrap();
    let refused = format
        .decode::<Vec<u8>>(
            &encoded.bytes,
            encoded.tag,
            "/p",
            #[cfg(feature = "encryption")]
            None,
        )
        .unwrap_err();
    assert!(matches!(
        refused,
        FormatError::PayloadTooLarge { limit: 1024 }
    ));
}

#[cfg(feature = "encryption")]
mod sealed {
    use localcache_format::{Key, reseal};

    use super::*;

    fn key(byte: u8) -> Key {
        Key::from_slice(&[byte; 32]).unwrap()
    }

    #[test]
    fn every_row_seals_bound_to_its_path_and_opens_only_there() {
        let (k, other) = (key(1), key(2));
        for (codec, compressed, base) in rows() {
            let format = format_of(codec, compressed);
            let encoded = format.encode(&sample(), "/a", Some(&k)).unwrap();
            assert_eq!(encoded.tag, format!("{base}-aes256gcm-bound"));
            assert_eq!(seal_state(encoded.tag), SealState::Bound);
            let ok: Vec<String> = format
                .decode(&encoded.bytes, encoded.tag, "/a", Some(&k))
                .unwrap();
            assert_eq!(ok, sample());
            // another path, another key, another tag, and no key: all refused
            for refused in [
                format.decode::<Vec<String>>(&encoded.bytes, encoded.tag, "/b", Some(&k)),
                format.decode::<Vec<String>>(&encoded.bytes, encoded.tag, "/a", Some(&other)),
            ] {
                assert!(matches!(refused, Err(FormatError::EncryptionError(_))));
            }
            assert!(matches!(
                format.decode::<Vec<String>>(&encoded.bytes, encoded.tag, "/a", None),
                Err(FormatError::MissingEncryptionKey)
            ));
        }
    }

    #[test]
    fn a_plain_payload_read_with_a_key_is_unsealed() {
        let format = PayloadFormat::new(Codec::Bincode);
        let plain = format.encode(&sample(), "/a", None).unwrap();
        let refused = format.decode::<Vec<String>>(&plain.bytes, plain.tag, "/a", Some(&key(1)));
        assert!(matches!(refused, Err(FormatError::UnsealedEntry)));
    }

    #[test]
    fn reseal_moves_a_payload_to_the_new_key_and_keeps_its_path() {
        let (old, new) = (key(1), key(2));
        let format = PayloadFormat::new(Codec::Bincode);
        let sealed = format.encode(&sample(), "/a", Some(&old)).unwrap();
        let again = reseal(&sealed.bytes, sealed.tag, "/a", &old, &new).unwrap();
        assert_eq!(again.tag, "raw-aes256gcm-bound");
        let back: Vec<String> = format
            .decode(&again.content, again.tag, "/a", Some(&new))
            .unwrap();
        assert_eq!(back, sample());
        assert!(
            format
                .decode::<Vec<String>>(&again.content, again.tag, "/a", Some(&old))
                .is_err()
        );
    }

    #[test]
    fn a_key_is_exactly_32_bytes_and_prints_nothing() {
        assert!(Key::from_slice(&[0; 31]).is_none());
        assert!(Key::from_slice(&[0; 33]).is_none());
        assert_eq!(format!("{:?}", key(9)), "[redacted]");
    }
}
