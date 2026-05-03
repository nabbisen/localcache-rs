//! Cache configuration types.

use std::path::PathBuf;
use std::time::Duration;

/// Selects the algorithm used to decide whether a cached entry is still valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeDetectionMode {
    /// Compare only `mtime` and `file_size`.
    MetadataOnly,
    /// Metadata first; on mismatch verify with a partial BLAKE3 hash
    /// (head + tail sampling, 64 KiB each).
    MetadataThenPartialHash,
    /// Metadata first; on mismatch verify with a full BLAKE3 hash.
    MetadataThenFullHash,
    /// Always compute a full BLAKE3 hash regardless of metadata.
    StrictFullHash,
}

/// SQLite journal mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JournalMode {
    /// Write-Ahead Logging — recommended for most workloads.
    #[default]
    Wal,
    /// The classic rollback journal.
    Delete,
    /// In-memory journal (ephemeral).
    Memory,
}

impl JournalMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            JournalMode::Wal => "WAL",
            JournalMode::Delete => "DELETE",
            JournalMode::Memory => "MEMORY",
        }
    }
}

/// SQLite `synchronous` pragma.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SynchronousMode {
    /// No `fsync` calls.
    Off,
    /// `fsync` at critical moments.
    #[default]
    Normal,
    /// `fsync` at every checkpoint.
    Full,
    /// Like `Full` plus extra syncs on directory entries.
    Extra,
}

impl SynchronousMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            SynchronousMode::Off => "OFF",
            SynchronousMode::Normal => "NORMAL",
            SynchronousMode::Full => "FULL",
            SynchronousMode::Extra => "EXTRA",
        }
    }
}

/// Configuration for opening a [`crate::CacheEngine`].
#[derive(Debug, Clone)]
pub struct CacheOptions {
    /// Path to the SQLite database file, or `":memory:"` for an in-memory
    /// database.
    pub database_path: PathBuf,

    /// Algorithm used to decide whether a cached entry is still valid.
    pub change_detection_mode: ChangeDetectionMode,

    /// SQLite journal mode.  Defaults to [`JournalMode::Wal`].
    pub journal_mode: JournalMode,

    /// SQLite `synchronous` setting.  Defaults to [`SynchronousMode::Normal`].
    pub synchronous: SynchronousMode,

    /// Optional time-to-live for cache entries.
    ///
    /// `get_if_fresh` and `batch_get_fresh` return `None` for entries older
    /// than this duration.  `None` (default) means entries never expire.
    pub ttl: Option<Duration>,

    /// Logical namespace for cache entries.  Defaults to `"default"`.
    pub namespace: String,

    /// Open the database in read-only mode.
    ///
    /// All write operations return [`crate::LocalFileCacheError::ReadOnly`].
    pub read_only: bool,

    /// Payload schema version.
    ///
    /// When non-zero, `get_if_fresh`, `check_status`, and the async equivalents
    /// treat entries whose stored version differs from this value as
    /// [`crate::CacheStatus::Stale`].  Increment this value whenever the type
    /// `T` changes in a backward-incompatible way.
    ///
    /// Defaults to `0` (version checks disabled).
    pub payload_version: u32,

    /// Compress payloads with zstd before storing.
    ///
    /// Requires the `compression` feature.  When the feature is disabled this
    /// field is ignored.
    #[cfg(feature = "compression")]
    pub compress_payloads: bool,
}

impl Default for CacheOptions {
    fn default() -> Self {
        Self {
            database_path: PathBuf::from("localcache.sqlite3"),
            change_detection_mode: ChangeDetectionMode::MetadataOnly,
            journal_mode: JournalMode::Wal,
            synchronous: SynchronousMode::Normal,
            ttl: None,
            namespace: "default".to_owned(),
            read_only: false,
            payload_version: 0,
            #[cfg(feature = "compression")]
            compress_payloads: false,
        }
    }
}

/// Returns `true` when `database_path` refers to an in-memory SQLite database.
pub(crate) fn is_memory_path(path: &std::path::Path) -> bool {
    path == std::path::Path::new(":memory:")
}
