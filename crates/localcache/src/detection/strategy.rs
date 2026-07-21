//! Change-detection strategies.
//!
//! Each [`ChangeDetectionMode`] variant maps to a distinct algorithm.

use std::path::Path;

use crate::cache::entry::CacheStatus;
use crate::cache::options::ChangeDetectionMode;
use crate::detection::hash::{compute_full_hash, compute_partial_hash, is_partial_hash};
use crate::detection::metadata::{FileMetadata, collect_metadata};
use crate::error::LocalFileCacheError;

/// Compare the on-disk file against `stored` metadata using `mode`.
///
/// The caller must have already verified that both the DB record and the
/// on-disk file exist.
pub(crate) fn detect_change(
    path: &Path,
    stored: &FileMetadata,
    mode: ChangeDetectionMode,
) -> Result<CacheStatus, LocalFileCacheError> {
    match mode {
        ChangeDetectionMode::MetadataOnly => detect_metadata_only(path, stored),
        ChangeDetectionMode::MetadataThenPartialHash => {
            detect_metadata_then_partial_hash(path, stored)
        }
        ChangeDetectionMode::MetadataThenFullHash => detect_metadata_then_full_hash(path, stored),
        ChangeDetectionMode::StrictFullHash => detect_strict_full_hash(path, stored),
    }
}

// ---------------------------------------------------------------------------
// Per-mode implementations
// ---------------------------------------------------------------------------

fn detect_metadata_only(
    path: &Path,
    stored: &FileMetadata,
) -> Result<CacheStatus, LocalFileCacheError> {
    let current = collect_metadata(path)?;
    if metadata_matches(stored, &current) {
        Ok(CacheStatus::Fresh)
    } else {
        Ok(CacheStatus::Stale)
    }
}

fn detect_metadata_then_partial_hash(
    path: &Path,
    stored: &FileMetadata,
) -> Result<CacheStatus, LocalFileCacheError> {
    let current = collect_metadata(path)?;
    if metadata_matches(stored, &current) {
        return Ok(CacheStatus::Fresh);
    }

    // Metadata changed — confirm with a hash.
    match &stored.hash {
        None => Ok(CacheStatus::Stale),
        Some(stored_hash) => {
            if is_partial_hash(stored_hash) {
                // Stored hash is partial: compute a new partial hash.
                let new_hash = compute_partial_hash(path)?;
                freshen_if_equal(&new_hash, stored_hash)
            } else {
                // Stored hash is a full hash (entry written with a different
                // mode).  Fall back to full comparison to stay correct.
                let new_hash = compute_full_hash(path)?;
                freshen_if_equal(&new_hash, stored_hash)
            }
        }
    }
}

fn detect_metadata_then_full_hash(
    path: &Path,
    stored: &FileMetadata,
) -> Result<CacheStatus, LocalFileCacheError> {
    let current = collect_metadata(path)?;
    if metadata_matches(stored, &current) {
        return Ok(CacheStatus::Fresh);
    }
    compare_full_hash(path, stored)
}

fn detect_strict_full_hash(
    path: &Path,
    stored: &FileMetadata,
) -> Result<CacheStatus, LocalFileCacheError> {
    compare_full_hash(path, stored)
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

#[inline]
fn metadata_matches(stored: &FileMetadata, current: &FileMetadata) -> bool {
    stored.mtime == current.mtime && stored.file_size == current.file_size
}

/// Compare the stored full hash with a freshly computed full hash.
///
/// If no hash was stored (entry saved with `MetadataOnly`) the entry is
/// conservatively treated as stale.
fn compare_full_hash(
    path: &Path,
    stored: &FileMetadata,
) -> Result<CacheStatus, LocalFileCacheError> {
    match &stored.hash {
        None => Ok(CacheStatus::Stale),
        Some(stored_hash) => {
            // Partial hash stored but caller wants full comparison: be
            // conservative and treat as stale so the caller re-caches.
            if is_partial_hash(stored_hash) {
                return Ok(CacheStatus::Stale);
            }
            let new_hash = compute_full_hash(path)?;
            freshen_if_equal(&new_hash, stored_hash)
        }
    }
}

#[inline]
fn freshen_if_equal(a: &str, b: &str) -> Result<CacheStatus, LocalFileCacheError> {
    if a == b {
        Ok(CacheStatus::Fresh)
    } else {
        Ok(CacheStatus::Stale)
    }
}
