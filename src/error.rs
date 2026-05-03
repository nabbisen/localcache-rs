//! Error types for localcache.

use std::path::PathBuf;

/// All errors that can occur when using `localcache`.
#[derive(Debug, thiserror::Error)]
pub enum LocalFileCacheError {
    /// An error from the underlying SQLite database.
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// An I/O error (file reading, canonicalization, etc.).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// A serialization or deserialization error via bincode.
    #[error("serialization error: {0}")]
    Serialization(#[from] Box<bincode::ErrorKind>),

    /// The specified file does not exist on disk.
    #[error("file does not exist: {path}")]
    FileNotFound { path: PathBuf },

    /// A feature is defined but not yet fully implemented.
    #[error("unsupported feature: {0}")]
    UnsupportedFeature(String),

    /// The provided path cannot be resolved to a valid, canonical path.
    #[error("invalid path: {path}")]
    InvalidPath { path: PathBuf },

    /// A write operation was attempted on a read-only [`crate::CacheEngine`].
    #[error("operation not permitted: cache is open in read-only mode")]
    ReadOnly,

    /// A stored payload uses an encoding that is not understood by this build.
    #[error("unknown payload encoding: {0}")]
    UnknownEncoding(String),

    /// A payload's schema version does not match the configured version.
    #[error("payload version mismatch: stored={stored}, expected={expected}")]
    PayloadVersionMismatch { stored: u32, expected: u32 },

    /// An AES-256-GCM encryption or decryption failure.
    ///
    /// Possible causes: wrong key, corrupted data, missing `encryption` feature
    /// when trying to decrypt an encrypted entry.
    #[cfg(feature = "encryption")]
    #[error("encryption error: {0}")]
    EncryptionError(String),

    /// An async task spawned via `tokio::task::spawn_blocking` panicked.
    #[cfg(feature = "async")]
    #[error("async blocking task panicked")]
    AsyncTaskPanicked,
}
