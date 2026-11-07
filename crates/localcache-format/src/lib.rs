//! Implementation crate of `localcache`: the payload format. Depend on
//! `localcache`; this crate's API is not a separate stability promise.
//!
//! It holds what turns a value into the bytes that are stored, and back: the
//! codec, compression, JSON, the table of encoding tags, and sealing and
//! opening. It knows nothing of SQLite, files or caches.
//!
//! The encoding pipeline is: **codec → compress → encrypt**.
//! The encoding tag stored in the database reflects all active layers:
//!
//! # Wire-format stability
//!
//! The `Bincode` codec path **always** writes and reads the legacy `bincode`-1
//! wire format of RFC 008 (little-endian, fixed-width integers, `u64` length
//! prefixes, `u32` variant indices), implemented by the private
//! `payload_codec` module (RFC 028).  This is a permanent commitment: the exact
//! byte sequence produced today must be decodable by every past and future
//! `0.x`/`1.x` release without migration, and bytes written by any past release
//! must decode.  **Do not change the format** — doing so would silently corrupt
//! every existing user database with no compile-time error; the golden vectors in
//! `tests/fixtures/payload_codec` of this crate fail if it changes.  Any future format change
//! requires a schema-version migration (the schema code of `localcache`) and a headline
//! CHANGELOG entry.
//!
//! | Tag                           | Codec   | Compress | Encrypt                         |
//! |-------------------------------|---------|----------|---------------------------------|
//! | `"raw"`                       | bincode | —        | —                               |
//! | `"zstd"`                      | bincode | zstd     | —                               |
//! | `"json"`                      | json    | —        | —                               |
//! | `"json-zstd"`                 | json    | zstd     | —                               |
//! | `"raw-aes256gcm"`             | bincode | —        | AES-256-GCM, not bound (≤ v0.23) |
//! | `"zstd-aes256gcm"`            | bincode | zstd     | AES-256-GCM, not bound (≤ v0.23) |
//! | `"json-aes256gcm"`            | json    | —        | AES-256-GCM, not bound (≤ v0.23) |
//! | `"json-zstd-aes256gcm"`       | json    | zstd     | AES-256-GCM, not bound (≤ v0.23) |
//! | `"raw-aes256gcm-bound"`       | bincode | —        | AES-256-GCM, bound to the path  |
//! | `"zstd-aes256gcm-bound"`      | bincode | zstd     | AES-256-GCM, bound to the path  |
//! | `"json-aes256gcm-bound"`      | json    | —        | AES-256-GCM, bound to the path  |
//! | `"json-zstd-aes256gcm-bound"` | json    | zstd     | AES-256-GCM, bound to the path  |
//!
//! A *bound* payload is sealed with associated data
//! `"localcache/seal/1" 0x00 <the full stored tag> 0x00 <the entry's path>`
//! (RFC 029 R1), so a ciphertext moved to another path, or read under another
//! tag, fails to open. The tag is matched exactly and case-sensitively.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

#[cfg(feature = "compression")]
use std::io::Cursor;

use serde::{Serialize, de::DeserializeOwned};

/// Selects the serialization codec used to encode payload values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Codec {
    /// Compact binary encoding (the legacy `bincode`-1 wire format of
    /// RFC 008) — compact and fast.  Default.
    ///
    /// # Wire-format stability guarantee
    ///
    /// Payloads written with this codec are in the legacy `bincode`-1 wire
    /// format, which is byte-compatible with every `localcache` release from
    /// 0.1 onwards.  This guarantee holds for **all 0.x and future 1.x**
    /// releases: the wire format will only change behind a headline
    /// CHANGELOG migration that transparently rewrites existing payloads.
    ///
    /// **Consequence for applications:** a `localcache` upgrade **never**
    /// requires bumping `payload_version`.  The `payload_version` field is
    /// yours to increment when *your* payload struct or pipeline changes,
    /// not when the crate version changes.
    #[default]
    Bincode,

    /// JSON encoding via `serde_json` — human-readable, larger on disk.
    ///
    /// Requires the `json` Cargo feature.
    #[cfg(feature = "json")]
    Json,
}

// ---------------------------------------------------------------------------
// Decompression limit
// ---------------------------------------------------------------------------

/// The default for `CacheOptions::max_decompressed_bytes` of `localcache`: 64 MiB
/// (`67_108_864` bytes), about ten thousand times the largest payload the
/// project's benchmarks store. A larger default lets a hostile database file
/// exhaust a modest host; a smaller one costs users with unusually large
/// compressed payloads a clear error and a one-line fix.
///
/// Requires the `compression` Cargo feature.
#[cfg(feature = "compression")]
pub const DEFAULT_MAX_DECOMPRESSED_BYTES: usize = 64 * 1024 * 1024;

mod error;
#[cfg(feature = "encryption")]
mod key;
mod payload_codec;

pub use error::FormatError;
#[cfg(feature = "encryption")]
pub use key::{ErasedVec, Key, erase};

#[cfg(all(test, feature = "encryption"))]
mod seal_tests;

// ---------------------------------------------------------------------------
// Encryption suffix
// ---------------------------------------------------------------------------

/// The suffix of a payload sealed without binding (v0.23 and earlier).
const LEGACY_SUFFIX: &str = "-aes256gcm";
/// The suffix of a payload sealed bound to its path (RFC 029 R1).
const BOUND_SUFFIX: &str = "-aes256gcm-bound";

/// How a stored payload is sealed, read from its encoding tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SealState {
    /// Not encrypted.
    Plain,
    /// Encrypted without binding to the path (written by v0.23 and earlier).
    Unbound,
    /// Encrypted with the entry's path and tag as associated data.
    Bound,
}

/// Split a stored tag into its base tag and how it is sealed.
///
/// The match is exact and case-sensitive. The suffixes cannot both match one
/// tag (one ends in `m`, the other in `d`), so no tag parses two ways.
fn parse_tag(tag: &str) -> (&str, SealState) {
    if let Some(base) = tag.strip_suffix(BOUND_SUFFIX) {
        (base, SealState::Bound)
    } else if let Some(base) = tag.strip_suffix(LEGACY_SUFFIX) {
        (base, SealState::Unbound)
    } else {
        (tag, SealState::Plain)
    }
}

/// How the payload stored under `tag` is sealed. The suffix is matched exactly
/// and case-sensitively; a tag this build cannot decode still has a state.
pub fn seal_state(tag: &str) -> SealState {
    parse_tag(tag).1
}

/// Whether `base` is one of the base tags this build can decode.
#[cfg(feature = "encryption")]
fn is_known_base(base: &str) -> bool {
    match base {
        "raw" => true,
        #[cfg(feature = "compression")]
        "zstd" => true,
        #[cfg(feature = "json")]
        "json" => true,
        #[cfg(all(feature = "json", feature = "compression"))]
        "json-zstd" => true,
        _ => false,
    }
}

/// The bound tag for a base tag, if the build has the layers it names.
#[cfg(feature = "encryption")]
fn bound_tag(base: &str) -> Option<&'static str> {
    match base {
        "raw" => Some("raw-aes256gcm-bound"),
        #[cfg(feature = "compression")]
        "zstd" => Some("zstd-aes256gcm-bound"),
        #[cfg(feature = "json")]
        "json" => Some("json-aes256gcm-bound"),
        #[cfg(all(feature = "json", feature = "compression"))]
        "json-zstd" => Some("json-zstd-aes256gcm-bound"),
        _ => None,
    }
}

/// The associated data of a bound payload (RFC 029 R1): one function, called
/// by sealing and by opening alike.
#[cfg(feature = "encryption")]
fn associated_data(tag: &str, path: &str) -> Vec<u8> {
    let mut aad = Vec::with_capacity(17 + 1 + tag.len() + 1 + path.len());
    aad.extend_from_slice(b"localcache/seal/1");
    aad.push(0);
    aad.extend_from_slice(tag.as_bytes());
    aad.push(0);
    aad.extend_from_slice(path.as_bytes());
    aad
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// How payloads are turned into stored bytes and back: the codec, whether to
/// compress, and the bound on a decompressed size. The pipeline is
/// **codec → compress → seal**; the key, when there is one, is given per call.
#[derive(Debug, Clone, Copy)]
pub struct PayloadFormat {
    codec: Codec,
    #[cfg(feature = "compression")]
    compress: bool,
    #[cfg(feature = "compression")]
    max_decompressed_bytes: usize,
}

/// A payload ready to be stored: the bytes and the encoding tag that says how
/// to read them back.
#[derive(Debug)]
pub struct Encoded {
    /// The bytes to store.
    pub bytes: Vec<u8>,
    /// The encoding tag to store with them (see the table in the crate docs).
    pub tag: &'static str,
}

impl PayloadFormat {
    /// An uncompressed `codec`, with the default decompression bound.
    pub fn new(codec: Codec) -> Self {
        Self {
            codec,
            #[cfg(feature = "compression")]
            compress: false,
            #[cfg(feature = "compression")]
            max_decompressed_bytes: DEFAULT_MAX_DECOMPRESSED_BYTES,
        }
    }

    /// Whether to compress with zstd before sealing.
    #[cfg(feature = "compression")]
    #[must_use]
    pub fn compressed(mut self, compress: bool) -> Self {
        self.compress = compress;
        self
    }

    /// The most a compressed payload may decompress to, on write and on read.
    #[cfg(feature = "compression")]
    #[must_use]
    pub fn max_decompressed_bytes(mut self, limit: usize) -> Self {
        self.max_decompressed_bytes = limit;
        self
    }

    /// Encode `value` for storage under `path`. With a `key` the payload is
    /// sealed bound to `path` and to its tag.
    ///
    /// With compression, a payload whose serialized size exceeds the bound is
    /// refused before any compression, because the same bound would refuse to
    /// read it back. A payload that is not compressed is not checked
    /// (RFC 025 Amendment 2, D4).
    pub fn encode<T: Serialize>(
        &self,
        value: &T,
        path: &str,
        #[cfg(feature = "encryption")] key: Option<&Key>,
    ) -> Result<Encoded, FormatError> {
        #[cfg(feature = "compression")]
        let compress = self.compress;
        #[cfg(not(feature = "compression"))]
        let compress = false;
        let (bytes, tag) = encode_payload(
            value,
            path,
            compress,
            self.codec,
            #[cfg(feature = "compression")]
            self.max_decompressed_bytes,
            #[cfg(feature = "encryption")]
            key.map(Key::as_bytes),
        )?;
        Ok(Encoded { bytes, tag })
    }

    /// Decode the bytes stored under `path` with the encoding `tag`.
    ///
    /// With a key only a payload bound to `path` is read; a plain or
    /// unbound one is `FormatError::UnsealedEntry`. Without a key a sealed
    /// tag is `FormatError::MissingEncryptionKey` and a plain tag is read.
    /// A tag this build cannot decode is [`FormatError::UnknownEncoding`]
    /// whatever the key. A compressed payload that would decompress to more
    /// than the bound is `FormatError::PayloadTooLarge`; an uncompressed one
    /// is not limited. (The first three variants exist with the `encryption`
    /// or `compression` feature, so they are not links.)
    pub fn decode<T: DeserializeOwned>(
        &self,
        bytes: &[u8],
        tag: &str,
        path: &str,
        #[cfg(feature = "encryption")] key: Option<&Key>,
    ) -> Result<T, FormatError> {
        decode_payload(
            bytes,
            tag,
            path,
            #[cfg(feature = "compression")]
            self.max_decompressed_bytes,
            #[cfg(feature = "encryption")]
            key.map(Key::as_bytes),
        )
    }
}

// ---------------------------------------------------------------------------
// Public encode / decode
// ---------------------------------------------------------------------------

/// Encode `payload` for storage, returning `(bytes, encoding_tag)`.
///
/// With `compress`, a payload whose serialized size exceeds
/// `max_decompressed_bytes` is refused (before any compression), because the
/// same limit would refuse to read it back. A payload that is not compressed
/// is not checked (RFC 025 Amendment 2, D4).
fn encode_payload<T>(
    payload: &T,
    path: &str,
    compress: bool,
    codec: Codec,
    #[cfg(feature = "compression")] max_decompressed_bytes: usize,
    #[cfg(feature = "encryption")] encryption_key: Option<&[u8; 32]>,
) -> Result<(Vec<u8>, &'static str), FormatError>
where
    T: Serialize,
{
    #[cfg(not(feature = "encryption"))]
    let _ = path;
    let serialized = serialize_with_codec(payload, codec)?;

    // Compression step.
    #[allow(unused_mut)]
    let (mut bytes, mut base_tag): (Vec<u8>, &'static str) = {
        #[cfg(feature = "compression")]
        if compress {
            if serialized.len() > max_decompressed_bytes {
                return Err(FormatError::PayloadTooLarge {
                    limit: max_decompressed_bytes,
                });
            }
            let compressed = compress_bytes(&serialized)?;
            let tag = match codec {
                Codec::Bincode => "zstd",
                #[cfg(feature = "json")]
                Codec::Json => "json-zstd",
            };
            (compressed, tag)
        } else {
            let tag = match codec {
                Codec::Bincode => "raw",
                #[cfg(feature = "json")]
                Codec::Json => "json",
            };
            (serialized, tag)
        }
        #[cfg(not(feature = "compression"))]
        {
            let _ = compress;
            let tag = match codec {
                Codec::Bincode => "raw",
                #[cfg(feature = "json")]
                Codec::Json => "json",
            };
            (serialized, tag)
        }
    };

    // Encryption step: the tag is chosen first, because it is part of the
    // associated data.
    #[cfg(feature = "encryption")]
    if let Some(key) = encryption_key {
        let tag = bound_tag(base_tag).ok_or_else(|| {
            FormatError::Serialization(format!("no encrypted tag for base tag '{base_tag}'"))
        })?;
        bytes = encrypt_bytes(&bytes, key, &associated_data(tag, path))?;
        base_tag = tag;
    }

    Ok((bytes, base_tag))
}

/// Decode stored bytes back into `T` given the `encoding` tag.
///
/// A compressed payload that would decompress to more than
/// `max_decompressed_bytes` is refused with
/// [`FormatError::PayloadTooLarge`] (RFC 025 Amendment 2, D3);
/// uncompressed payloads are not limited.
fn decode_payload<T>(
    bytes: &[u8],
    encoding: &str,
    path: &str,
    #[cfg(feature = "compression")] max_decompressed_bytes: usize,
    #[cfg(feature = "encryption")] encryption_key: Option<&[u8; 32]>,
) -> Result<T, FormatError>
where
    T: DeserializeOwned,
{
    #[cfg(not(feature = "encryption"))]
    let _ = path;
    // With a key, only a bound payload is read (RFC 029 R2); a plain or
    // legacy-sealed one is refused as `UnsealedEntry`. Without a key, a sealed
    // tag is `MissingEncryptionKey` and a plain tag is read, as before. A tag
    // this build cannot decode is `UnknownEncoding` whatever the key.
    let (base, sealing) = parse_tag(encoding);
    let (inner_bytes_cow, inner_tag) = match sealing {
        SealState::Plain => {
            #[cfg(feature = "encryption")]
            if encryption_key.is_some() && is_known_base(base) {
                return Err(FormatError::UnsealedEntry);
            }
            (std::borrow::Cow::Borrowed(bytes), encoding)
        }
        SealState::Unbound | SealState::Bound => {
            #[cfg(feature = "encryption")]
            {
                let key = encryption_key.ok_or(FormatError::MissingEncryptionKey)?;
                if !is_known_base(base) {
                    return Err(FormatError::UnknownEncoding(base.to_owned()));
                }
                if sealing == SealState::Unbound {
                    return Err(FormatError::UnsealedEntry);
                }
                let decrypted = decrypt_bytes(bytes, key, &associated_data(encoding, path))?;
                (std::borrow::Cow::Owned(decrypted), base)
            }
            #[cfg(not(feature = "encryption"))]
            {
                let _ = (bytes, base);
                return Err(FormatError::UnknownEncoding(encoding.to_owned()));
            }
        }
    };

    let data: &[u8] = &inner_bytes_cow;

    // Decompression + deserialization.
    match inner_tag {
        "raw" => deserialize_bincode(data),

        #[cfg(feature = "compression")]
        "zstd" => {
            let d = decompress_bytes(data, max_decompressed_bytes)?;
            deserialize_bincode(&d)
        }

        #[cfg(feature = "json")]
        "json" => deserialize_json(data),

        #[cfg(all(feature = "json", feature = "compression"))]
        "json-zstd" => {
            let d = decompress_bytes(data, max_decompressed_bytes)?;
            deserialize_json(&d)
        }

        other => Err(FormatError::UnknownEncoding(other.to_owned())),
    }
}

// ---------------------------------------------------------------------------
// Codec helpers
// ---------------------------------------------------------------------------

fn serialize_with_codec<T: Serialize>(payload: &T, codec: Codec) -> Result<Vec<u8>, FormatError> {
    match codec {
        Codec::Bincode => serialize_bincode(payload),
        #[cfg(feature = "json")]
        Codec::Json => serialize_json(payload),
    }
}

pub(crate) fn serialize_bincode<T: Serialize>(payload: &T) -> Result<Vec<u8>, FormatError> {
    payload_codec::to_vec(payload).map_err(|e| FormatError::Serialization(e.to_string()))
}

/// Decode a legacy-format payload. Bytes after the value are ignored (RFC 028 R2.5).
pub(crate) fn deserialize_bincode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, FormatError> {
    payload_codec::from_slice(bytes).map_err(|e| FormatError::Serialization(e.to_string()))
}

#[cfg(feature = "json")]
fn serialize_json<T: Serialize>(payload: &T) -> Result<Vec<u8>, FormatError> {
    serde_json::to_vec(payload)
        .map_err(|e| FormatError::Serialization(format!("json serialization error: {e}")))
}

#[cfg(feature = "json")]
fn deserialize_json<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, FormatError> {
    serde_json::from_slice(bytes)
        .map_err(|e| FormatError::Serialization(format!("json deserialization error: {e}")))
}

// ---------------------------------------------------------------------------
// Compression
// ---------------------------------------------------------------------------

#[cfg(feature = "compression")]
fn compress_bytes(data: &[u8]) -> Result<Vec<u8>, FormatError> {
    zstd::encode_all(Cursor::new(data), 3).map_err(FormatError::Io)
}

/// Decompress a zstd frame into at most `limit` bytes (RFC 025 Amendment 2).
///
/// A few kilobytes of compressed data can expand to gigabytes, so neither the
/// frame header nor the stream is trusted:
///
/// 1. a header that *declares* a content size above `limit` is refused before
///    anything is allocated;
/// 2. otherwise the stream is decoded in small chunks into a buffer that never
///    grows past `limit`, and a stream that yields more than `limit` bytes is
///    refused as soon as the byte after the limit appears.
///
/// A malformed frame is an [`Io`](FormatError::Io) error, as before.
/// This is the only place a zstd frame is decoded.
#[cfg(feature = "compression")]
fn decompress_bytes(data: &[u8], limit: usize) -> Result<Vec<u8>, FormatError> {
    use std::io::{ErrorKind, Read};

    let too_large = || FormatError::PayloadTooLarge { limit };

    // A header that cannot be parsed is left to the decoder, which reports it
    // as a malformed frame.
    let declared = zstd::zstd_safe::get_frame_content_size(data).ok().flatten();
    if declared.is_some_and(|size| size > limit as u64) {
        return Err(too_large());
    }

    // `declared <= limit`, so this pre-size is at most the limit.
    let mut out: Vec<u8> = Vec::with_capacity(declared.map_or(0, |size| size as usize));
    let mut decoder = zstd::stream::read::Decoder::new(data).map_err(FormatError::Io)?;
    let mut chunk = [0_u8; 16 * 1024];
    loop {
        let n = match decoder.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(FormatError::Io(error)),
        };
        if n > limit - out.len() {
            return Err(too_large());
        }
        if n > out.capacity() - out.len() {
            // Double, but never past the limit: the buffer's capacity (and
            // so the memory held, apart from one reallocation) is bounded by
            // `limit`, where `Vec`'s own growth would reach twice it.
            let target = out
                .capacity()
                .saturating_mul(2)
                .max(out.len() + n)
                .min(limit);
            out.reserve_exact(target - out.len());
        }
        out.extend_from_slice(&chunk[..n]);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Encryption (AES-256-GCM)
// ---------------------------------------------------------------------------

#[cfg(feature = "encryption")]
fn encrypt_bytes(plaintext: &[u8], key: &[u8; 32], aad: &[u8]) -> Result<Vec<u8>, FormatError> {
    use aes_gcm::{
        Aes256Gcm, KeyInit,
        aead::{Aead, Generate, Nonce, Payload},
    };

    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| FormatError::EncryptionError(format!("key init failed: {e}")))?;
    let nonce = Nonce::<Aes256Gcm>::try_generate()
        .map_err(|e| FormatError::EncryptionError(format!("nonce generation failed: {e}")))?;
    let ciphertext = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|e| FormatError::EncryptionError(format!("encryption failed: {e}")))?;

    // Store as: nonce (12 bytes) ‖ ciphertext.
    let mut result = Vec::with_capacity(nonce.len() + ciphertext.len());
    result.extend_from_slice(&nonce);
    result.extend_from_slice(&ciphertext);
    Ok(result)
}

#[cfg(feature = "encryption")]
fn decrypt_bytes(data: &[u8], key: &[u8; 32], aad: &[u8]) -> Result<Vec<u8>, FormatError> {
    use aes_gcm::{
        Aes256Gcm, KeyInit,
        aead::{Aead, Nonce, Payload},
    };

    const NONCE_LEN: usize = 12;
    if data.len() < NONCE_LEN {
        return Err(FormatError::EncryptionError(
            "ciphertext too short to contain nonce".into(),
        ));
    }

    let (nonce_bytes, ciphertext) = data.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| FormatError::EncryptionError(format!("key init failed: {e}")))?;
    let nonce = Nonce::<Aes256Gcm>::try_from(nonce_bytes)
        .map_err(|_| FormatError::EncryptionError("nonce has the wrong length".into()))?;
    cipher
        .decrypt(
            &nonce,
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| {
            FormatError::EncryptionError(
                "decryption failed — wrong key, corrupted data, or the entry was moved from another path"
                .into(),
            )
        })
}

// ---------------------------------------------------------------------------
// Key rotation (feature-gated)
// ---------------------------------------------------------------------------

/// A payload sealed again: the new content and its new (bound) tag.
#[cfg(feature = "encryption")]
#[derive(Debug)]
pub struct Resealed {
    /// The sealed bytes to store.
    pub content: Vec<u8>,
    /// The bound tag to store with them.
    pub tag: &'static str,
}

/// Re-seal one stored payload under `new`, bound to `path`, whatever state it
/// is in (RFC 029 R3): plain bytes are sealed as they are, a payload sealed
/// without binding is opened with `old` and no associated data, a bound one
/// with `old` and its associated data.
///
/// Returns the new content and the new (bound) tag. A tag this build cannot
/// decode is [`FormatError::UnknownEncoding`].
#[cfg(feature = "encryption")]
pub fn reseal(
    content: &[u8],
    tag: &str,
    path: &str,
    old: &Key,
    new: &Key,
) -> Result<Resealed, FormatError> {
    let (old, new) = (old.as_bytes(), new.as_bytes());
    let (base, sealing) = parse_tag(tag);
    if !is_known_base(base) {
        return Err(FormatError::UnknownEncoding(tag.to_owned()));
    }
    let new_tag = bound_tag(base).ok_or_else(|| FormatError::UnknownEncoding(tag.to_owned()))?;
    let plain = match sealing {
        SealState::Plain => std::borrow::Cow::Borrowed(content),
        SealState::Unbound => std::borrow::Cow::Owned(decrypt_bytes(content, old, &[])?),
        SealState::Bound => {
            std::borrow::Cow::Owned(decrypt_bytes(content, old, &associated_data(tag, path))?)
        }
    };
    let sealed = encrypt_bytes(&plain, new, &associated_data(new_tag, path))?;
    Ok(Resealed {
        content: sealed,
        tag: new_tag,
    })
}
