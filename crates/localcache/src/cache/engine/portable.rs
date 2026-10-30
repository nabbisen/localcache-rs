//! Portable-record export/import and cross-engine/namespace data movement
//! for [`CacheEngine`].
//!
//! Extracted from `engine.rs` (Phase 22 N5) to keep the primary
//! implementation file under the project's line-count guidance. This is a
//! mechanical split: no method here changes signature, visibility, or
//! behavior. As a child module of `cache::engine`, this module can reach
//! `CacheEngine`'s private items directly -- no visibility was widened to
//! make the move possible.

use serde::{Serialize, de::DeserializeOwned};

use crate::cache::entry::ExportRecord;
use crate::db::repository;
use crate::error::{ArgumentName, LocalFileCacheError, SqliteResultExt, invalid_argument};

use super::CacheEngine;

impl<T> CacheEngine<T>
where
    T: Serialize + DeserializeOwned,
{
    /// Export every entry in the current namespace as a `Vec<ExportRecord>`.
    ///
    /// Each record contains the raw (possibly compressed/encrypted) payload
    /// bytes encoded as Base64, together with all metadata needed to re-import
    /// the entry.  Decryption is **not** performed during export; the bytes
    /// are transferred verbatim.
    pub fn export_entries(&self) -> Result<Vec<ExportRecord>, LocalFileCacheError> {
        use base64::{Engine as _, engine::general_purpose::STANDARD};

        let rows = repository::load_all_full(&self.conn, &self.namespace)?;
        Ok(rows
            .into_iter()
            .map(|r| ExportRecord {
                path: r.path,
                payload_b64: STANDARD.encode(&r.content),
                encoding: r.encoding,
                mtime: r.mtime,
                file_size: r.file_size,
                hash: r.hash,
                payload_version: r.payload_version,
                updated_at: r.updated_at,
                last_accessed_at: r.last_accessed_at,
            })
            .collect())
    }

    /// Import a slice of [`crate::ExportRecord`]s into the current namespace.
    ///
    /// Existing entries for the same path are replaced atomically inside a
    /// single transaction.  Returns the number of records imported.
    ///
    /// With [`max_entries`](crate::CacheOptions::max_entries) set, the import
    /// follows the rule `batch_set` follows: records for more **distinct**
    /// paths (the stored key; the same path twice counts once) than
    /// `max_entries` are rejected before anything is written, and otherwise
    /// the eviction runs in the import's own transaction and never evicts a
    /// row the import wrote. **`Ok` means the records were stored and the
    /// bound enforced. `Err` means nothing changed.**
    ///
    /// # Errors
    ///
    /// * [`LocalFileCacheError::ReadOnly`] — engine is in read-only mode.
    /// * [`LocalFileCacheError::InvalidArgument`] — `argument: "records"`: a
    ///   payload that is not valid Base64, or more distinct paths than
    ///   `max_entries`.
    /// * [`LocalFileCacheError::Database`] — including a busy database after
    ///   the busy timeout.
    ///
    /// The payload bytes are stored verbatim (still compressed/encrypted as
    /// they were when exported); no re-encoding is performed.
    pub fn import_entries(&self, records: &[ExportRecord]) -> Result<usize, LocalFileCacheError> {
        use base64::{Engine as _, engine::general_purpose::STANDARD};

        self.guard_write()?;

        let rows: Result<Vec<repository::FullRow>, LocalFileCacheError> = records
            .iter()
            .map(|r| {
                let content = STANDARD.decode(&r.payload_b64).map_err(|e| {
                    invalid_argument(
                        ArgumentName::Records,
                        format!("base64 decode error for '{}': {e}", r.path),
                    )
                })?;
                Ok(repository::FullRow {
                    path: r.path.clone(),
                    content,
                    encoding: r.encoding.clone(),
                    mtime: r.mtime,
                    file_size: r.file_size,
                    hash: r.hash.clone(),
                    payload_version: r.payload_version,
                    updated_at: r.updated_at,
                    last_accessed_at: r.last_accessed_at,
                })
            })
            .collect();

        self.import_bounded(&rows?)
    }

    /// Copy all entries from `source` into the current namespace.
    ///
    /// This is equivalent to `self.import_entries(&source.export_entries()?)`,
    /// but avoids the Base64 round-trip by operating directly on raw bytes.
    /// Returns the number of entries copied.
    ///
    /// The two engines may point to different databases or different namespaces
    /// within the same database. Entries already in the current namespace for
    /// the same path are replaced.
    ///
    /// `max_entries` applies as for
    /// [`import_entries`](Self::import_entries): a source holding more
    /// distinct entries than the bound is rejected with
    /// `InvalidArgument { argument: "records" }` before anything is written,
    /// and a source that fits evicts older entries of this namespace in the
    /// same transaction, never one it wrote.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use localcache::CacheEngine;
    /// # let src = CacheEngine::<Vec<f32>>::builder().database(":memory:").build()?;
    /// // Copy every entry of `src` into namespace "v2" of another database.
    /// let dst = CacheEngine::<Vec<f32>>::builder()
    ///     .database("dst.sqlite3")
    ///     .namespace("v2")
    ///     .build()?;
    /// let n = dst.import_from(&src)?;
    /// println!("copied {n} entries");
    /// # Ok::<(), localcache::LocalFileCacheError>(())
    /// ```
    pub fn import_from<U>(&self, source: &CacheEngine<U>) -> Result<usize, LocalFileCacheError>
    where
        U: Serialize + DeserializeOwned,
    {
        self.guard_write()?;
        let rows = repository::load_all_full(&source.conn, &source.namespace)?;
        self.import_bounded(&rows)
    }

    /// The one import path: `import_entries` and `import_from` both end
    /// here, under the rule `batch_set` follows (RFC 024 B3, RFC 025 R6):
    ///
    /// * more **distinct** stored keys than `max_entries` is rejected before
    ///   anything is written, with `InvalidArgument { argument: "records" }`;
    /// * the write and the eviction share one `IMMEDIATE` transaction, and the
    ///   eviction (`enforce_max_entries`, shared with `set` and `batch_set`)
    ///   never evicts a row this call wrote;
    /// * the eviction callback runs after the commit.
    ///
    /// Returns the number of records imported.
    fn import_bounded(&self, rows: &[repository::FullRow]) -> Result<usize, LocalFileCacheError> {
        if rows.is_empty() {
            return Ok(0);
        }
        if let Some(max_entries) = self.max_entries {
            let distinct = rows
                .iter()
                .map(|row| row.path.as_str())
                .collect::<std::collections::HashSet<_>>()
                .len();
            if distinct > max_entries {
                return Err(invalid_argument(
                    ArgumentName::Records,
                    format!("{distinct} distinct entries exceed max_entries ({max_entries})"),
                ));
            }
        }

        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )
        .db()?;
        let written = repository::import_rows_in_tx(&tx, &self.namespace, rows)?;
        let evicted = self.enforce_max_entries(&tx, &written)?;
        tx.commit().db()?;
        if let Some(callback) = &self.evict_callback {
            for path in &evicted {
                callback(path);
            }
        }
        Ok(rows.len())
    }

    /// List all distinct namespace names present in the current database.
    ///
    /// Returns names sorted alphabetically.  Useful for inspecting which
    /// namespaces exist before running maintenance or migration tasks.
    pub fn namespace_list(&self) -> Result<Vec<String>, LocalFileCacheError> {
        repository::list_namespaces(&self.conn)
    }
}
