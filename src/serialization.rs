//! Serialization and encoding helpers.
//!
//! All payload persistence goes through these functions.  Two layers exist:
//!
//! 1. **Serialization** — convert `T` ↔ `Vec<u8>` via bincode.
//! 2. **Encoding** — optionally compress/decompress those bytes via zstd
//!    (requires the `compression` feature).
//!
//! The encoding used when writing is recorded as a string tag in the database
//! (`"raw"` or `"zstd"`), so that reads can decode transparently even if the
//! engine is later opened without the `compression` feature (in which case a
//! `zstd`-encoded entry returns [`LocalFileCacheError::UnknownEncoding`]).

use std::io::Cursor;

use serde::{Serialize, de::DeserializeOwned};

use crate::error::LocalFileCacheError;

// ---------------------------------------------------------------------------
// Serialization (bincode)
// ---------------------------------------------------------------------------

/// Encode `payload` to bytes using bincode with a pre-allocated buffer.
///
/// `bincode::serialized_size` is used to size the buffer before writing,
/// avoiding internal reallocations during serialization.
pub(crate) fn serialize_payload<T>(payload: &T) -> Result<Vec<u8>, LocalFileCacheError>
where
    T: Serialize,
{
    let capacity = bincode::serialized_size(payload).unwrap_or(256) as usize;
    let mut buf = Vec::with_capacity(capacity);
    bincode::serialize_into(&mut buf, payload).map_err(LocalFileCacheError::Serialization)?;
    Ok(buf)
}

/// Decode bytes produced by [`serialize_payload`] back into `T`.
///
/// Uses `deserialize_from` with a zero-copy `Cursor` to avoid an intermediate
/// buffer.
pub(crate) fn deserialize_payload<T>(bytes: &[u8]) -> Result<T, LocalFileCacheError>
where
    T: DeserializeOwned,
{
    bincode::deserialize_from(Cursor::new(bytes)).map_err(LocalFileCacheError::Serialization)
}

// ---------------------------------------------------------------------------
// Encoding layer (raw / zstd)
// ---------------------------------------------------------------------------

/// Encode `payload` for storage, returning `(bytes, encoding_tag)`.
///
/// When `compress` is `true` **and** the `compression` feature is enabled,
/// the serialized bytes are compressed with zstd and the tag `"zstd"` is
/// returned.  Otherwise the raw bincode bytes are returned with tag `"raw"`.
pub(crate) fn encode_payload<T>(
    payload: &T,
    compress: bool,
) -> Result<(Vec<u8>, &'static str), LocalFileCacheError>
where
    T: Serialize,
{
    let bytes = serialize_payload(payload)?;

    #[cfg(feature = "compression")]
    if compress {
        let compressed = compress_bytes(&bytes)?;
        return Ok((compressed, "zstd"));
    }
    let _ = compress; // suppress unused warning when feature is off
    Ok((bytes, "raw"))
}

/// Decode stored bytes back into `T`, given the `encoding` tag.
///
/// * `"raw"` — plain bincode deserialization.
/// * `"zstd"` — decompress with zstd first, then deserialize.  Returns
///   [`LocalFileCacheError::UnknownEncoding`] if the `compression` feature is
///   not enabled in this build.
/// * anything else — returns [`LocalFileCacheError::UnknownEncoding`].
pub(crate) fn decode_payload<T>(bytes: &[u8], encoding: &str) -> Result<T, LocalFileCacheError>
where
    T: DeserializeOwned,
{
    match encoding {
        "raw" => deserialize_payload(bytes),
        #[cfg(feature = "compression")]
        "zstd" => {
            let decompressed = decompress_bytes(bytes)?;
            deserialize_payload(&decompressed)
        }
        other => Err(LocalFileCacheError::UnknownEncoding(other.to_owned())),
    }
}

// ---------------------------------------------------------------------------
// Compression helpers (only compiled with the `compression` feature)
// ---------------------------------------------------------------------------

#[cfg(feature = "compression")]
fn compress_bytes(data: &[u8]) -> Result<Vec<u8>, LocalFileCacheError> {
    // Compression level 3 is a good default: fast with reasonable ratio.
    zstd::encode_all(Cursor::new(data), 3).map_err(LocalFileCacheError::Io)
}

#[cfg(feature = "compression")]
fn decompress_bytes(data: &[u8]) -> Result<Vec<u8>, LocalFileCacheError> {
    zstd::decode_all(Cursor::new(data)).map_err(LocalFileCacheError::Io)
}
