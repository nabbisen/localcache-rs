//! Cache configuration types.

use std::path::PathBuf;

/// Selects the algorithm used to decide whether a cached entry is still valid.
///
/// Less-strict modes are faster but may occasionally miss changes; stricter
/// modes are more reliable at the cost of I/O.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeDetectionMode {
    /// Compare only `mtime` and `file_size`.  Fast but can be fooled by
    /// same-size overwrites that preserve the modification timestamp.
    MetadataOnly,

    /// Use metadata first; on mismatch, verify with a partial hash.
    ///
    /// *Initial implementation falls back to a full hash.*  Partial-hash
    /// tuning (e.g. head/tail sampling) is reserved for a future release.
    MetadataThenPartialHash,

    /// Use metadata first; on mismatch, verify with a full BLAKE3 hash.
    MetadataThenFullHash,

    /// Always compute a full BLAKE3 hash regardless of metadata.  The most
    /// reliable option, but reads the entire file on every check.
    StrictFullHash,
}

/// Configuration for opening a [`crate::CacheEngine`].
#[derive(Debug, Clone)]
pub struct CacheOptions {
    /// Path to the SQLite database file.  The file is created if it does not
    /// exist yet.
    pub database_path: PathBuf,

    /// Algorithm used when deciding whether a cached entry is still valid.
    pub change_detection_mode: ChangeDetectionMode,
}

impl Default for CacheOptions {
    fn default() -> Self {
        Self {
            database_path: PathBuf::from("localcache.sqlite3"),
            change_detection_mode: ChangeDetectionMode::MetadataOnly,
        }
    }
}
