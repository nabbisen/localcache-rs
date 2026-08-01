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
use crate::error::LocalFileCacheError;

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
    /// single transaction.  Returns the number of entries imported.
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
                    LocalFileCacheError::UnsupportedFeature(format!(
                        "base64 decode error for '{}': {e}",
                        r.path
                    ))
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

        repository::import_rows(&self.conn, &self.namespace, &rows?)
    }

    /// Copy all entries from `source` into the current namespace.
    ///
    /// This is equivalent to `self.import_entries(&source.export_entries()?)`,
    /// but avoids the Base64 round-trip by operating directly on raw bytes.
    /// Returns the number of entries copied.
    ///
    /// The two engines may point to different databases or different namespaces
    /// within the same database.
    pub fn import_from<U>(&self, source: &CacheEngine<U>) -> Result<usize, LocalFileCacheError>
    where
        U: Serialize + DeserializeOwned,
    {
        self.guard_write()?;
        let rows = repository::load_all_full(&source.conn, &source.namespace)?;
        repository::import_rows(&self.conn, &self.namespace, &rows)
    }

    /// List all distinct namespace names present in the current database.
    ///
    /// Returns names sorted alphabetically.  Useful for inspecting which
    /// namespaces exist before running maintenance or migration tasks.
    pub fn namespace_list(&self) -> Result<Vec<String>, LocalFileCacheError> {
        repository::list_namespaces(&self.conn)
    }

    /// Copy all entries from `source_namespace` into `dest_namespace`.
    ///
    /// The source and destination may be in the **same** database file (this
    /// engine's database) or in different files — pass `source` as any
    /// `CacheEngine` opened on the source database.
    ///
    /// Already-existing entries in `dest_namespace` for the same path are
    /// replaced.  Returns the number of entries copied.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use localcache::CacheEngine;
    /// # let src = CacheEngine::<Vec<f32>>::builder().database(":memory:").build()?;
    /// let dst = CacheEngine::<Vec<f32>>::builder()
    ///     .database("dst.sqlite3")
    ///     .namespace("v2")
    ///     .build()?;
    /// let n = dst.namespace_copy(&src)?;
    /// println!("copied {n} entries");
    /// # Ok::<(), localcache::LocalFileCacheError>(())
    /// ```
    pub fn namespace_copy<U>(&self, source: &CacheEngine<U>) -> Result<usize, LocalFileCacheError>
    where
        U: Serialize + DeserializeOwned,
    {
        // Phase 23 P0 Part B: identical to `import_from` -- same three
        // steps, same guard, same repository calls. The two names exist
        // for different call-site framing (namespace-to-namespace copy vs.
        // copy-from-another-engine), not different behavior, so this
        // delegates rather than duplicating the body.
        self.import_from(source)
    }
}
