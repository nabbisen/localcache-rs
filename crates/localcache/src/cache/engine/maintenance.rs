//! Maintenance and index-management operations for [`CacheEngine`].
//!
//! Extracted from `engine.rs` (RFC 015 R6) to keep the primary implementation
//! file under the project's line-count guidance. This is a mechanical split:
//! no method here changes signature, visibility, or behavior. As a child
//! module of `cache::engine`, this module can reach `CacheEngine`'s private
//! items directly — no visibility was widened to make the move possible.

use std::path::Path;

use serde::{Serialize, de::DeserializeOwned};

use crate::db::{indexes, repository};
use crate::error::LocalFileCacheError;

use super::{CacheEngine, is_expired};

impl<T> CacheEngine<T>
where
    T: Serialize + DeserializeOwned,
{
    pub fn entry_count(&self) -> Result<usize, LocalFileCacheError> {
        repository::count_in_namespace(&self.conn, &self.namespace)
    }

    pub fn entry_count_by_version(&self) -> Result<Vec<(u32, usize)>, LocalFileCacheError> {
        repository::count_by_version(&self.conn, &self.namespace)
    }

    /// Return aggregate statistics for the current namespace.
    pub fn cache_stats(&self) -> Result<crate::cache::entry::CacheStats, LocalFileCacheError> {
        use crate::cache::entry::CacheStats;

        let raw = repository::aggregate_stats(&self.conn, &self.namespace)?;
        let entries_by_encoding = repository::encoding_breakdown(&self.conn, &self.namespace)?;
        let entries_by_payload_version = repository::count_by_version(&self.conn, &self.namespace)?;

        Ok(CacheStats {
            namespace: self.namespace.clone(),
            total_entries: raw.total_entries,
            total_payload_bytes: raw.total_payload_bytes,
            oldest_updated_at: raw.oldest_updated_at,
            newest_updated_at: raw.newest_updated_at,
            entries_by_encoding,
            entries_by_payload_version,
        })
    }

    /// Remove entries whose stored paths no longer exist on disk.
    ///
    /// Returns the number of entries deleted.
    ///
    /// # Path semantics
    ///
    /// Normal `set` paths are canonical absolute paths; imported records may
    /// retain a portable valid UTF-8 key. This method iterates exact stored
    /// strings and calls `Path::exists()` on each one **without
    /// re-canonicalizing**.
    ///
    /// Consequence on **case-insensitive filesystems** (Windows, default
    /// macOS): a file renamed *only by case* still satisfies `exists()` and
    /// its entry is therefore **preserved** — the correct behaviour on such
    /// systems (the original canonical path still resolves to the file).
    ///
    /// If you need to track case-only renames explicitly, use
    /// [`check_status`][CacheEngine::check_status] per entry to compare
    /// stored vs current metadata.
    pub fn cleanup_missing_files(&self) -> Result<usize, LocalFileCacheError> {
        self.guard_write()?;
        let paths = repository::all_paths_in_namespace(&self.conn, &self.namespace)?;
        let mut removed = 0;
        for p in &paths {
            if !Path::new(p).exists() {
                repository::delete_path(&self.conn, &self.namespace, p)?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    pub fn cleanup_expired(&self) -> Result<usize, LocalFileCacheError> {
        self.guard_write()?;
        let Some(ttl) = self.ttl else {
            return Ok(0);
        };
        let rows = repository::all_file_rows_in_namespace(&self.conn, &self.namespace)?;
        let mut removed = 0;
        for (_, path, updated_at) in &rows {
            if is_expired(*updated_at, Some(ttl)) {
                repository::delete_path(&self.conn, &self.namespace, path)?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    pub fn purge_stale_versions(&self) -> Result<usize, LocalFileCacheError> {
        self.guard_write()?;
        repository::delete_by_other_version(&self.conn, &self.namespace, self.payload_version)
    }

    pub fn shrink_database(&self) -> Result<(), LocalFileCacheError> {
        self.guard_write()?;
        self.conn.execute_batch("VACUUM;")?;
        Ok(())
    }

    /// Create an additional SQLite index on `files(namespace, path)`.
    ///
    /// `name` is a suffix of 1–64 ASCII alphanumeric/underscore bytes. The
    /// full index name is prefixed with `"lc_user_"` and returned. Existing
    /// structurally valid legacy indexes remain idempotently discoverable,
    /// but an out-of-grammar legacy spelling cannot be recreated after drop.
    /// Rejected names return [`LocalFileCacheError::UnsupportedFeature`].
    pub fn create_path_index(&self, name: &str) -> Result<String, LocalFileCacheError> {
        self.guard_write()?;
        indexes::create_path_index(&self.conn, name)
    }

    /// Drop an owned main-schema user index by suffix.
    ///
    /// Returns `true` if it existed in `main` and was dropped, or `false` if
    /// no matching main-schema object exists. Structurally authorized legacy
    /// names can be removed safely; TEMP and attached-schema objects are
    /// never targets.
    pub fn drop_path_index(&self, name: &str) -> Result<bool, LocalFileCacheError> {
        self.guard_write()?;
        indexes::drop_path_index(&self.conn, name)
    }

    /// List structurally valid main-schema user indexes in alphabetical order.
    ///
    /// Each result is valid for the catalog snapshot used by this call. A
    /// later operation revalidates the index before using it.
    pub fn list_path_indexes(&self) -> Result<Vec<String>, LocalFileCacheError> {
        indexes::list_path_indexes(&self.conn)
    }
}
