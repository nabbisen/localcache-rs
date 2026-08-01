//! Error types for localcache.

use std::path::PathBuf;

/// All errors that can occur when using `localcache`.
///
/// # Exhaustiveness
///
/// This enum is `#[non_exhaustive]`: a `match` without a `_` arm fails to
/// compile from outside this crate, even when every variant currently known
/// is listed, so a new variant can never again become a breaking change.
///
/// The `E0004` on the block below documents *intent* for a human reader --
/// rustdoc does not verify a `compile_fail` block's error code against the
/// actual diagnostic, so this is not an enforced check. The real guarantee
/// is established by mutation testing at review time (confirmed a `_` arm
/// makes this pass, and that an unrelated compile error still passes too).
///
/// ```compile_fail,E0004
/// # use localcache::LocalFileCacheError;
/// fn handle(err: LocalFileCacheError) {
///     match err {
///         LocalFileCacheError::Database(_) => {}
///         LocalFileCacheError::Io(_) => {}
///         LocalFileCacheError::Serialization(_) => {}
///         LocalFileCacheError::FileNotFound { .. } => {}
///         LocalFileCacheError::UnsupportedFeature(_) => {}
///         LocalFileCacheError::InvalidPath { .. } => {}
///         LocalFileCacheError::ReadOnly => {}
///         LocalFileCacheError::UnknownEncoding(_) => {}
///         LocalFileCacheError::PayloadVersionMismatch { .. } => {}
///         LocalFileCacheError::Poisoned { .. } => {}
///         #[cfg(feature = "encryption")]
///         LocalFileCacheError::EncryptionError(_) => {}
///         #[cfg(any(feature = "async", feature = "async-std", feature = "smol"))]
///         LocalFileCacheError::AsyncTaskPanicked => {}
///         // deliberately no `_` arm
///     }
/// }
/// ```
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LocalFileCacheError {
    /// An error from the underlying SQLite database.
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// An I/O error (file reading, canonicalization, etc.).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// A serialization or deserialization error (bincode encode/decode failure).
    #[error("serialization error: {0}")]
    Serialization(String),

    /// The specified file does not exist on disk.
    #[error("file does not exist: {path}")]
    FileNotFound { path: PathBuf },

    /// A requested feature/configuration is unsupported or failed a safety
    /// precondition.
    #[error("unsupported feature: {0}")]
    UnsupportedFeature(String),

    /// The provided path cannot be represented as an exact valid UTF-8
    /// database key.
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

    /// A lock guarding shared cache state was poisoned by a panic in another
    /// thread. The data behind it may reflect a partially completed operation;
    /// this error stops the panic propagating to callers who did nothing wrong,
    /// and does not attempt to repair state.
    #[error("lock poisoned: {resource}")]
    Poisoned { resource: &'static str },

    /// An AES-256-GCM encryption or decryption failure.
    ///
    /// Possible causes: wrong key, corrupted data, missing `encryption` feature
    /// when trying to decrypt an encrypted entry.
    #[cfg(feature = "encryption")]
    #[error("encryption error: {0}")]
    EncryptionError(String),

    /// An async task spawned via `spawn_blocking` panicked.
    ///
    /// Applies to the Tokio (`async`), async-std (`async-std`), and smol
    /// (`smol`) runtime backends.
    #[cfg(any(feature = "async", feature = "async-std", feature = "smol"))]
    #[error("async blocking task panicked")]
    AsyncTaskPanicked,
}
