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
    ///
    /// The payload is opaque: [`DatabaseError::sqlite_extended_code`] gives the
    /// SQLite result code when there is one, and
    /// [`std::error::Error::source`] the underlying error.
    #[error("database error: {0}")]
    Database(#[source] DatabaseError),

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

    /// Reserved, and not currently returned by any operation. A stored
    /// payload whose version does not match the configured version instead
    /// makes `get_if_fresh` return `None` and `check_status` return
    /// `CacheStatus::Stale`.
    #[error("payload version mismatch: stored={stored}, expected={expected}")]
    PayloadVersionMismatch { stored: u32, expected: u32 },

    /// A lock guarding shared cache state was poisoned by a panic in another
    /// thread. The data behind it may reflect a partially completed operation;
    /// this error stops the panic propagating to callers who did nothing wrong,
    /// and does not attempt to repair state.
    #[error("lock poisoned: {resource}")]
    Poisoned { resource: &'static str },

    /// An AES-256-GCM encryption or decryption failure: wrong key or
    /// corrupted data.
    ///
    /// Two related failures return different variants, not this one: an
    /// encrypted entry decoded without the `encryption` feature compiled in
    /// returns [`LocalFileCacheError::UnknownEncoding`], and an encrypted
    /// entry decoded with the feature compiled in but no key configured
    /// returns [`LocalFileCacheError::UnsupportedFeature`].
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

/// An error from the underlying SQLite database, as carried by
/// [`LocalFileCacheError::Database`].
///
/// The error is opaque: no database-library type appears in this crate's
/// public API. Its [`Display`](std::fmt::Display) is the database's own message,
/// [`std::error::Error::source`] returns the underlying error, and
/// [`sqlite_extended_code`](Self::sqlite_extended_code) gives the SQLite result
/// code when the database reported one.
///
/// A value of this type is created by the library only.
#[derive(Debug)]
pub struct DatabaseError {
    source: Box<dyn std::error::Error + Send + Sync + 'static>,
    extended_code: Option<i32>,
}

impl DatabaseError {
    /// The SQLite *extended* result code of this error, or `None` when the
    /// failure did not come from SQLite reporting a code (for example a
    /// query that returned no rows, or a value that could not be converted).
    ///
    /// The *primary* result code is `code & 0xff`. For example a busy database
    /// is `SQLITE_BUSY`, primary code 5, so
    /// `error.sqlite_extended_code().map(|code| code & 0xff) == Some(5)` tells a
    /// caller the database was busy without naming any database library. The
    /// extended codes are listed in the SQLite documentation, "Result and Error
    /// Codes".
    pub fn sqlite_extended_code(&self) -> Option<i32> {
        self.extended_code
    }

    pub(crate) fn from_sqlite(error: rusqlite::Error) -> Self {
        let extended_code = match &error {
            rusqlite::Error::SqliteFailure(failure, _) => Some(failure.extended_code),
            _ => None,
        };
        Self {
            source: Box::new(error),
            extended_code,
        }
    }
}

impl std::fmt::Display for DatabaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.source, f)
    }
}

impl std::error::Error for DatabaseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&*self.source)
    }
}

/// Crate-private conversion of a database result into the library's error
/// type. It replaces the `From<rusqlite::Error>` impl, which would have been a
/// public trait impl.
pub(crate) trait SqliteResultExt<T> {
    fn db(self) -> Result<T, LocalFileCacheError>;
}

impl<T> SqliteResultExt<T> for Result<T, rusqlite::Error> {
    fn db(self) -> Result<T, LocalFileCacheError> {
        self.map_err(|error| LocalFileCacheError::Database(DatabaseError::from_sqlite(error)))
    }
}

#[cfg(test)]
mod tests;
