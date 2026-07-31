//! Staleness diagnosis for [`CacheEngine`].
//!
//! Extracted from `engine.rs` (Phase 22 N5) to keep the primary
//! implementation file under the project's line-count guidance. This is a
//! mechanical split: `explain` changes no signature, visibility, or
//! behavior. As a child module of `cache::engine`, this module can reach
//! `CacheEngine`'s private items directly -- no visibility was widened to
//! make the move possible.

use std::path::Path;

use serde::{Serialize, de::DeserializeOwned};

use crate::cache::entry::CacheStatus;
use crate::db::repository;
use crate::error::LocalFileCacheError;
use crate::path::resolve_path_key;

use super::CacheEngine;

impl<T> CacheEngine<T>
where
    T: Serialize + DeserializeOwned,
{
    /// Return a detailed [`crate::Diagnosis`] for `path`.
    ///
    /// Unlike [`check_status`](Self::check_status), `explain` returns rich
    /// structured information about *why* an entry is in its current state:
    /// metadata differences, hash comparison results, TTL remaining time,
    /// and payload version mismatches.
    ///
    /// This is intended for debugging and CLI tooling, not for hot paths.
    pub fn explain<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<crate::cache::entry::Diagnosis, LocalFileCacheError> {
        use crate::cache::entry::{Diagnosis, MetadataDiff, PayloadVersionInfo};
        use crate::detection::hash::{compute_full_hash, compute_partial_hash, is_partial_hash};
        use crate::detection::metadata::collect_metadata;

        let path = path.as_ref();
        let resolved = resolve_path_key(path)?;
        let file_exists = resolved.exists();
        let canonical = resolved.path();
        let entry_row = repository::find_file(&self.conn, &self.namespace, resolved.key())?;

        let entry_exists = entry_row.is_some();

        if !entry_exists {
            return Ok(Diagnosis {
                path: canonical.to_path_buf(),
                status: CacheStatus::Missing,
                entry_exists: false,
                file_exists,
                ttl_remaining_secs: None,
                hash_match: None,
                metadata_diff: None,
                payload_version: None,
                summary: if file_exists {
                    "File exists on disk but has no cache entry.".into()
                } else {
                    "File does not exist on disk and has no cache entry.".into()
                },
            });
        }

        let row = entry_row.unwrap();

        // TTL check.
        let ttl_remaining_secs = self.ttl.map(|ttl| {
            let elapsed = repository::now_secs().saturating_sub(row.updated_at);
            let ttl_secs = ttl.as_secs() as i64;
            (ttl_secs - elapsed).max(0)
        });
        let ttl_expired = self
            .ttl
            .map(|_| ttl_remaining_secs == Some(0))
            .unwrap_or(false);

        // Version check.
        let pv_info = if self.payload_version > 0 {
            Some(PayloadVersionInfo {
                stored: row.payload_version,
                expected: self.payload_version,
                matches: row.payload_version == self.payload_version,
            })
        } else {
            None
        };
        let version_mismatch = pv_info.as_ref().map(|i| !i.matches).unwrap_or(false);

        // Metadata + hash diff (only if file exists).
        let (metadata_diff, hash_match) = if file_exists {
            let current = collect_metadata(canonical)?;
            let diff = MetadataDiff {
                stored_mtime: row.metadata.mtime,
                current_mtime: current.mtime,
                stored_file_size: row.metadata.file_size,
                current_file_size: current.file_size,
                mtime_changed: row.metadata.mtime != current.mtime,
                size_changed: row.metadata.file_size != current.file_size,
            };
            // Compare hash if one was stored, using whichever strategy
            // produced the stored digest (mirrors `detection::strategy`).
            let hm = if let Some(stored_hash) = &row.metadata.hash {
                let current = if is_partial_hash(stored_hash) {
                    compute_partial_hash(canonical).ok()
                } else {
                    compute_full_hash(canonical).ok()
                };
                current.map(|h| h == *stored_hash)
            } else {
                None
            };
            (Some(diff), hm)
        } else {
            (None, None)
        };

        // Overall status.
        let status = self.check_status(path)?;

        // Build summary.
        let summary = if !file_exists {
            "Source file no longer exists on disk.".into()
        } else if ttl_expired {
            format!(
                "TTL expired (entry is {} s old).",
                repository::now_secs().saturating_sub(row.updated_at)
            )
        } else if version_mismatch {
            format!(
                "Payload version mismatch: stored={}, expected={}.",
                row.payload_version, self.payload_version
            )
        } else if metadata_diff
            .as_ref()
            .map(|d| d.mtime_changed || d.size_changed)
            .unwrap_or(false)
        {
            let d = metadata_diff.as_ref().unwrap();
            match (d.mtime_changed, d.size_changed) {
                (true, true) => "Both mtime and file_size differ.".into(),
                (true, false) => "mtime changed; file_size unchanged.".into(),
                (false, true) => "file_size changed; mtime unchanged.".into(),
                (false, false) => unreachable!(),
            }
        } else if hash_match == Some(false) {
            "File content changed (hash mismatch).".into()
        } else {
            "Entry is fresh.".into()
        };

        Ok(Diagnosis {
            path: canonical.to_path_buf(),
            status,
            entry_exists,
            file_exists,
            ttl_remaining_secs,
            hash_match,
            metadata_diff,
            payload_version: pv_info,
            summary,
        })
    }
}
