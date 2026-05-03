//! Change-detection strategies.
//!
//! Each [`ChangeDetectionMode`] variant maps to a distinct algorithm for
//! deciding whether the on-disk file still matches the cached metadata.

use std::path::Path;

use crate::cache::entry::CacheStatus;
use crate::cache::options::ChangeDetectionMode;
use crate::detection::hash::compute_full_hash;
use crate::detection::metadata::{FileMetadata, collect_metadata};
use crate::error::LocalFileCacheError;

/// Determine whether a file is [`CacheStatus::Fresh`] or [`CacheStatus::Stale`]
/// by comparing the `stored` metadata (from the database) with the current
/// state of the file on disk.
///
/// The caller must have already confirmed that both the DB record and the
/// on-disk file exist before calling this function.
pub(crate) fn detect_change(
    path: &Path,
    stored: &FileMetadata,
    mode: ChangeDetectionMode,
) -> Result<CacheStatus, LocalFileCacheError> {
    match mode {
        ChangeDetectionMode::MetadataOnly => detect_metadata_only(path, stored),
        ChangeDetectionMode::MetadataThenPartialHash => {
            // Initial implementation falls back to full hash; partial-hash
            // tuning is deferred to a future release.
            detect_metadata_then_full_hash(path, stored)
        }
        ChangeDetectionMode::MetadataThenFullHash => detect_metadata_then_full_hash(path, stored),
        ChangeDetectionMode::StrictFullHash => detect_strict_full_hash(path, stored),
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn detect_metadata_only(
    path: &Path,
    stored: &FileMetadata,
) -> Result<CacheStatus, LocalFileCacheError> {
    let current = collect_metadata(path)?;
    if stored.mtime == current.mtime && stored.file_size == current.file_size {
        Ok(CacheStatus::Fresh)
    } else {
        Ok(CacheStatus::Stale)
    }
}

fn detect_metadata_then_full_hash(
    path: &Path,
    stored: &FileMetadata,
) -> Result<CacheStatus, LocalFileCacheError> {
    let current = collect_metadata(path)?;
    // Fast path: if metadata matches, trust it as fresh.
    if stored.mtime == current.mtime && stored.file_size == current.file_size {
        return Ok(CacheStatus::Fresh);
    }
    // Metadata differs; confirm with a full hash before declaring stale.
    compare_hash(path, stored)
}

fn detect_strict_full_hash(
    path: &Path,
    stored: &FileMetadata,
) -> Result<CacheStatus, LocalFileCacheError> {
    compare_hash(path, stored)
}

/// Compare the stored hash with a freshly computed hash of `path`.
///
/// If no hash was stored (i.e., the entry was originally saved with a
/// metadata-only mode), the file is conservatively treated as stale.
fn compare_hash(path: &Path, stored: &FileMetadata) -> Result<CacheStatus, LocalFileCacheError> {
    match &stored.hash {
        None => Ok(CacheStatus::Stale),
        Some(stored_hash) => {
            let current_hash = compute_full_hash(path)?;
            if *stored_hash == current_hash {
                Ok(CacheStatus::Fresh)
            } else {
                Ok(CacheStatus::Stale)
            }
        }
    }
}
