//! The format layer's error type (RFC 030 slice 2): the seven variants of the
//! public `LocalFileCacheError` this layer produces, with the same `Display`
//! texts and feature gates. It reaches the public enum through the
//! crate-private lift in `crate::error`.

/// An error of the payload codec, compression or sealing.
#[derive(Debug, thiserror::Error)]
pub(crate) enum FormatError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("unknown payload encoding: {0}")]
    UnknownEncoding(String),
    #[cfg(feature = "compression")]
    #[error(
        "payload exceeds the {limit}-byte limit for compressed payloads (CacheOptions::max_decompressed_bytes)"
    )]
    PayloadTooLarge { limit: usize },
    #[cfg(feature = "encryption")]
    #[error("encryption error: {0}")]
    EncryptionError(String),
    #[cfg(feature = "encryption")]
    #[error("no encryption key is configured")]
    MissingEncryptionKey,
    #[cfg(feature = "encryption")]
    #[error(
        "unsealed entry: it is not sealed under this engine's key and bound to its path \
         (it is stored plain, or was sealed before v0.24.0); call rotate_encryption_key once \
         with the same key to seal it, or purge the entry"
    )]
    UnsealedEntry,
}
