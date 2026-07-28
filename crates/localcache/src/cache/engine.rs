//! [`CacheEngine`] implementation.

mod maintenance;

use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};
use serde::{Serialize, de::DeserializeOwned};

use crate::cache::entry::{CacheEntry, CacheStatus, EntryInfo, PreloadReport};
use crate::cache::options::{
    CacheOptions, ChangeDetectionMode, Codec, ScanOptions, is_memory_path,
};
use crate::db::{repository, schema};
use crate::detection::hash::{compute_full_hash, compute_partial_hash};
use crate::detection::metadata::collect_metadata;
use crate::detection::strategy::detect_change;
use crate::error::LocalFileCacheError;
use crate::path::{normalize_path, path_to_str, resolve_path_key};
use crate::serialization::{decode_payload, encode_payload};

/// Type alias for the LRU eviction callback stored in [`CacheEngine`].
pub(crate) type EvictCallback = Arc<dyn Fn(&Path) + Send + Sync>;

// ---------------------------------------------------------------------------
// Public result types
// ---------------------------------------------------------------------------

/// Result summary returned by [`CacheEngine::batch_set`].
#[derive(Debug, Default)]
pub struct BatchSetReport {
    /// Number of entries stored successfully.
    pub succeeded: usize,
    /// Per-item errors: `(path, error)`.
    pub failed: Vec<(PathBuf, LocalFileCacheError)>,
}

// ---------------------------------------------------------------------------
// CacheEngine
// ---------------------------------------------------------------------------

/// The main entry point for `localcache`.
///
/// ## Quick example
///
/// ```no_run
/// use localcache::{CacheEngine, CacheOptions, ChangeDetectionMode};
///
/// let engine = CacheEngine::<Vec<f32>>::open(CacheOptions {
///     database_path: "cache.sqlite3".into(),
///     change_detection_mode: ChangeDetectionMode::MetadataThenFullHash,
///     ..CacheOptions::default()
/// })?;
///
/// engine.set("sample.txt", &vec![0.1_f32, 0.2, 0.3])?;
///
/// if let Some(entry) = engine.get_if_fresh("sample.txt")? {
///     println!("cached: {:?}", entry.payload);
/// }
/// # Ok::<(), localcache::LocalFileCacheError>(())
/// ```
pub struct CacheEngine<T> {
    pub(crate) conn: Connection,
    #[cfg(feature = "watching")]
    pub(crate) database_path: std::path::PathBuf,
    #[cfg(feature = "watching")]
    pub(crate) watch_dirs: bool,
    pub(crate) mode: ChangeDetectionMode,
    pub(crate) codec: Codec,
    pub(crate) namespace: String,
    pub(crate) ttl: Option<Duration>,
    pub(crate) read_only: bool,
    pub(crate) payload_version: u32,
    pub(crate) compress: bool,
    pub(crate) max_entries: Option<usize>,
    /// Optional callback invoked with the path of each LRU-evicted entry.
    pub(crate) evict_callback: Option<EvictCallback>,
    #[cfg(feature = "encryption")]
    pub(crate) encryption_key: Option<[u8; 32]>,
    _phantom: PhantomData<T>,
}

impl<T> CacheEngine<T>
where
    T: Serialize + DeserializeOwned,
{
    // ------------------------------------------------------------------
    // Construction
    // ------------------------------------------------------------------

    /// Open (or create) a [`CacheEngine`] using `options`.
    ///
    /// Effective read-only opens accept only an existing database with the
    /// exact current schema. They never initialize or migrate a database, and
    /// an explicit read-only in-memory request is rejected.
    pub fn open(options: CacheOptions) -> Result<Self, LocalFileCacheError> {
        let is_memory = is_memory_path(&options.database_path);

        if options.read_only && is_memory {
            return Err(LocalFileCacheError::UnsupportedFeature(
                "read-only mode does not support in-memory databases".into(),
            ));
        }

        // `shared_cache` on a file-backed database implies read-only.
        // On `:memory:` it opens a named shared in-memory database in
        // read-write mode instead (a read-only fresh in-memory database
        // would be permanently empty).
        let read_only = options.read_only || (options.shared_cache && !is_memory);

        let mut conn = if options.shared_cache {
            if is_memory {
                // Named shared in-memory database: every connection opened
                // with this URI within the process sees the same data.
                Connection::open_with_flags(
                    "file::memory:?cache=shared",
                    OpenFlags::SQLITE_OPEN_URI
                        | OpenFlags::SQLITE_OPEN_READ_WRITE
                        | OpenFlags::SQLITE_OPEN_CREATE
                        | OpenFlags::SQLITE_OPEN_SHARED_CACHE,
                )?
            } else {
                let path_str = options.database_path.to_str().ok_or_else(|| {
                    LocalFileCacheError::InvalidPath {
                        path: options.database_path.clone(),
                    }
                })?;
                let uri = format!("file:{}?mode=ro&cache=shared", uri_encode_path(path_str));
                Connection::open_with_flags(
                    uri,
                    OpenFlags::SQLITE_OPEN_URI
                        | OpenFlags::SQLITE_OPEN_READ_ONLY
                        | OpenFlags::SQLITE_OPEN_SHARED_CACHE
                        | OpenFlags::SQLITE_OPEN_NO_MUTEX,
                )?
            }
        } else if is_memory {
            Connection::open_in_memory()?
        } else if read_only {
            Connection::open_with_flags(
                &options.database_path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?
        } else {
            Connection::open(&options.database_path)?
        };

        if read_only {
            schema::validate_read_only(&mut conn)?;
        } else {
            let outcome = schema::initialize(&mut conn, is_memory)?;
            if !is_memory {
                schema::apply_runtime_configuration(
                    &conn,
                    options.journal_mode,
                    options.synchronous,
                    outcome.schema_migration_committed,
                )?;
            }
        }

        let compress = {
            #[cfg(feature = "compression")]
            {
                options.compress_payloads
            }
            #[cfg(not(feature = "compression"))]
            {
                false
            }
        };

        #[cfg(feature = "encryption")]
        let encryption_key: Option<[u8; 32]> = match options.encryption_key {
            None => None,
            Some(ref k) => {
                let arr: [u8; 32] = k.as_slice().try_into().map_err(|_| {
                    LocalFileCacheError::UnsupportedFeature(format!(
                        "encryption key must be exactly 32 bytes, got {}",
                        k.len()
                    ))
                })?;
                Some(arr)
            }
        };

        Ok(Self {
            conn,
            #[cfg(feature = "watching")]
            database_path: options.database_path.clone(),
            #[cfg(feature = "watching")]
            watch_dirs: options.watch_dirs,
            mode: options.change_detection_mode,
            codec: options.codec,
            namespace: options.namespace,
            ttl: options.ttl,
            read_only,
            payload_version: options.payload_version,
            compress,
            max_entries: options.max_entries,
            evict_callback: None,
            #[cfg(feature = "encryption")]
            encryption_key,
            _phantom: PhantomData,
        })
    }

    // ------------------------------------------------------------------
    // Reads
    // ------------------------------------------------------------------

    /// Return the cached entry for `path`, if one exists.
    ///
    /// Existing sources resolve through canonicalization. If the source is
    /// missing, only the caller's exact valid UTF-8 stored key is used; no
    /// relative, symlink, basename, or suffix alias is guessed.
    ///
    /// Updates `last_accessed_at` on a cache hit (LRU tracking).
    /// No change-detection or version check is performed.
    pub fn get<P>(&self, path: P) -> Result<Option<CacheEntry<T>>, LocalFileCacheError>
    where
        P: AsRef<Path>,
    {
        #[cfg(feature = "tracing")]
        let _span =
            tracing::debug_span!("localcache::get", path = %path.as_ref().display(), namespace = %self.namespace).entered();

        #[cfg(feature = "metrics")]
        metrics::counter!("localcache.get.total",
            "namespace" => self.namespace.clone())
        .increment(1);

        let resolved = resolve_path_key(path.as_ref())?;
        let path_str = resolved.key();

        let Some(row) = repository::find_file(&self.conn, &self.namespace, path_str)? else {
            #[cfg(feature = "tracing")]
            tracing::debug!("cache miss");
            #[cfg(feature = "metrics")]
            metrics::counter!("localcache.get.miss",
                "namespace" => self.namespace.clone())
            .increment(1);
            return Ok(None);
        };
        let Some(payload_row) = repository::load_payload(&self.conn, row.id)? else {
            #[cfg(feature = "metrics")]
            metrics::counter!("localcache.get.miss",
                "namespace" => self.namespace.clone())
            .increment(1);
            return Ok(None);
        };
        let payload: T = self.decode(&payload_row.content, &payload_row.encoding)?;
        if !self.read_only {
            let _ = repository::touch_last_accessed(&self.conn, row.id);
        }
        #[cfg(feature = "tracing")]
        tracing::debug!("cache hit");
        #[cfg(feature = "metrics")]
        metrics::counter!("localcache.get.hit",
            "namespace" => self.namespace.clone())
        .increment(1);
        Ok(Some(CacheEntry {
            path: PathBuf::from(&row.path),
            metadata: row.metadata,
            payload,
        }))
    }

    /// Return the cached entry for `path` only if it is still fresh.
    ///
    /// Updates `last_accessed_at` on a fresh hit (LRU tracking).
    pub fn get_if_fresh<P>(&self, path: P) -> Result<Option<CacheEntry<T>>, LocalFileCacheError>
    where
        P: AsRef<Path>,
    {
        let resolved = resolve_path_key(path.as_ref())?;
        if !resolved.exists() {
            return Ok(None);
        }
        let canonical = resolved.path();
        let path_str = resolved.key();

        let Some(row) = repository::find_file(&self.conn, &self.namespace, path_str)? else {
            return Ok(None);
        };
        if is_expired(row.updated_at, self.ttl) {
            return Ok(None);
        }
        if self.payload_version > 0 && row.payload_version != self.payload_version {
            return Ok(None);
        }
        match detect_change(canonical, &row.metadata, self.mode)? {
            CacheStatus::Stale | CacheStatus::Missing => return Ok(None),
            CacheStatus::Fresh => {}
        }
        let Some(payload_row) = repository::load_payload(&self.conn, row.id)? else {
            return Ok(None);
        };
        let payload: T = self.decode(&payload_row.content, &payload_row.encoding)?;
        if !self.read_only {
            let _ = repository::touch_last_accessed(&self.conn, row.id);
        }
        Ok(Some(CacheEntry {
            path: PathBuf::from(&row.path),
            metadata: row.metadata,
            payload,
        }))
    }

    // ------------------------------------------------------------------
    // Batch reads
    // ------------------------------------------------------------------

    pub fn batch_get<P>(
        &self,
        paths: &[P],
    ) -> Vec<Result<Option<CacheEntry<T>>, LocalFileCacheError>>
    where
        P: AsRef<Path>,
    {
        paths.iter().map(|p| self.get(p.as_ref())).collect()
    }

    pub fn batch_get_fresh<P>(
        &self,
        paths: &[P],
    ) -> Vec<Result<Option<CacheEntry<T>>, LocalFileCacheError>>
    where
        P: AsRef<Path>,
    {
        paths
            .iter()
            .map(|p| self.get_if_fresh(p.as_ref()))
            .collect()
    }

    // ------------------------------------------------------------------
    // Writes
    // ------------------------------------------------------------------

    pub fn set<P>(&self, path: P, payload: &T) -> Result<(), LocalFileCacheError>
    where
        P: AsRef<Path>,
    {
        #[cfg(feature = "tracing")]
        let _span =
            tracing::debug_span!("localcache::set", path = %path.as_ref().display(), namespace = %self.namespace).entered();

        self.guard_write()?;
        let canonical = normalize_path(path.as_ref())?;
        let path_str = path_to_str(&canonical)?;
        let mut metadata = collect_metadata(&canonical)?;
        metadata.hash = compute_hash_for_mode(&canonical, self.mode)?;
        let (bytes, encoding) = self.encode(payload)?;
        repository::upsert(
            &self.conn,
            &self.namespace,
            path_str,
            &metadata,
            &bytes,
            encoding,
            self.payload_version,
        )?;
        self.enforce_max_entries()?;
        #[cfg(feature = "tracing")]
        tracing::debug!(bytes = bytes.len(), encoding, "stored");
        #[cfg(feature = "metrics")]
        {
            metrics::counter!("localcache.set.total",
                "namespace" => self.namespace.clone())
            .increment(1);
            metrics::histogram!("localcache.set.bytes",
                "namespace" => self.namespace.clone())
            .record(bytes.len() as f64);
        }
        Ok(())
    }

    pub fn batch_set<P>(&self, items: &[(P, T)]) -> Result<BatchSetReport, LocalFileCacheError>
    where
        P: AsRef<Path>,
    {
        self.guard_write()?;

        let mut report = BatchSetReport::default();
        let mut prepared: Vec<(
            String,
            crate::detection::metadata::FileMetadata,
            Vec<u8>,
            &'static str,
        )> = Vec::with_capacity(items.len());

        for (path, payload) in items {
            let canonical = match normalize_path(path.as_ref()) {
                Ok(p) => p,
                Err(e) => {
                    report.failed.push((path.as_ref().to_path_buf(), e));
                    continue;
                }
            };
            let path_str = match path_to_str(&canonical) {
                Ok(s) => s.to_owned(),
                Err(e) => {
                    report.failed.push((canonical.clone(), e));
                    continue;
                }
            };
            let mut metadata = match collect_metadata(&canonical) {
                Ok(m) => m,
                Err(e) => {
                    report.failed.push((canonical.clone(), e));
                    continue;
                }
            };
            match compute_hash_for_mode(&canonical, self.mode) {
                Ok(h) => metadata.hash = h,
                Err(e) => {
                    report.failed.push((canonical.clone(), e));
                    continue;
                }
            }
            let (bytes, encoding) = match self.encode(payload) {
                Ok(r) => r,
                Err(e) => {
                    report.failed.push((canonical.clone(), e));
                    continue;
                }
            };
            prepared.push((path_str, metadata, bytes, encoding));
        }

        let tx = self.conn.unchecked_transaction()?;
        for (path_str, metadata, bytes, encoding) in &prepared {
            repository::upsert_in_tx(
                &tx,
                &self.namespace,
                path_str,
                metadata,
                bytes,
                encoding,
                self.payload_version,
            )?;
            report.succeeded += 1;
        }
        tx.commit()?;
        self.enforce_max_entries()?;
        Ok(report)
    }

    // ------------------------------------------------------------------
    // Removal
    // ------------------------------------------------------------------

    pub fn remove<P>(&self, path: P) -> Result<bool, LocalFileCacheError>
    where
        P: AsRef<Path>,
    {
        self.guard_write()?;
        let resolved = resolve_path_key(path.as_ref())?;
        repository::delete_by_path(&self.conn, &self.namespace, resolved.key())
    }

    // ------------------------------------------------------------------
    // Status
    // ------------------------------------------------------------------

    pub fn check_status<P>(&self, path: P) -> Result<CacheStatus, LocalFileCacheError>
    where
        P: AsRef<Path>,
    {
        #[cfg(feature = "tracing")]
        let _span = tracing::debug_span!(
            "localcache::check_status",
            path = %path.as_ref().display(),
            namespace = %self.namespace,
        )
        .entered();

        let resolved = resolve_path_key(path.as_ref())?;
        if !resolved.exists() {
            #[cfg(feature = "tracing")]
            tracing::debug!(status = "Missing");
            return Ok(CacheStatus::Missing);
        }
        let canonical = resolved.path();
        let path_str = resolved.key();
        let Some(row) = repository::find_file(&self.conn, &self.namespace, path_str)? else {
            #[cfg(feature = "tracing")]
            tracing::debug!(status = "Missing");
            return Ok(CacheStatus::Missing);
        };
        if is_expired(row.updated_at, self.ttl) {
            #[cfg(feature = "tracing")]
            tracing::debug!(status = "Stale", reason = "ttl_expired");
            return Ok(CacheStatus::Stale);
        }
        if self.payload_version > 0 && row.payload_version != self.payload_version {
            #[cfg(feature = "tracing")]
            tracing::debug!(
                status = "Stale",
                reason = "version_mismatch",
                stored = row.payload_version,
                expected = self.payload_version,
            );
            return Ok(CacheStatus::Stale);
        }
        let status = detect_change(canonical, &row.metadata, self.mode)?;
        #[cfg(feature = "tracing")]
        tracing::debug!(status = ?status);
        Ok(status)
    }

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

    // ------------------------------------------------------------------
    // Directory scan
    // ------------------------------------------------------------------

    pub fn scan_dir<P: AsRef<Path>>(
        &self,
        dir: P,
        recursive: bool,
    ) -> Result<Vec<(PathBuf, CacheStatus)>, LocalFileCacheError> {
        self.scan_dir_filtered(
            dir,
            ScanOptions {
                recursive,
                ..ScanOptions::default()
            },
        )
    }

    /// Scan `dir` with fine-grained filtering via [`ScanOptions`].
    ///
    /// Supports extension filtering, `max_depth`, and case-sensitive glob
    /// patterns on file names (`*`, `?`, and nested/multiple `{a,b}` groups).
    /// Wildcards operate on Unicode scalar values on every platform.
    pub fn scan_dir_filtered<P: AsRef<Path>>(
        &self,
        dir: P,
        options: ScanOptions,
    ) -> Result<Vec<(PathBuf, CacheStatus)>, LocalFileCacheError> {
        let dir = dir.as_ref();
        if !dir.is_dir() {
            return Err(LocalFileCacheError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("not a directory: {}", dir.display()),
            )));
        }
        // Compile glob pattern once before walking.
        let glob = options
            .glob_pattern
            .as_deref()
            .map(crate::cache::glob::compile)
            .transpose()?;

        let files = walk_dir_filtered(dir, &options, &glob, 0)?;
        let mut results = Vec::with_capacity(files.len());
        for file in files {
            let status = self.check_status(&file)?;
            results.push((file, status));
        }
        Ok(results)
    }

    // ------------------------------------------------------------------
    // Observability
    // ------------------------------------------------------------------

    /// Return lightweight metadata for all entries in the current namespace.
    ///
    /// Entries are ordered by `updated_at` descending (most recently written
    /// first).  Payload content is **not** loaded.
    pub fn list_entries(&self) -> Result<Vec<EntryInfo>, LocalFileCacheError> {
        repository::list_entries(&self.conn, &self.namespace)
    }

    // ------------------------------------------------------------------
    // Export / import
    // ------------------------------------------------------------------

    /// Export every entry in the current namespace as a `Vec<ExportRecord>`.
    ///
    /// Each record contains the raw (possibly compressed/encrypted) payload
    /// bytes encoded as Base64, together with all metadata needed to re-import
    /// the entry.  Decryption is **not** performed during export; the bytes
    /// are transferred verbatim.
    pub fn export_entries(
        &self,
    ) -> Result<Vec<crate::cache::entry::ExportRecord>, LocalFileCacheError> {
        use crate::cache::entry::ExportRecord;
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
    pub fn import_entries(
        &self,
        records: &[crate::cache::entry::ExportRecord],
    ) -> Result<usize, LocalFileCacheError> {
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
        U: serde::Serialize + serde::de::DeserializeOwned,
    {
        self.guard_write()?;
        let rows = repository::load_all_full(&source.conn, &source.namespace)?;
        repository::import_rows(&self.conn, &self.namespace, &rows)
    }

    // ------------------------------------------------------------------
    // Batch status
    // ------------------------------------------------------------------

    /// Check the freshness of multiple paths in a single call.
    ///
    /// Returns one `Result<CacheStatus, _>` per input path, in the same order.
    /// Individual errors (e.g. I/O errors reading metadata for one file) do
    /// not abort the remaining checks.
    pub fn check_status_batch<P>(
        &self,
        paths: &[P],
    ) -> Vec<Result<CacheStatus, LocalFileCacheError>>
    where
        P: AsRef<Path>,
    {
        paths
            .iter()
            .map(|p| self.check_status(p.as_ref()))
            .collect()
    }

    // ------------------------------------------------------------------
    // Key rotation
    // ------------------------------------------------------------------

    /// Re-encrypt all entries in the current namespace with `new_key`.
    ///
    /// Every payload whose encoding ends in `"-aes256gcm"` is decrypted with
    /// the current key and re-encrypted with `new_key`.  The operation is
    /// performed inside a single SQLite transaction so that a failure leaves
    /// the database consistent (still encrypted with the old key).
    ///
    /// Returns the number of entries that were re-encrypted.
    ///
    /// # Errors
    ///
    /// * [`LocalFileCacheError::ReadOnly`] — engine is in read-only mode.
    /// * [`LocalFileCacheError::UnsupportedFeature`] — no encryption key is
    ///   currently set on this engine (nothing to rotate).
    /// * [`LocalFileCacheError::EncryptionError`] — decryption or re-encryption
    ///   failed.
    #[cfg(feature = "encryption")]
    pub fn rotate_encryption_key(&self, new_key: &[u8]) -> Result<usize, LocalFileCacheError> {
        self.guard_write()?;

        let old_key = self.encryption_key.ok_or_else(|| {
            LocalFileCacheError::UnsupportedFeature(
                "rotate_encryption_key requires an existing encryption key on this engine".into(),
            )
        })?;

        let new_key_arr: [u8; 32] = new_key.try_into().map_err(|_| {
            LocalFileCacheError::UnsupportedFeature(format!(
                "new encryption key must be exactly 32 bytes, got {}",
                new_key.len()
            ))
        })?;

        // Load all encrypted payload rows for this namespace.
        let rows = repository::load_encrypted_payloads(&self.conn, &self.namespace)?;
        if rows.is_empty() {
            return Ok(0);
        }

        // Re-encrypt each row; collect updates before opening the transaction
        // to keep the borrow of `self.conn` clean.
        let mut updates: Vec<(i64, Vec<u8>)> = Vec::with_capacity(rows.len());
        for row in &rows {
            // Decrypt with old key.
            let plaintext = crate::serialization::decrypt_for_rotation(&row.content, &old_key)?;
            // Re-encrypt with new key.
            let ciphertext = crate::serialization::encrypt_for_rotation(&plaintext, &new_key_arr)?;
            updates.push((row.file_id, ciphertext));
        }

        // Write all updates atomically.
        let tx = self.conn.unchecked_transaction()?;
        for (file_id, new_content) in &updates {
            repository::update_payload_content(&tx, *file_id, new_content)?;
        }
        tx.commit()?;

        Ok(updates.len())
    }

    // ------------------------------------------------------------------
    // Builder entrypoint
    // ------------------------------------------------------------------

    /// Return a fluent builder for constructing a [`CacheEngine`].
    ///
    /// # Example
    ///
    /// ```no_run
    /// use std::time::Duration;
    /// use localcache::{CacheEngine, ChangeDetectionMode};
    ///
    /// let engine = CacheEngine::<Vec<f32>>::builder()
    ///     .database("cache.sqlite3")
    ///     .namespace("embeddings")
    ///     .change_detection(ChangeDetectionMode::MetadataThenFullHash)
    ///     .ttl(Duration::from_secs(3600))
    ///     .max_entries(500)
    ///     .build()?;
    /// # Ok::<(), localcache::LocalFileCacheError>(())
    /// ```
    pub fn builder() -> crate::cache::builder::CacheEngineBuilder<T> {
        crate::cache::builder::CacheEngineBuilder::new()
    }

    // ------------------------------------------------------------------
    // File-watching (watching feature)
    // ------------------------------------------------------------------

    /// Start a background file-system watcher for all currently cached entries.
    ///
    /// The watcher monitors source files using OS-native events (`inotify` on
    /// Linux, `kqueue` on macOS, `ReadDirectoryChanges` on Windows).  When a
    /// watched file is modified, renamed, or deleted, the corresponding cache
    /// entry is automatically removed from the database and a [`crate::WatchEvent`]
    /// is sent on the event channel.
    ///
    /// Requires the `watching` Cargo feature.
    ///
    /// Returns [`LocalFileCacheError::ReadOnly`] before creating a watcher or
    /// helper connection when this engine is read-only, because watcher events
    /// invalidate database rows.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use localcache::CacheEngine;
    ///
    /// let engine = CacheEngine::<Vec<f32>>::builder()
    ///     .database("cache.sqlite3")
    ///     .build()?;
    ///
    /// let watcher = engine.watcher()?;
    /// for event in watcher.events() {
    ///     println!("invalidated: {}", event.path.display());
    /// }
    /// # Ok::<(), localcache::LocalFileCacheError>(())
    /// ```
    #[cfg(feature = "watching")]
    pub fn watcher(&self) -> Result<crate::cache::watcher::CacheWatcher<T>, LocalFileCacheError>
    where
        T: Send + 'static,
    {
        self.guard_write()?;
        use std::sync::{Arc, Mutex};
        // Build a minimal shared state for the watcher: it only needs to open
        // its own DB connection to delete stale entries.  We pass an
        // Arc<Mutex<CacheEngine<T>>> that wraps a *new* connection so the
        // watcher callback (which runs on another thread) does not share
        // SQLite connection with the caller.
        let inner = Arc::new(Mutex::new(CacheEngine::open(
            crate::cache::options::CacheOptions {
                database_path: self.database_path.clone(),
                change_detection_mode: self.mode,
                codec: self.codec,
                namespace: self.namespace.clone(),
                ttl: self.ttl,
                read_only: false,
                payload_version: self.payload_version,
                #[cfg(feature = "compression")]
                compress_payloads: self.compress,
                #[cfg(feature = "encryption")]
                encryption_key: self.encryption_key.map(|k| k.to_vec()),
                ..crate::cache::options::CacheOptions::default()
            },
        )?));
        // Pre-load paths from *this* engine so the watcher knows what to watch.
        let paths = self.keys(None)?;
        crate::cache::watcher::CacheWatcher::new_with_paths(inner, paths, self.watch_dirs)
    }

    // ------------------------------------------------------------------
    // Bulk preload
    // ------------------------------------------------------------------

    /// Scan `dir` and cache every file using `factory` to compute the payload.
    ///
    /// `factory` receives the file path and must return `Ok(payload)` or an
    /// error.  Files for which `factory` returns an error are skipped and
    /// counted in [`PreloadReport::skipped`].
    ///
    /// Already-fresh entries are **not** recomputed — only missing or stale
    /// files are processed.  Pass `force = true` to recompute every file
    /// regardless.
    ///
    /// Returns a [`PreloadReport`] with counts of stored, skipped, and already
    /// fresh entries.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use localcache::{CacheEngine, ScanOptions};
    ///
    /// let engine = CacheEngine::<Vec<f32>>::builder()
    ///     .database("cache.sqlite3")
    ///     .build()?;
    ///
    /// let report = engine.preload(
    ///     ".",
    ///     ScanOptions { recursive: true, ..Default::default() },
    ///     false,
    ///     |path| Ok(vec![path.to_string_lossy().len() as f32]),
    /// )?;
    ///
    /// println!("stored={} skipped={} fresh={}",
    ///     report.stored, report.skipped, report.already_fresh);
    /// # Ok::<(), localcache::LocalFileCacheError>(())
    /// ```
    pub fn preload<P, F>(
        &self,
        dir: P,
        options: crate::cache::options::ScanOptions,
        force: bool,
        factory: F,
    ) -> Result<PreloadReport, LocalFileCacheError>
    where
        P: AsRef<Path>,
        F: Fn(&Path) -> Result<T, Box<dyn std::error::Error + Send + Sync>>,
    {
        self.guard_write()?;
        let scan = self.scan_dir_filtered(dir, options)?;
        let mut report = PreloadReport::default();

        for (path, status) in &scan {
            if !force && *status == crate::cache::entry::CacheStatus::Fresh {
                report.already_fresh += 1;
                continue;
            }
            match factory(path) {
                Ok(payload) => {
                    self.set(path, &payload)?;
                    report.stored += 1;
                }
                Err(e) => {
                    report.skipped += 1;
                    report.errors.push((path.clone(), e.to_string()));
                }
            }
        }
        Ok(report)
    }

    // ------------------------------------------------------------------
    // Namespace management
    // ------------------------------------------------------------------

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
        U: serde::Serialize + serde::de::DeserializeOwned,
    {
        self.guard_write()?;
        let rows = repository::load_all_full(&source.conn, &source.namespace)?;
        repository::import_rows(&self.conn, &self.namespace, &rows)
    }

    // ------------------------------------------------------------------
    // Debounced watching (watching feature)
    // ------------------------------------------------------------------

    /// Start a **debounced** background watcher for all currently cached entries.
    ///
    /// Like [`watcher()`](Self::watcher) but file events within `window` of
    /// each other are merged into a single [`crate::WatchEvent`].  This prevents
    /// rapid back-to-back writes (e.g. editors that save incrementally) from
    /// generating a flood of invalidation events.
    ///
    /// Requires the `watching` Cargo feature.
    ///
    /// Returns [`LocalFileCacheError::ReadOnly`] before creating a watcher or
    /// helper connection when this engine is read-only, because watcher events
    /// invalidate database rows.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use std::time::Duration;
    /// use localcache::CacheEngine;
    ///
    /// let engine = CacheEngine::<Vec<f32>>::builder()
    ///     .database("cache.sqlite3")
    ///     .build()?;
    ///
    /// let watcher = engine.debounced_watcher(Duration::from_millis(300))?;
    /// for event in watcher.events() {
    ///     println!("debounced: {}", event.path.display());
    /// }
    /// # Ok::<(), localcache::LocalFileCacheError>(())
    /// ```
    #[cfg(feature = "watching")]
    pub fn debounced_watcher(
        &self,
        window: std::time::Duration,
    ) -> Result<crate::cache::watcher::CacheDebouncedWatcher<T>, LocalFileCacheError>
    where
        T: Send + 'static,
    {
        self.guard_write()?;
        let paths = self.keys(None)?;
        crate::cache::watcher::CacheDebouncedWatcher::new_with_paths(
            self.database_path.clone(),
            self.mode,
            self.codec,
            self.namespace.clone(),
            self.ttl,
            self.payload_version,
            paths,
            window,
            self.watch_dirs,
        )
    }

    // ------------------------------------------------------------------
    // Builder entrypoint
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // Lightweight existence / key queries
    // ------------------------------------------------------------------

    /// Return `true` if the current namespace contains a cache entry for
    /// `path`.
    ///
    /// This is cheaper than `get()` because it does not load the payload.
    pub fn contains<P: AsRef<Path>>(&self, path: P) -> Result<bool, LocalFileCacheError> {
        let resolved = resolve_path_key(path.as_ref())?;
        repository::exists(&self.conn, &self.namespace, resolved.key())
    }

    /// Return the exact stored paths of all entries in the current namespace,
    /// sorted lexicographically. Normal `set` entries are canonical; imported
    /// records may retain portable keys.
    ///
    /// Optionally filter by a SQLite `LIKE` pattern applied to the stored
    /// path string (`%` matches any sequence, `_` matches one character).
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use localcache::{CacheEngine, CacheOptions};
    /// # let engine = CacheEngine::<Vec<f32>>::open(CacheOptions::default())?;
    /// // All paths under /home/user/docs/
    /// let paths = engine.keys(Some("/home/user/docs/%"))?;
    /// # Ok::<(), localcache::LocalFileCacheError>(())
    /// ```
    pub fn keys(
        &self,
        path_like: Option<&str>,
    ) -> Result<Vec<std::path::PathBuf>, LocalFileCacheError> {
        repository::keys(&self.conn, &self.namespace, path_like, None, None, None)
    }

    // ------------------------------------------------------------------
    // Payload queries (requires `json` feature for predicates)
    // ------------------------------------------------------------------

    /// Return a [`crate::QueryBuilder`] for filtering entries by payload
    /// content.
    ///
    /// The query performs a linear scan over all entries in the namespace
    /// (subject to optional `path_like` filtering).  Suitable for small-to-
    /// medium caches or infrequent queries.
    ///
    /// Payload predicates serialise the decoded value through
    /// `serde_json::Value`, so they work with any codec; the `json` Cargo
    /// feature must be enabled.
    pub fn query(&self) -> crate::cache::query::QueryBuilder<'_, T> {
        crate::cache::query::QueryBuilder {
            core: self.core(),
            _phantom: PhantomData,
            #[cfg(feature = "json")]
            predicates: Vec::new(),
            limit: None,
            offset: 0,
            path_like: None,
            index_hint: None,
            path_in_dir: None,
            path_glob: None,
            order_by: Vec::new(),
        }
    }

    // ------------------------------------------------------------------
    // LRU touch
    // ------------------------------------------------------------------

    /// Update `last_accessed_at` for `path` to the current time.
    ///
    /// Useful for warming entries that should not be evicted by the LRU
    /// policy.  Returns `true` if the entry existed and was updated.
    /// Returns [`LocalFileCacheError::ReadOnly`] before path validation when
    /// this engine is read-only.
    pub fn touch<P: AsRef<Path>>(&self, path: P) -> Result<bool, LocalFileCacheError> {
        self.guard_write()?;
        let resolved = resolve_path_key(path.as_ref())?;
        if !resolved.exists() {
            return Ok(false);
        }
        let path_str = resolved.key();
        let Some(row) = repository::find_file(&self.conn, &self.namespace, path_str)? else {
            return Ok(false);
        };
        repository::touch_last_accessed(&self.conn, row.id)?;
        Ok(true)
    }

    // ------------------------------------------------------------------
    // Private helpers
    // ------------------------------------------------------------------

    #[inline]
    fn guard_write(&self) -> Result<(), LocalFileCacheError> {
        if self.read_only {
            Err(LocalFileCacheError::ReadOnly)
        } else {
            Ok(())
        }
    }

    fn enforce_max_entries(&self) -> Result<(), LocalFileCacheError> {
        let Some(max) = self.max_entries else {
            return Ok(());
        };
        let count = repository::count_in_namespace(&self.conn, &self.namespace)?;
        if count <= max {
            return Ok(());
        }
        let excess = count - max;
        // If there's a callback, collect the paths before deleting.
        if let Some(cb) = &self.evict_callback {
            let paths = repository::list_lru_n_paths(&self.conn, &self.namespace, excess)?;
            repository::delete_lru_n(&self.conn, &self.namespace, excess)?;
            for p in &paths {
                cb(p);
            }
        } else {
            repository::delete_lru_n(&self.conn, &self.namespace, excess)?;
        }
        Ok(())
    }

    fn encode(&self, payload: &T) -> Result<(Vec<u8>, &'static str), LocalFileCacheError> {
        encode_payload(
            payload,
            self.compress,
            self.codec,
            #[cfg(feature = "encryption")]
            self.encryption_key.as_ref(),
        )
    }

    fn decode(&self, bytes: &[u8], encoding: &str) -> Result<T, LocalFileCacheError> {
        decode_payload(
            bytes,
            encoding,
            #[cfg(feature = "encryption")]
            self.encryption_key.as_ref(),
        )
    }

    /// Borrow the payload-type-independent parts of this engine.
    ///
    /// Lets [`crate::cache::query::QueryBuilder`] (and `AsyncCacheEngine`'s
    /// query path) operate on a query result type different from this
    /// engine's own `T` without reinterpreting `CacheEngine<T>` as
    /// `CacheEngine<U>`.
    pub(crate) fn core(&self) -> EngineCore<'_> {
        EngineCore {
            conn: &self.conn,
            namespace: &self.namespace,
            #[cfg(feature = "encryption")]
            encryption_key: self.encryption_key.as_ref(),
        }
    }
}

/// Payload-type-independent borrow of a [`CacheEngine`], used by
/// [`crate::cache::query::QueryBuilder`] so it can decode a query result
/// type different from the engine's own payload type without any `unsafe`
/// reinterpretation of `CacheEngine<T>`.
///
/// Mirrors every `#[cfg]` gate on the corresponding `CacheEngine<T>` field —
/// today only `encryption_key`. `database_path` / `watch_dirs`
/// (`watching`-gated) are not read by the query path and are intentionally
/// not part of this type.
pub(crate) struct EngineCore<'e> {
    pub(crate) conn: &'e Connection,
    pub(crate) namespace: &'e str,
    #[cfg(feature = "encryption")]
    pub(crate) encryption_key: Option<&'e [u8; 32]>,
}

/// Decode `bytes` into `U`, using `core`'s configuration rather than a
/// typed `CacheEngine<U>`. The generic replacement for the removed
/// `CacheEngine::decode_pub`.
#[cfg_attr(not(feature = "encryption"), allow(unused_variables))]
pub(crate) fn decode_with<U>(
    core: &EngineCore<'_>,
    bytes: &[u8],
    encoding: &str,
) -> Result<U, LocalFileCacheError>
where
    U: DeserializeOwned,
{
    decode_payload(
        bytes,
        encoding,
        #[cfg(feature = "encryption")]
        core.encryption_key,
    )
}

// ---------------------------------------------------------------------------
// Free helpers (pub(crate) for async_engine)
// ---------------------------------------------------------------------------

pub(crate) fn compute_hash_for_mode(
    path: &Path,
    mode: ChangeDetectionMode,
) -> Result<Option<String>, LocalFileCacheError> {
    match mode {
        ChangeDetectionMode::MetadataOnly => Ok(None),
        ChangeDetectionMode::MetadataThenPartialHash => Ok(Some(compute_partial_hash(path)?)),
        ChangeDetectionMode::MetadataThenFullHash | ChangeDetectionMode::StrictFullHash => {
            Ok(Some(compute_full_hash(path)?))
        }
    }
}

pub(crate) fn is_expired(updated_at: i64, ttl: Option<Duration>) -> bool {
    let Some(ttl) = ttl else {
        return false;
    };
    let now = repository::now_secs();
    now.saturating_sub(updated_at) as u64 >= ttl.as_secs()
}

/// Percent-encode the characters that are significant inside a SQLite
/// `file:` URI path component: `%`, `#`, `?`, and space.
///
/// SQLite decodes `%XX` escapes in URI filenames, so a literal `%` must be
/// escaped first; `#` and `?` would otherwise terminate the path component.
/// No external dependency is required for this small, fixed set.
fn uri_encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        match c {
            '%' => out.push_str("%25"),
            '#' => out.push_str("%23"),
            '?' => out.push_str("%3F"),
            ' ' => out.push_str("%20"),
            _ => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Directory walking
// ---------------------------------------------------------------------------

fn walk_dir_filtered(
    dir: &Path,
    opts: &ScanOptions,
    glob: &Option<crate::cache::glob::CompiledGlob>,
    current_depth: usize,
) -> Result<Vec<PathBuf>, LocalFileCacheError> {
    let mut files = Vec::new();

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        let path = entry.path();

        if ft.is_file() {
            // Validate the complete scan candidate before any filter can skip
            // it. SQLite TEXT identity cannot represent a non-UTF-8 filename.
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| LocalFileCacheError::InvalidPath { path: path.clone() })?;
            // Extension filter.
            if !opts.extensions.is_empty() {
                let ext = match path.extension() {
                    Some(extension) => extension
                        .to_str()
                        .ok_or_else(|| LocalFileCacheError::InvalidPath { path: path.clone() })?,
                    None => "",
                }
                .to_lowercase();
                if !opts.extensions.iter().any(|e| e.to_lowercase() == ext) {
                    continue;
                }
            }
            // Glob filter (matched against file name, not full path).
            if let Some(pat) = glob {
                if !pat.matches(name) {
                    continue;
                }
            }
            files.push(path);
        } else if ft.is_dir() {
            let can_descend =
                opts.recursive && opts.max_depth.is_none_or(|max| current_depth < max);
            if can_descend {
                let sub = walk_dir_filtered(&path, opts, glob, current_depth + 1)?;
                files.extend(sub);
            }
        }
    }

    Ok(files)
}
