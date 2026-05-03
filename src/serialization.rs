//! Serialization and encoding helpers.
//!
//! Two orthogonal layers:
//!
//! 1. **Codec** — serialize `T` → `Vec<u8>` using bincode or serde_json.
//! 2. **Compression** — optionally zstd-compress those bytes.
//!
//! The combination is captured in a short encoding tag stored in the database:
//!
//! | Tag          | Codec   | Compression |
//! |--------------|---------|-------------|
//! | `"raw"`      | bincode | none        |
//! | `"zstd"`     | bincode | zstd        |
//! | `"json"`     | json    | none        |
//! | `"json-zstd"`| json    | zstd        |

use std::io::Cursor;

use serde::{Serialize, de::DeserializeOwned};

use crate::cache::options::Codec;
use crate::error::LocalFileCacheError;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Encode `payload` for storage, returning `(bytes, encoding_tag)`.
pub(crate) fn encode_payload<T>(
    payload: &T,
    compress: bool,
    codec: Codec,
) -> Result<(Vec<u8>, &'static str), LocalFileCacheError>
where
    T: Serialize,
{
    let serialized = serialize_with_codec(payload, codec)?;

    #[cfg(feature = "compression")]
    if compress {
        let compressed = compress_bytes(&serialized)?;
        let tag = match codec {
            Codec::Bincode => "zstd",
            #[cfg(feature = "json")]
            Codec::Json => "json-zstd",
        };
        return Ok((compressed, tag));
    }
    let _ = compress;

    let tag = match codec {
        Codec::Bincode => "raw",
        #[cfg(feature = "json")]
        Codec::Json => "json",
    };
    Ok((serialized, tag))
}

/// Decode stored bytes back into `T` given the `encoding` tag.
pub(crate) fn decode_payload<T>(bytes: &[u8], encoding: &str) -> Result<T, LocalFileCacheError>
where
    T: DeserializeOwned,
{
    match encoding {
        "raw" => deserialize_bincode(bytes),

        #[cfg(feature = "compression")]
        "zstd" => {
            let d = decompress_bytes(bytes)?;
            deserialize_bincode(&d)
        }

        #[cfg(feature = "json")]
        "json" => deserialize_json(bytes),

        #[cfg(all(feature = "json", feature = "compression"))]
        "json-zstd" => {
            let d = decompress_bytes(bytes)?;
            deserialize_json(&d)
        }

        other => Err(LocalFileCacheError::UnknownEncoding(other.to_owned())),
    }
}

// ---------------------------------------------------------------------------
// Codec-level serialization
// ---------------------------------------------------------------------------

fn serialize_with_codec<T: Serialize>(
    payload: &T,
    codec: Codec,
) -> Result<Vec<u8>, LocalFileCacheError> {
    match codec {
        Codec::Bincode => serialize_bincode(payload),
        #[cfg(feature = "json")]
        Codec::Json => serialize_json(payload),
    }
}

/// bincode: pre-allocate via `serialized_size`, then `serialize_into`.
pub(crate) fn serialize_bincode<T: Serialize>(payload: &T) -> Result<Vec<u8>, LocalFileCacheError> {
    let capacity = bincode::serialized_size(payload).unwrap_or(256) as usize;
    let mut buf = Vec::with_capacity(capacity);
    bincode::serialize_into(&mut buf, payload).map_err(LocalFileCacheError::Serialization)?;
    Ok(buf)
}

/// bincode: zero-copy read via `Cursor`.
pub(crate) fn deserialize_bincode<T: DeserializeOwned>(
    bytes: &[u8],
) -> Result<T, LocalFileCacheError> {
    bincode::deserialize_from(Cursor::new(bytes)).map_err(LocalFileCacheError::Serialization)
}

#[cfg(feature = "json")]
fn serialize_json<T: Serialize>(payload: &T) -> Result<Vec<u8>, LocalFileCacheError> {
    serde_json::to_vec(payload).map_err(|e| {
        LocalFileCacheError::UnsupportedFeature(format!("json serialization error: {e}"))
    })
}

#[cfg(feature = "json")]
fn deserialize_json<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, LocalFileCacheError> {
    serde_json::from_slice(bytes).map_err(|e| {
        LocalFileCacheError::UnsupportedFeature(format!("json deserialization error: {e}"))
    })
}

// ---------------------------------------------------------------------------
// Compression helpers
// ---------------------------------------------------------------------------

#[cfg(feature = "compression")]
fn compress_bytes(data: &[u8]) -> Result<Vec<u8>, LocalFileCacheError> {
    zstd::encode_all(Cursor::new(data), 3).map_err(LocalFileCacheError::Io)
}

#[cfg(feature = "compression")]
fn decompress_bytes(data: &[u8]) -> Result<Vec<u8>, LocalFileCacheError> {
    zstd::decode_all(Cursor::new(data)).map_err(LocalFileCacheError::Io)
}
