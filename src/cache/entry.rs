//! Cache entry and status types.

use std::path::PathBuf;

pub use crate::detection::metadata::FileMetadata;

/// The status of a cache entry relative to the current state of the file on
/// disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheStatus {
    /// The cached payload is up to date with the current file.
    Fresh,
    /// No cache entry exists for this file, or the file itself no longer
    /// exists.
    Missing,
    /// The file has changed since the payload was cached.
    Stale,
}

/// A cache entry returned from [`crate::CacheEngine::get`] or
/// [`crate::CacheEngine::get_if_fresh`].
#[derive(Debug, Clone)]
pub struct CacheEntry<T> {
    /// Canonical path to the source file.
    pub path: PathBuf,
    /// File metadata recorded at the time the entry was stored.
    pub metadata: FileMetadata,
    /// The cached computation result.
    pub payload: T,
}
