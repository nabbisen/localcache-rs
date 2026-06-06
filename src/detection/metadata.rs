//! File metadata types and collection.

use std::path::Path;
use std::time::UNIX_EPOCH;

use crate::error::LocalFileCacheError;

/// Metadata associated with a file at a point in time.
///
/// This is the public-facing type exposed via [`crate::CacheEntry`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMetadata {
    /// Modification time as **nanoseconds** since the Unix epoch.
    ///
    /// Stored with nanosecond precision (schema v5+) so that files
    /// overwritten within the same second are correctly detected as stale
    /// even when their size is unchanged.  On filesystems with coarser
    /// timestamp resolution the sub-second portion will be zero, which is
    /// still correct — the comparison remains exact.
    ///
    /// Fits in an `i64` for all dates through the year 2262
    /// (`i64::MAX ≈ 9.2 × 10¹⁸ ns`).
    pub mtime: i64,
    /// File size in bytes.
    pub file_size: u64,
    /// Optional BLAKE3 hash (hex-encoded), present when a hash-based detection
    /// mode was active at the time the entry was stored.
    pub hash: Option<String>,
}

/// Read the current [`FileMetadata`] from the filesystem for `path`.
///
/// `hash` is left as `None`; callers that need a hash must compute it
/// separately and set the field themselves.
pub(crate) fn collect_metadata(path: &Path) -> Result<FileMetadata, LocalFileCacheError> {
    let meta = std::fs::metadata(path)?;
    let mtime = meta
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0);
    let file_size = meta.len();
    Ok(FileMetadata {
        mtime,
        file_size,
        hash: None,
    })
}
