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
    /// The records come **oldest use first** (ties by the order the entries
    /// were stored), so importing them in order into a fresh database keeps
    /// the relative recency of entries used within the same second, which the
    /// public `last_accessed_at` (seconds) cannot express (RFC 026 R2).
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
    ///   payload that is not valid Base64, more distinct paths than
    ///   `max_entries`, a record whose recency is out of range (a negative
    ///   `last_accessed_at` or `updated_at`, or one so large that its
    ///   microseconds reach 2^62), or, **on an engine with an encryption key**,
    ///   a record that is not sealed bound to its path (its tag does not end in
    ///   `-aes256gcm-bound`: it is plain, or was sealed by v0.23 or earlier).
    ///   The whole import is refused before anything is written; import such
    ///   records into an engine without a key, then call
    ///   `rotate_encryption_key` on the engine with the key. The check is of the tag, not of the authenticity: a
    ///   record that is forged or sealed under another key is imported, and
    ///   fails closed when it is read, like any corrupt entry. An engine
    ///   without a key stores every record verbatim.
    /// * [`LocalFileCacheError::Database`] — including a busy database after
    ///   the busy timeout.
    ///
    /// The payload bytes are stored verbatim (still compressed/encrypted as
    /// they were when exported); no re-encoding is performed.
    ///
    /// An import is a restore, not a use (RFC 026 R2): each entry keeps the
    /// recency it was exported with, as its `last_accessed_at` seconds (or its
    /// `updated_at` for a record from before v0.23.0 that was never read, whose
    /// `last_accessed_at` is 0). Entries that share a second are ordered by
    /// the order of the records. Every later use ranks above all of them.
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
    /// On an engine with an encryption key, the rule of
    /// [`import_entries`](Self::import_entries) applies: every entry of
    /// `source` must be sealed bound to its path, or the whole copy is refused
    /// with `InvalidArgument { argument: "records" }` before anything is
    /// written. Copying between two engines with the same key, or from an
    /// engine without a key into another, is not affected.
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
    /// * with an encryption key, a record whose tag is not bound is rejected
    ///   the same way (RFC 029 R2), so an engine never stores what it would
    ///   refuse to read.
    ///
    /// Returns the number of records imported.
    fn import_bounded(&self, rows: &[repository::FullRow]) -> Result<usize, LocalFileCacheError> {
        if rows.is_empty() {
            return Ok(0);
        }
        // RFC 029 R2: with a key, only bound records are accepted. The tag is
        // checked, not the authenticity.
        #[cfg(feature = "encryption")]
        if self.encryption_key.borrow().is_some() {
            let unbound = rows
                .iter()
                .filter(|row| {
                    crate::serialization::parse_tag(&row.encoding).1
                        != crate::serialization::Sealing::Bound
                })
                .count();
            if unbound > 0 {
                return Err(invalid_argument(
                    ArgumentName::Records,
                    format!(
                        "{unbound} of {} records are not sealed bound to their path (they are \
                         plain, or were sealed before v0.24.0), and an engine with an encryption \
                         key accepts only bound records; import them into an engine without a key \
                         and call rotate_encryption_key, or purge them",
                        rows.len()
                    ),
                ));
            }
        }
        // RFC 026 F2: refuse the whole import, before writing, if any record's
        // recency cannot be held by the use order.
        if rows.iter().any(|row| {
            repository::use_value_of_record(row.last_accessed_at, row.updated_at).is_none()
        }) {
            return Err(repository::unrepresentable_recency());
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
