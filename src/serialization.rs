//! Serialization helpers.
//!
//! All payload persistence goes through these two functions so that a future
//! migration to bincode v2 (or another codec) only requires changes in one
//! place.
//!
//! ## Streaming approach
//!
//! Rather than calling `bincode::serialize` (which allocates a fresh `Vec`
//! internally and returns it), we use `bincode::serialized_size` to obtain a
//! byte-count hint and pre-allocate the destination buffer before writing with
//! `bincode::serialize_into`.  This avoids the internal growth-and-copy that
//! can occur when bincode expands its buffer mid-serialisation.
//!
//! On the read side, `bincode::deserialize_from` takes any `Read` impl; we
//! wrap the byte slice in a `std::io::Cursor` to avoid copying it into a
//! separate buffer first.

use std::io::Cursor;

use serde::{Serialize, de::DeserializeOwned};

use crate::error::LocalFileCacheError;

/// Encode `payload` to bytes using bincode with pre-allocated capacity.
pub(crate) fn serialize_payload<T>(payload: &T) -> Result<Vec<u8>, LocalFileCacheError>
where
    T: Serialize,
{
    // Obtain a byte-count hint so the Vec can be pre-allocated to the right
    // size, avoiding reallocation during serialisation.
    let capacity = bincode::serialized_size(payload).unwrap_or(256) as usize;
    let mut buf = Vec::with_capacity(capacity);
    bincode::serialize_into(&mut buf, payload).map_err(LocalFileCacheError::Serialization)?;
    Ok(buf)
}

/// Decode bytes produced by [`serialize_payload`] back into `T`.
///
/// Uses `deserialize_from` with a zero-copy `Cursor` to avoid allocating an
/// intermediate buffer.
pub(crate) fn deserialize_payload<T>(bytes: &[u8]) -> Result<T, LocalFileCacheError>
where
    T: DeserializeOwned,
{
    bincode::deserialize_from(Cursor::new(bytes)).map_err(LocalFileCacheError::Serialization)
}
