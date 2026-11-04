//! The error type of the payload format.

use std::fmt;

/// Why a payload could not be encoded, decoded, sealed or opened.
///
/// The `Display` texts are the ones `localcache` has always shown for the
/// public variant of the same name: the facade lifts each variant to its
/// `LocalFileCacheError` counterpart one to one, and a test there pins the
/// two texts equal.
///
/// Not `#[non_exhaustive]`: the facade matches it without a wildcard arm, so a
/// new variant is a compile error there and not a silent mapping.
#[derive(Debug)]
pub enum FormatError {
    /// An I/O failure while compressing or decompressing.
    Io(std::io::Error),
    /// The payload could not be serialized or deserialized; the message says why.
    Serialization(String),
    /// The encoding tag names a format this build cannot read.
    UnknownEncoding(String),
    /// A compressed payload would be larger than the allowed decompressed size.
    #[cfg(feature = "compression")]
    PayloadTooLarge {
        /// The limit that was exceeded, in bytes.
        limit: usize,
    },
    /// Sealing or opening failed; the message says why. It never carries key
    /// material.
    #[cfg(feature = "encryption")]
    EncryptionError(String),
    /// A sealed payload was read with no key configured.
    #[cfg(feature = "encryption")]
    MissingEncryptionKey,
    /// A payload that is not sealed under this key and bound to its path was
    /// read with a key configured.
    #[cfg(feature = "encryption")]
    UnsealedEntry,
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "io error: {error}"),
            Self::Serialization(message) => write!(f, "serialization error: {message}"),
            Self::UnknownEncoding(tag) => write!(f, "unknown payload encoding: {tag}"),
            #[cfg(feature = "compression")]
            Self::PayloadTooLarge { limit } => write!(
                f,
                "payload exceeds the {limit}-byte limit for compressed payloads (CacheOptions::max_decompressed_bytes)"
            ),
            #[cfg(feature = "encryption")]
            Self::EncryptionError(message) => write!(f, "encryption error: {message}"),
            #[cfg(feature = "encryption")]
            Self::MissingEncryptionKey => f.write_str("no encryption key is configured"),
            #[cfg(feature = "encryption")]
            Self::UnsealedEntry => f.write_str(
                "unsealed entry: it is not sealed under this engine's key and bound to its path \
                 (it is stored plain, or was sealed before v0.24.0); call rotate_encryption_key once \
                 with the same key to seal it, or purge the entry",
            ),
        }
    }
}

impl std::error::Error for FormatError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for FormatError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
