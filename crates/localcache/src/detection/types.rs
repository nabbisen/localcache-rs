//! Freshness types (RFC 030 R2): what detection decides and how it decides.

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
