//! Value types the database layer shares with the layers above it (RFC 030 R2).

use std::path::PathBuf;

/// SQLite journal mode.
///
/// The default, [`Preserve`](Self::Preserve), chooses nothing: it gives a new
/// database WAL and leaves an existing database as it is. Choose
/// [`Wal`](Self::Wal) or [`Delete`](Self::Delete) to set the mode every time
/// the database is opened.
///
/// `#[non_exhaustive]`: a `match` needs a wildcard arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum JournalMode {
    /// Do not choose a journal mode. The engine applies WAL to a **new**
    /// database and leaves an **existing** database's journal mode
    /// unchanged. This is the default.
    ///
    /// *New* means the database has no localcache schema when the engine
    /// opens it: the file did not exist, or it is an empty file. A database
    /// that already has a localcache schema, current or older, is existing,
    /// and keeps its mode through a migration.
    ///
    /// Read-only engines and [`ReadPool`](crate::ReadPool) never change a
    /// journal mode, whatever is chosen. An in-memory database has no
    /// journal mode to preserve.
    #[default]
    Preserve,
    /// Write-Ahead Logging — recommended for most workloads. Applied every
    /// time a writable engine opens the database, converting it if it is in
    /// another mode.
    Wal,
    /// The classic rollback journal. Applied every time a writable engine
    /// opens the database, converting it if it is in WAL.
    Delete,
    /// In-memory journal (ephemeral). Applied to the connection after the
    /// schema work, as before; it is not stored in the database file.
    Memory,
}

/// SQLite `synchronous` pragma.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SynchronousMode {
    Off,
    #[default]
    Normal,
    Full,
    Extra,
}

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

/// Lightweight metadata about a cache entry, returned by
/// [`crate::CacheEngine::list_entries`].
///
/// Unlike [`CacheEntry`](crate::CacheEntry), this type does **not** include the payload, making
/// it cheap to enumerate large caches.
#[derive(Debug, Clone)]
pub struct EntryInfo {
    /// Canonical path to the source file.
    pub path: PathBuf,
    /// File metadata at the time the entry was last written.
    pub metadata: FileMetadata,
    /// Payload schema version stored with this entry.
    pub payload_version: u32,
    /// Encoding tag (e.g. `"raw"`, `"zstd"`, `"json"`, `"raw-aes256gcm-bound"`).
    pub encoding: String,
    /// Unix timestamp (seconds) when this entry was last written via `set`.
    pub updated_at: i64,
    /// Unix timestamp (seconds) of the last **use** of this entry: a read
    /// hit (`get`, `get_if_fresh`), a `touch`, or a write (`set`, `batch_set`,
    /// an overwrite). It is never `0` for a stored entry. This is the second
    /// the eviction order is built from; eviction itself is finer than this
    /// field (RFC 026): two entries used in the same second are still in a
    /// strict order. Before v0.23.0 it meant the last **read** and was `0` for
    /// an entry never read.
    pub last_accessed_at: i64,
}
