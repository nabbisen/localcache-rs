//! Serialization helpers.
//!
//! All payload persistence goes through these two functions so that a future
//! migration to bincode v2 (or another codec) only requires changes in one
//! place.

use serde::{Serialize, de::DeserializeOwned};

use crate::error::LocalFileCacheError;

/// Encode `payload` to bytes using bincode.
pub(crate) fn serialize_payload<T>(payload: &T) -> Result<Vec<u8>, LocalFileCacheError>
where
    T: Serialize,
{
    bincode::serialize(payload).map_err(LocalFileCacheError::Serialization)
}

/// Decode bytes produced by [`serialize_payload`] back into `T`.
pub(crate) fn deserialize_payload<T>(bytes: &[u8]) -> Result<T, LocalFileCacheError>
where
    T: DeserializeOwned,
{
    bincode::deserialize(bytes).map_err(LocalFileCacheError::Serialization)
}
