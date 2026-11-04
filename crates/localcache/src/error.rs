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
///         // feature (`MissingEncryptionKey`, `UnsealedEntry`, `EncryptionError`,
///         // `Watcher`, `AsyncTaskPanicked`) are not named, so the match is the
///         // same in every feature combination
///     }
/// }
/// ```
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LocalFileCacheError {
    /// An error from the underlying SQLite database.
    ///
    /// The payload is opaque: [`DatabaseError::sqlite_extended_code`](crate::DatabaseError::sqlite_extended_code) gives the
    /// SQLite result code when there is one, and
    /// [`std::error::Error::source`] the underlying error.
    #[error("database error: {0}")]
    Database(#[source] crate::db::DatabaseError),

    /// An I/O error (file reading, canonicalization, etc.).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// A serialization or deserialization error (the payload could not be
    /// encoded, or the stored bytes are not a valid payload of the type).
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

    /// An AES-256-GCM encryption or decryption failure: wrong key, corrupted
    /// data, or an entry that was moved from another path (a bound entry is
    /// sealed to the path it was written for, RFC 029).
    ///
    /// Three related failures return different variants, not this one: an
    /// entry that is not sealed under the engine's key and bound to its path
    /// returns [`LocalFileCacheError::UnsealedEntry`], an
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

    /// An engine with an encryption key met an entry that is not sealed under
    /// a key and bound to its path: it is stored plain, or it was sealed by
    /// v0.23 or earlier, before entries were bound to their path (RFC 029).
    ///
    /// With a key set, only bound entries are read; anything else is refused
    /// here, never read silently. Call
    /// [`rotate_encryption_key`](crate::CacheEngine::rotate_encryption_key)
    /// once with the same key to seal every entry of the namespace bound to
    /// its path, or purge the entry. An engine without a key is not affected.
    #[cfg(feature = "encryption")]
    #[error(
        "unsealed entry: it is not sealed under this engine's key and bound to its path \
         (it is stored plain, or was sealed before v0.24.0); call rotate_encryption_key once \
         with the same key to seal it, or purge the entry"
    )]
    UnsealedEntry,

    /// The database or the connection is in a state this crate will not work
    /// with, for example a journal mode a migration cannot use, or SQLite not
    /// honouring a setting, or a use order that has run out of room (a stored
    /// use value at or above 2^62, which only a damaged or hostile file
    /// holds). The payload says which, and states whether the database was
    /// left unchanged.
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
    Records,
    Items,
}

impl ArgumentName {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            #[cfg(feature = "encryption")]
            Self::NewKey => "new_key",
            Self::PathGlob => "path_glob",
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

/// Crate-private conversion of a database result into the library's error
/// type, for the code above the database layer that still holds a connection
/// (RFC 030 slice 3 removes it). It replaces the `From<rusqlite::Error>` impl,
/// which would have been a public trait impl.
pub(crate) trait SqliteResultExt<T> {
    fn db(self) -> Result<T, LocalFileCacheError>;
}

impl<T> SqliteResultExt<T> for Result<T, rusqlite::Error> {
    fn db(self) -> Result<T, LocalFileCacheError> {
        self.map_err(|error| {
            LocalFileCacheError::Database(crate::db::DatabaseError::from_sqlite(error))
        })
    }
}

/// RFC 030 slice 2: the layer errors reach the public enum through this
/// crate-private trait, one to one. A `From` impl would be public API and
/// rustdoc would list it with the internal type.
pub(crate) trait Lift<T> {
    fn lift(self) -> Result<T, LocalFileCacheError>;
}

impl<T> Lift<T> for Result<T, crate::db::StorageError> {
    fn lift(self) -> Result<T, LocalFileCacheError> {
        self.map_err(LocalFileCacheError::from_storage)
    }
}

impl<T> Lift<T> for Result<T, crate::serialization::FormatError> {
    fn lift(self) -> Result<T, LocalFileCacheError> {
        self.map_err(LocalFileCacheError::from_format)
    }
}

impl LocalFileCacheError {
    pub(crate) fn from_storage(error: crate::db::StorageError) -> Self {
        use crate::db::StorageError as S;
        match error {
            S::Database(e) => Self::Database(e),
            S::SchemaNotCurrent => Self::SchemaNotCurrent,
            S::IncompatibleSchema(m) => Self::IncompatibleSchema(m),
            S::DatabaseState(m) => Self::DatabaseState(m),
            S::InvalidArgument { argument, reason } => Self::InvalidArgument { argument, reason },
        }
    }

    pub(crate) fn from_format(error: crate::serialization::FormatError) -> Self {
        use crate::serialization::FormatError as F;
        match error {
            F::Io(e) => Self::Io(e),
            F::Serialization(m) => Self::Serialization(m),
            F::UnknownEncoding(m) => Self::UnknownEncoding(m),
            #[cfg(feature = "compression")]
            F::PayloadTooLarge { limit } => Self::PayloadTooLarge { limit },
            #[cfg(feature = "encryption")]
            F::EncryptionError(m) => Self::EncryptionError(m),
            #[cfg(feature = "encryption")]
            F::MissingEncryptionKey => Self::MissingEncryptionKey,
            #[cfg(feature = "encryption")]
            F::UnsealedEntry => Self::UnsealedEntry,
        }
    }
}

#[cfg(test)]
mod tests;
