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
    /// Metadata first; on mismatch, a **file-size change is conclusive**
    /// (`Stale`, no hashing). Otherwise verify with a partial BLAKE3 hash
    /// (head + tail sampling, 64 KiB each). This mode detects any size
    /// change and any change within the first or last 64 KiB; it does
    /// **not** detect a same-size change confined to the middle of a
    /// larger file — use [`MetadataThenFullHash`](Self::MetadataThenFullHash)
    /// if that matters for your data.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Codec {
    /// Binary encoding via `bincode` — compact and fast.  Default.
    ///
    /// # Wire-format stability guarantee
    ///
    /// Payloads written with this codec use `bincode::config::legacy()`,
    /// which is byte-compatible with every `localcache` release from 0.1
    /// onwards.  This guarantee holds for **all 0.x and future 1.x**
    /// releases: the wire format will only change behind a headline
    /// CHANGELOG migration that transparently rewrites existing payloads.
    ///
    /// **Consequence for applications:** a `localcache` upgrade **never**
    /// requires bumping `payload_version`.  The `payload_version` field is
    /// yours to increment when *your* payload struct or pipeline changes,
    /// not when the crate version changes.
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

/// SQLite `synchronous` pragma.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SynchronousMode {
    Off,
    #[default]
    Normal,
    Full,
    Extra,
}

// ---------------------------------------------------------------------------
// ScanOptions
// ---------------------------------------------------------------------------

/// Options for [`crate::CacheEngine::scan_dir_filtered`].
#[derive(Debug, Clone, Default)]
pub struct ScanOptions {
    /// Descend into subdirectories.
    pub recursive: bool,

    /// Maximum directory depth relative to the root directory.
    ///
    /// * `None` — unlimited.
    /// * `Some(0)` — root only.
    /// * `Some(1)` — root and one level of subdirectories.
    pub max_depth: Option<usize>,

    /// Restrict to files whose extension (case-insensitive, no leading dot)
    /// matches one of these strings.  Empty list accepts all extensions.
    pub extensions: Vec<String>,

    /// Glob pattern matched against the **file name** (not the full path).
    ///
    /// Supports `*` (any sequence of Unicode scalar values), `?` (exactly one
    /// scalar), and nested/multiple `{a,b}` alternatives. Matching is
    /// case-sensitive on every platform and performs no Unicode
    /// normalization. Malformed or resource-excessive patterns are reported
    /// by `scan_dir_filtered`/`preload` before directory iteration.
    ///
    /// Examples: `"*.txt"`, `"report_???.md"`, `"data_*"`.
    ///
    /// `None` (default) disables glob filtering.
    pub glob_pattern: Option<String>,
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

    /// Payload serialization codec.
    pub codec: Codec,

    /// SQLite journal mode.  Defaults to [`JournalMode::Wal`].
    pub journal_mode: JournalMode,

    /// SQLite `synchronous` setting.  Defaults to [`SynchronousMode::Normal`].
    pub synchronous: SynchronousMode,

    /// Optional time-to-live for cache entries.
    ///
    /// An entry's age is measured from its last write. A system clock that is
    /// stepped back does not expire entries: an entry written "in the future"
    /// counts as age zero, so it stays fresh.
    ///
    /// Has **one-second resolution**: `updated_at` is stored in Unix seconds,
    /// so a duration under one second makes every entry immediately stale —
    /// even one read back in the same second it was written already exceeds
    /// it. Not rejected in this release; **from v0.22.0** a sub-second TTL
    /// will be rejected at open instead of silently accepted.
    pub ttl: Option<Duration>,

    /// Logical namespace for cache entries.  Defaults to `"default"`.
    pub namespace: String,

    /// Open an existing file-backed database in read-only mode.
    ///
    /// The database must already have the exact current localcache schema;
    /// this mode never initializes or migrates it. Every mutating operation
    /// returns [`crate::LocalFileCacheError::ReadOnly`]. Explicit read-only
    /// in-memory databases are unsupported.
    pub read_only: bool,

    /// Payload schema version.
    pub payload_version: u32,

    /// Maximum number of entries to keep in the current namespace.
    ///
    /// When exceeded after a `set` or `batch_set`, the least recently
    /// **read** entries are evicted until the count is within the limit:
    /// never-read entries (`last_accessed_at == 0`) first, then by oldest
    /// `updated_at`, then by first insertion. **A write never evicts the
    /// entries it just wrote** — `set`'s own row, or every row a
    /// `batch_set` call wrote, is excluded from that call's own eviction.
    ///
    /// Nothing here is rejected as an error in this release: `max_entries(0)`
    /// keeps only the most recently written entry, and a `batch_set` larger
    /// than `max_entries` stores every entry it reports as succeeded — the
    /// *next* write is what brings the namespace back within the bound.
    /// Rejecting these cases with an error is planned for v0.22.0.
    ///
    /// The bound is enforced by `set`/`batch_set` only — not by
    /// `import_entries` or `import_from`.
    pub max_entries: Option<usize>,

    /// AES-256-GCM encryption key (exactly 32 bytes).
    ///
    /// When set, all payloads written by this engine are encrypted with
    /// AES-256-GCM.  A fresh 96-bit nonce is generated per write; the nonce
    /// is prepended to the ciphertext in the database.
    ///
    /// **This encrypts payload content only.** Paths, namespaces, file
    /// sizes, modification times, content hashes, and timestamps are
    /// stored unencrypted — anyone who can read the database file can read
    /// them. Enabling this option does not encrypt the database file.
    ///
    /// Requires the `encryption` Cargo feature.
    ///
    /// **Important**: losing the key makes all encrypted entries permanently
    /// unreadable.
    #[cfg(feature = "encryption")]
    pub encryption_key: Option<Vec<u8>>,

    /// Compress payloads with zstd before storing (and before encrypting, if
    /// encryption is also enabled).
    ///
    /// Requires the `compression` Cargo feature.
    #[cfg(feature = "compression")]
    pub compress_payloads: bool,

    /// Register each cached path's **parent directory** for recursive
    /// watching (instead of registering every file individually) when
    /// [`crate::CacheEngine::watcher`] or
    /// [`crate::CacheEngine::debounced_watcher`] is called.
    ///
    /// With directory watching, OS events arrive for *all* files in the
    /// watched subtrees; the watcher callback filters them so that only
    /// files with a corresponding cache entry trigger invalidation.
    /// Files cached *after* the watcher starts are covered automatically
    /// as long as they live under an already-watched directory.
    ///
    /// Defaults to `false` (per-file registration, the pre-0.17 behaviour).
    ///
    /// Requires the `watching` Cargo feature.
    #[cfg(feature = "watching")]
    pub watch_dirs: bool,

    /// Open the database in read-only **shared-cache** mode using a SQLite
    /// `file:` URI (`mode=ro&cache=shared`).
    ///
    /// Multiple [`crate::CacheEngine`] instances opened with this option on
    /// the same `database_path` within the same process share the SQLite
    /// page cache, reducing memory usage and read-lock overhead.
    /// `PRAGMA query_only = ON` is additionally enforced on the connection
    /// as defence in depth.
    ///
    /// For file-backed databases this implies `read_only = true`; write
    /// methods return [`crate::LocalFileCacheError::ReadOnly`].
    ///
    /// As a special case, `":memory:"` combined with `shared_cache` opens a
    /// **named shared in-memory database**
    /// (`file::memory:?cache=shared`) in read-*write* mode — a read-only
    /// fresh in-memory database would be permanently empty and therefore
    /// useless.  All engines opened this way within one process share the
    /// same in-memory data; this is primarily a testing convenience.
    pub shared_cache: bool,
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
            #[cfg(feature = "encryption")]
            encryption_key: None,
            #[cfg(feature = "compression")]
            compress_payloads: false,
            #[cfg(feature = "watching")]
            watch_dirs: false,
            shared_cache: false,
        }
    }
}

/// Returns `true` when `path` refers to an in-memory SQLite database.
pub(crate) fn is_memory_path(path: &std::path::Path) -> bool {
    path == std::path::Path::new(":memory:")
}
