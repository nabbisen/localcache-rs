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
}
