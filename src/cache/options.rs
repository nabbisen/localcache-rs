//! Cache configuration types.

use std::path::PathBuf;
use std::time::Duration;

// ---------------------------------------------------------------------------
// ChangeDetectionMode
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Codec
// ---------------------------------------------------------------------------

/// Selects the serialization codec used to encode payload values.
///
/// Different codecs can coexist in the same database (the codec used is
/// recorded per-entry in the `payloads.encoding` column).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Codec {
    /// Binary encoding via `bincode` — compact and fast.  Default.
    #[default]
    Bincode,

    /// JSON encoding via `serde_json` — human-readable, larger on disk.
    ///
    /// Requires the `json` Cargo feature.
    #[cfg(feature = "json")]
    Json,
}

// ---------------------------------------------------------------------------
// JournalMode / SynchronousMode
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// ScanOptions
// ---------------------------------------------------------------------------

/// Options for [`crate::CacheEngine::scan_dir_filtered`].
#[derive(Debug, Clone, Default)]
pub struct ScanOptions {
    /// Descend into subdirectories.
    pub recursive: bool,

    /// Maximum directory depth (relative to the root passed to `scan_dir_filtered`).
    ///
    /// * `None` — unlimited depth (descend into all nested directories).
    /// * `Some(0)` — only the root directory (equivalent to `recursive = false`).
    /// * `Some(1)` — root and one level of subdirectories.
    pub max_depth: Option<usize>,

    /// Restrict results to files whose extension matches one of these strings.
    ///
    /// Extensions are compared case-insensitively and **without** a leading
    /// dot.  E.g., `vec!["txt".into(), "md".into()]`.
    ///
    /// An empty list means *all* extensions are accepted.
    pub extensions: Vec<String>,
}

// ---------------------------------------------------------------------------
// CacheOptions
// ---------------------------------------------------------------------------

/// Configuration for opening a [`crate::CacheEngine`].
#[derive(Debug, Clone)]
pub struct CacheOptions {
    /// Path to the SQLite database file, or `":memory:"` for an in-memory
    /// database.
    pub database_path: PathBuf,

    /// Algorithm used to decide whether a cached entry is still valid.
    pub change_detection_mode: ChangeDetectionMode,

    /// Payload serialization codec.  Defaults to [`Codec::Bincode`].
    pub codec: Codec,

    /// SQLite journal mode.  Defaults to [`JournalMode::Wal`].
    pub journal_mode: JournalMode,

    /// SQLite `synchronous` setting.  Defaults to [`SynchronousMode::Normal`].
    pub synchronous: SynchronousMode,

    /// Optional time-to-live for cache entries.
    pub ttl: Option<Duration>,

    /// Logical namespace for cache entries.  Defaults to `"default"`.
    pub namespace: String,

    /// Open the database in read-only mode.
    pub read_only: bool,

    /// Payload schema version.  Non-zero enables version checks in
    /// `get_if_fresh` and `check_status`.
    pub payload_version: u32,

    /// Maximum number of entries to keep in the current namespace.
    ///
    /// When a `set` operation causes the namespace entry count to exceed this
    /// limit, the oldest entries (by `updated_at`) are automatically removed
    /// until the count is at most `max_entries`.
    ///
    /// `None` (default) means no limit.
    pub max_entries: Option<usize>,

    /// Compress payloads with zstd before storing.
    ///
    /// Requires the `compression` feature.
    #[cfg(feature = "compression")]
    pub compress_payloads: bool,
}

impl Default for CacheOptions {
    fn default() -> Self {
        Self {
            database_path: PathBuf::from("localcache.sqlite3"),
            change_detection_mode: ChangeDetectionMode::MetadataOnly,
            codec: Codec::Bincode,
            journal_mode: JournalMode::Wal,
            synchronous: SynchronousMode::Normal,
            ttl: None,
            namespace: "default".to_owned(),
            read_only: false,
            payload_version: 0,
            max_entries: None,
            #[cfg(feature = "compression")]
            compress_payloads: false,
        }
    }
}

/// Returns `true` when `path` refers to an in-memory SQLite database.
pub(crate) fn is_memory_path(path: &std::path::Path) -> bool {
    path == std::path::Path::new(":memory:")
}
