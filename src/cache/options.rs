//! Cache configuration types.

use std::path::PathBuf;
use std::time::Duration;

/// Selects the algorithm used to decide whether a cached entry is still valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeDetectionMode {
    /// Compare only `mtime` and `file_size`.
    MetadataOnly,
    /// Metadata first; on mismatch verify with a partial hash.
    /// *v0.1–v0.2: falls back to a full BLAKE3 hash.*
    MetadataThenPartialHash,
    /// Metadata first; on mismatch verify with a full BLAKE3 hash.
    MetadataThenFullHash,
    /// Always compute a full BLAKE3 hash regardless of metadata.
    StrictFullHash,
}

/// SQLite journal mode.
///
/// See the [SQLite documentation](https://www.sqlite.org/pragma.html#pragma_journal_mode)
/// for a detailed description of each mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JournalMode {
    /// Write-Ahead Logging — the default and recommended mode for most workloads.
    /// Allows concurrent reads while a write is in progress.
    #[default]
    Wal,
    /// The classic rollback journal.
    Delete,
    /// In-memory journal (data is lost on crash — use only for ephemeral caches).
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
///
/// Controls how aggressively SQLite flushes data to storage.  Lower values are
/// faster but offer fewer durability guarantees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SynchronousMode {
    /// No `fsync` calls — fastest but data may be lost on OS crash.
    Off,
    /// `fsync` at critical moments — a good balance of speed and safety.
    #[default]
    Normal,
    /// `fsync` at every checkpoint — safest but slowest.
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
    /// Path to the SQLite database file.  Created if it does not exist yet.
    pub database_path: PathBuf,

    /// Algorithm used to decide whether a cached entry is still valid.
    pub change_detection_mode: ChangeDetectionMode,

    /// SQLite journal mode.  Defaults to [`JournalMode::Wal`].
    pub journal_mode: JournalMode,

    /// SQLite `synchronous` setting.  Defaults to [`SynchronousMode::Normal`].
    pub synchronous: SynchronousMode,

    /// Optional time-to-live for cache entries.
    ///
    /// When set, [`crate::CacheEngine::get_if_fresh`] and
    /// [`crate::CacheEngine::batch_get_fresh`] return `None` for entries whose
    /// `updated_at` timestamp is older than `ttl`.
    ///
    /// `None` (the default) means entries never expire by age.
    pub ttl: Option<Duration>,

    /// Logical namespace for cache entries.
    ///
    /// Multiple `CacheEngine` instances can share the same SQLite file while
    /// keeping their entries isolated by using distinct namespaces.  Defaults
    /// to `"default"`.
    pub namespace: String,
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
        }
    }
}
