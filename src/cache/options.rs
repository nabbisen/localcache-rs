//! Cache configuration types.

use std::path::PathBuf;
use std::time::Duration;

/// Selects the algorithm used to decide whether a cached entry is still valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeDetectionMode {
    /// Compare only `mtime` and `file_size`.
    MetadataOnly,

    /// Metadata first; on mismatch verify with a **partial** BLAKE3 hash.
    ///
    /// Reads the first 64 KiB and last 64 KiB of the file (128 KiB total I/O
    /// maximum per check).  For files smaller than 128 KiB the entire file is
    /// hashed, which is equivalent to a full hash.
    ///
    /// Partial hashes are stored with a `"partial:"` prefix so they can be
    /// distinguished from full hashes written by other modes.
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
    /// In-memory journal (data is lost on crash).
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
    /// No `fsync` calls — fastest but fewest durability guarantees.
    Off,
    /// `fsync` at critical moments — a good balance of speed and safety.
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
    /// database (useful in tests).
    ///
    /// An in-memory database exists only for the lifetime of the
    /// [`crate::CacheEngine`] instance and is not shared between instances.
    pub database_path: PathBuf,

    /// Algorithm used to decide whether a cached entry is still valid.
    pub change_detection_mode: ChangeDetectionMode,

    /// SQLite journal mode.  Defaults to [`JournalMode::Wal`].
    ///
    /// Ignored for in-memory databases.
    pub journal_mode: JournalMode,

    /// SQLite `synchronous` setting.  Defaults to [`SynchronousMode::Normal`].
    ///
    /// Ignored for in-memory databases.
    pub synchronous: SynchronousMode,

    /// Optional time-to-live for cache entries.
    ///
    /// When set, [`crate::CacheEngine::get_if_fresh`] and
    /// [`crate::CacheEngine::batch_get_fresh`] return `None` for entries older
    /// than `ttl`.  `None` (default) means entries never expire by age.
    pub ttl: Option<Duration>,

    /// Logical namespace for cache entries.  Defaults to `"default"`.
    pub namespace: String,

    /// Open the database in read-only mode.
    ///
    /// In read-only mode all write operations (`set`, `batch_set`, `remove`,
    /// `cleanup_*`, `shrink_database`) return
    /// [`crate::LocalFileCacheError::ReadOnly`].
    ///
    /// Incompatible with `database_path = ":memory:"` (read-only in-memory
    /// databases are empty and cannot be written to, making them useless).
    ///
    /// Defaults to `false`.
    pub read_only: bool,
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
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers used by the engine
// ---------------------------------------------------------------------------

/// Returns `true` when `database_path` refers to an in-memory database.
pub(crate) fn is_memory_path(path: &std::path::Path) -> bool {
    path == std::path::Path::new(":memory:")
}
