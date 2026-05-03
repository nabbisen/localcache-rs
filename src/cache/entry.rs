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

/// Lightweight metadata about a cache entry, returned by
/// [`crate::CacheEngine::list_entries`].
///
/// Unlike [`CacheEntry`], this type does **not** include the payload, making
/// it cheap to enumerate large caches.
#[derive(Debug, Clone)]
pub struct EntryInfo {
    /// Canonical path to the source file.
    pub path: PathBuf,
    /// File metadata at the time the entry was last written.
    pub metadata: FileMetadata,
    /// Payload schema version stored with this entry.
    pub payload_version: u32,
    /// Encoding tag (e.g. `"raw"`, `"zstd"`, `"json"`, `"raw-aes256gcm"`).
    pub encoding: String,
    /// Unix timestamp (seconds) when this entry was last written via `set`.
    pub updated_at: i64,
    /// Unix timestamp (seconds) when this entry was last read via `get` or
    /// `get_if_fresh`.  `0` means the entry has never been read after being
    /// written.
    pub last_accessed_at: i64,
}
