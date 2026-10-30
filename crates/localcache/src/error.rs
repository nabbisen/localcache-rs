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
///         LocalFileCacheError::InvalidConfiguration { .. } => {}
///         LocalFileCacheError::InvalidArgument { .. } => {}
///         LocalFileCacheError::SchemaNotCurrent => {}
///         LocalFileCacheError::IncompatibleSchema(_) => {}
///         LocalFileCacheError::DatabaseState(_) => {}
///         LocalFileCacheError::InvalidPath { .. } => {}
///         LocalFileCacheError::ReadOnly => {}
///         LocalFileCacheError::UnknownEncoding(_) => {}
///         LocalFileCacheError::Poisoned { .. } => {}
///         // deliberately no `_` arm; variants that exist only with a Cargo
///         // feature (`MissingEncryptionKey`, `EncryptionError`, `Watcher`,
///         // `AsyncTaskPanicked`) are not named, so the match is the same in
///         // every feature combination
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

    /// A compressed payload would be larger than
    /// [`CacheOptions::max_decompressed_bytes`](crate::CacheOptions::max_decompressed_bytes)
    /// once decompressed. Found on **read**, for a stored compressed payload
    /// that expands past the limit (a few kilobytes of compressed data can
    /// expand to gigabytes, so decompression is bounded instead of trusted),
    /// and on **write**, for a payload that is about to be compressed and
    /// would not be readable back under the same limit.
    ///
    /// `limit` is the configured limit. The real size is not reported:
    /// decompression stops as soon as the limit is passed, and a frame's
    /// declared size is not to be trusted.
    ///
    /// Raise the option if the payload is legitimate; otherwise treat the
    /// entry as unusable. Uncompressed payloads are not limited, and this
    /// variant exists only with the `compression` Cargo feature.
    #[cfg(feature = "compression")]
    #[error(
        "payload exceeds the {limit}-byte limit for compressed payloads (CacheOptions::max_decompressed_bytes)"
    )]
    PayloadTooLarge { limit: usize },

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
    /// returns [`LocalFileCacheError::MissingEncryptionKey`].
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

    /// A configuration option is invalid or unsupported, found before the
    /// database was touched.
    ///
    /// `option` names the [`CacheOptions`](crate::CacheOptions) field or
    /// argument at fault: `database_path`, `read_only`, `encryption_key`,
    /// `size`, `max_entries` or `ttl` (and, with the `compression` feature,
    /// `max_decompressed_bytes`); `reason` says what is wrong. Fix the option and open again;
    /// nothing was created or changed.
    #[error("invalid configuration: {option}: {reason}")]
    InvalidConfiguration {
        option: &'static str,
        reason: String,
    },

    /// An argument of a call is invalid, found before any work was done.
    ///
    /// `argument` names the parameter at fault: `new_key`, `path_glob`,
    /// `index_name`, `records` or `items`; `reason` says what is wrong. Correct the
    /// argument and call again.
    #[error("invalid argument: {argument}: {reason}")]
    InvalidArgument {
        argument: &'static str,
        reason: String,
    },

    /// A read-only open was refused because the database's schema is not the
    /// current one (it is older, or empty). A read-only open never initializes
    /// or migrates a database.
    ///
    /// Open it writable once to migrate it, or use a tool built for that (the
    /// CLI's `copy --from-db --upgrade-source`). The database was not modified.
    #[error(
        "database schema is not current; a read-only open cannot migrate it; database was not modified"
    )]
    SchemaNotCurrent,

    /// The database's schema was not recognised: it is newer than this
    /// version understands, or it is not a localcache database, or it is
    /// damaged. The payload says what was found.
    ///
    /// The database was not modified. Use a version of this crate that matches
    /// the database, or restore it from a backup.
    #[error("incompatible database schema: {0}")]
    IncompatibleSchema(String),

    /// No encryption key is configured, but the operation needs one: rotating
    /// a key on an engine that has none, or decoding an encrypted entry.
    ///
    /// Configure the engine's key (`CacheOptions::encryption_key`) and retry.
    #[cfg(feature = "encryption")]
    #[error("no encryption key is configured")]
    MissingEncryptionKey,

    /// The database or the connection is in a state this crate will not work
    /// with, for example a journal mode a migration cannot use, or SQLite not
    /// honouring a setting. The payload says which, and states whether the
    /// database was left unchanged.
    #[error("database state: {0}")]
    DatabaseState(String),

    /// Setting up or running a file-system watcher failed. The payload carries
    /// the operating-system watcher's message.
    #[cfg(feature = "watching")]
    #[error("file watching failed: {0}")]
    Watcher(String),
}

/// The configuration options [`LocalFileCacheError::InvalidConfiguration`]
/// can name. A new name needs a change here, which makes the set a decision
/// that is reviewed in one place.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ConfigOption {
    DatabasePath,
    ReadOnly,
    #[cfg(feature = "encryption")]
    EncryptionKey,
    Size,
    MaxEntries,
    Ttl,
    #[cfg(feature = "compression")]
    MaxDecompressedBytes,
}

impl ConfigOption {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::DatabasePath => "database_path",
            Self::ReadOnly => "read_only",
            #[cfg(feature = "encryption")]
            Self::EncryptionKey => "encryption_key",
            Self::Size => "size",
            Self::MaxEntries => "max_entries",
            Self::Ttl => "ttl",
            #[cfg(feature = "compression")]
            Self::MaxDecompressedBytes => "max_decompressed_bytes",
        }
    }
}

/// The argument names [`LocalFileCacheError::InvalidArgument`] can carry.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ArgumentName {
    #[cfg(feature = "encryption")]
    NewKey,
    PathGlob,
    IndexName,
    Records,
    Items,
}

impl ArgumentName {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            #[cfg(feature = "encryption")]
            Self::NewKey => "new_key",
            Self::PathGlob => "path_glob",
            Self::IndexName => "index_name",
            Self::Records => "records",
            Self::Items => "items",
        }
    }
}

pub(crate) fn invalid_configuration(
    option: ConfigOption,
    reason: impl Into<String>,
) -> LocalFileCacheError {
    LocalFileCacheError::InvalidConfiguration {
        option: option.as_str(),
        reason: reason.into(),
    }
}

pub(crate) fn invalid_argument(
    argument: ArgumentName,
    reason: impl Into<String>,
) -> LocalFileCacheError {
    LocalFileCacheError::InvalidArgument {
        argument: argument.as_str(),
        reason: reason.into(),
    }
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
