//! [`CacheEngine`] implementation.

use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::Connection;
use serde::{Serialize, de::DeserializeOwned};

use crate::cache::entry::{CacheEntry, CacheStatus};
use crate::cache::options::{CacheOptions, ChangeDetectionMode};
use crate::db::{repository, schema};
use crate::detection::hash::compute_full_hash;
use crate::detection::metadata::collect_metadata;
use crate::detection::strategy::detect_change;
use crate::error::LocalFileCacheError;
use crate::path::normalize_path;
use crate::serialization::{deserialize_payload, serialize_payload};

// ---------------------------------------------------------------------------
// Public result types for batch operations
// ---------------------------------------------------------------------------

/// Result summary returned by [`CacheEngine::batch_set`].
#[derive(Debug, Default)]
pub struct BatchSetReport {
    /// Number of entries that were stored successfully.
    pub succeeded: usize,
    /// Per-item errors: `(canonical_path_or_raw_input, error)`.
    pub failed: Vec<(PathBuf, LocalFileCacheError)>,
}

// ---------------------------------------------------------------------------
// CacheEngine
// ---------------------------------------------------------------------------

/// The main entry point for `localcache`.
///
/// `CacheEngine<T>` manages a SQLite-backed store that associates canonical
/// file paths with arbitrary, serialisable payloads.  A typical payload is a
/// vector embedding, a parsed document structure, or any other costly-to-compute
/// value derived from a local file.
///
/// `T` must implement [`serde::Serialize`] and [`serde::de::DeserializeOwned`]
/// because payloads are stored as bincode bytes.
///
/// # Example
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
    conn: Connection,
    mode: ChangeDetectionMode,
    namespace: String,
    ttl: Option<Duration>,
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
    /// The SQLite database file is created if it does not exist.  The schema
    /// is applied (and migrated if necessary) on every open.
    pub fn open(options: CacheOptions) -> Result<Self, LocalFileCacheError> {
        let conn = Connection::open(&options.database_path)?;

        // Apply configurable PRAGMAs before any DML.
        conn.execute_batch(&format!(
            "PRAGMA journal_mode = {}; PRAGMA synchronous = {};",
            options.journal_mode.as_str(),
            options.synchronous.as_str(),
        ))?;

        schema::initialize(&conn)?;

        Ok(Self {
            conn,
            mode: options.change_detection_mode,
            namespace: options.namespace,
            ttl: options.ttl,
            _phantom: PhantomData,
        })
    }

    // ------------------------------------------------------------------
    // Reads
    // ------------------------------------------------------------------

    /// Return the cached entry for `path`, if one exists.
    ///
    /// This is a pure database read; it does **not** check whether the
    /// on-disk file has changed.  Use [`get_if_fresh`](Self::get_if_fresh)
    /// when you need change detection.
    pub fn get<P>(&self, path: P) -> Result<Option<CacheEntry<T>>, LocalFileCacheError>
    where
        P: AsRef<Path>,
    {
        let canonical = normalize_path(path.as_ref())?;
        let path_str = path_to_str(&canonical)?;

        let Some(row) = repository::find_file(&self.conn, &self.namespace, path_str)? else {
            return Ok(None);
        };

        let Some(bytes) = repository::load_payload(&self.conn, row.id)? else {
            return Ok(None);
        };

        let payload: T = deserialize_payload(&bytes)?;
        Ok(Some(CacheEntry {
            path: PathBuf::from(&row.path),
            metadata: row.metadata,
            payload,
        }))
    }

    /// Return the cached entry for `path` only if it is still fresh.
    ///
    /// Returns `Ok(None)` when:
    /// * the file does not exist on disk,
    /// * no cache entry exists,
    /// * the change-detection check reports [`CacheStatus::Stale`],
    /// * or the entry is older than the configured TTL.
    pub fn get_if_fresh<P>(&self, path: P) -> Result<Option<CacheEntry<T>>, LocalFileCacheError>
    where
        P: AsRef<Path>,
    {
        let canonical = match normalize_path(path.as_ref()) {
            Ok(p) => p,
            Err(LocalFileCacheError::FileNotFound { .. }) => return Ok(None),
            Err(e) => return Err(e),
        };
        let path_str = path_to_str(&canonical)?;

        let Some(row) = repository::find_file(&self.conn, &self.namespace, path_str)? else {
            return Ok(None);
        };

        if is_expired(row.updated_at, self.ttl) {
            return Ok(None);
        }

        match detect_change(&canonical, &row.metadata, self.mode)? {
            CacheStatus::Stale | CacheStatus::Missing => return Ok(None),
            CacheStatus::Fresh => {}
        }

        let Some(bytes) = repository::load_payload(&self.conn, row.id)? else {
            return Ok(None);
        };

        let payload: T = deserialize_payload(&bytes)?;
        Ok(Some(CacheEntry {
            path: PathBuf::from(&row.path),
            metadata: row.metadata,
            payload,
        }))
    }

    // ------------------------------------------------------------------
    // Batch reads
    // ------------------------------------------------------------------

    /// Retrieve multiple cache entries in a single pass.
    ///
    /// Returns one `Result<Option<CacheEntry<T>>, LocalFileCacheError>` per
    /// input path, in the same order.  Individual errors do not abort the
    /// remaining lookups.  No change-detection is performed; use
    /// [`batch_get_fresh`](Self::batch_get_fresh) for that.
    pub fn batch_get<P>(
        &self,
        paths: &[P],
    ) -> Vec<Result<Option<CacheEntry<T>>, LocalFileCacheError>>
    where
        P: AsRef<Path>,
    {
        paths.iter().map(|p| self.get(p.as_ref())).collect()
    }

    /// Retrieve multiple cache entries, returning only those that are still fresh.
    ///
    /// Like [`batch_get`](Self::batch_get) but applies change-detection and TTL
    /// checks to each entry.
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

    /// Store `payload` for `path`.
    ///
    /// Current file metadata is captured automatically.  If an entry already
    /// exists it is replaced atomically within a single transaction.
    pub fn set<P>(&self, path: P, payload: &T) -> Result<(), LocalFileCacheError>
    where
        P: AsRef<Path>,
    {
        let canonical = normalize_path(path.as_ref())?;
        let path_str = path_to_str(&canonical)?;

        let mut metadata = collect_metadata(&canonical)?;
        if needs_hash(self.mode) {
            metadata.hash = Some(compute_full_hash(&canonical)?);
        }

        let bytes = serialize_payload(payload)?;
        repository::upsert(&self.conn, &self.namespace, path_str, &metadata, &bytes)?;
        Ok(())
    }

    /// Store multiple `(path, payload)` pairs inside a **single transaction**.
    ///
    /// This is significantly faster than calling [`set`](Self::set) in a loop
    /// because the SQLite commit overhead is paid only once.
    ///
    /// Failures for individual items are collected in
    /// [`BatchSetReport::failed`] rather than aborting the whole batch.
    /// However, if the transaction itself cannot be opened or committed the
    /// method returns an `Err`.
    ///
    /// # Atomicity
    ///
    /// All items that did not produce per-item errors are committed together.
    /// If the final commit fails, no items are stored.
    pub fn batch_set<P>(&self, items: &[(P, T)]) -> Result<BatchSetReport, LocalFileCacheError>
    where
        P: AsRef<Path>,
    {
        let mut report = BatchSetReport::default();
        let mut prepared: Vec<(String, crate::detection::metadata::FileMetadata, Vec<u8>)> =
            Vec::with_capacity(items.len());

        // Phase 1: normalise, collect metadata, serialise — all outside the TX.
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

            if needs_hash(self.mode) {
                match compute_full_hash(&canonical) {
                    Ok(h) => metadata.hash = Some(h),
                    Err(e) => {
                        report.failed.push((canonical.clone(), e));
                        continue;
                    }
                }
            }

            let bytes = match serialize_payload(payload) {
                Ok(b) => b,
                Err(e) => {
                    report.failed.push((canonical.clone(), e));
                    continue;
                }
            };

            prepared.push((path_str, metadata, bytes));
        }

        // Phase 2: write all prepared items in a single transaction.
        let tx = self.conn.unchecked_transaction()?;
        for (path_str, metadata, bytes) in &prepared {
            repository::upsert_in_tx(&tx, &self.namespace, path_str, metadata, bytes)?;
            report.succeeded += 1;
        }
        tx.commit()?;

        Ok(report)
    }

    // ------------------------------------------------------------------
    // Removal
    // ------------------------------------------------------------------

    /// Remove the cache entry for `path`.
    ///
    /// Returns `true` if an entry existed and was deleted, `false` otherwise.
    ///
    /// Works correctly even when `path` no longer exists on disk: the method
    /// first attempts canonical resolution, and if the file is missing it
    /// falls back to searching the database for a stored path matching the
    /// string representation of the input.
    pub fn remove<P>(&self, path: P) -> Result<bool, LocalFileCacheError>
    where
        P: AsRef<Path>,
    {
        // Try canonical first (file exists on disk).
        match normalize_path(path.as_ref()) {
            Ok(canonical) => {
                let path_str = path_to_str(&canonical)?;
                return repository::delete_by_path(&self.conn, &self.namespace, path_str);
            }
            Err(LocalFileCacheError::FileNotFound { .. }) => {}
            Err(e) => return Err(e),
        }

        // File is gone — look up by the string representation of the path as
        // stored in the DB. We try the absolute path first, then the raw input.
        let raw = path.as_ref().to_string_lossy();

        // Check if any stored path ends with / matches the provided string
        // (handles the common case where an absolute path was stored).
        let stored_paths = repository::all_paths_in_namespace(&self.conn, &self.namespace)?;
        for stored in &stored_paths {
            if stored.as_str() == raw.as_ref()
                || std::path::Path::new(stored)
                    .file_name()
                    .and_then(|n| n.to_str())
                    == std::path::Path::new(raw.as_ref())
                        .file_name()
                        .and_then(|n| n.to_str())
                    && stored.ends_with(raw.as_ref())
            {
                return repository::delete_by_path(&self.conn, &self.namespace, stored);
            }
        }

        // Direct match on raw string (last resort).
        repository::delete_by_path(&self.conn, &self.namespace, raw.as_ref())
    }

    // ------------------------------------------------------------------
    // Status
    // ------------------------------------------------------------------

    /// Check whether the cache entry for `path` is [`CacheStatus::Fresh`],
    /// [`CacheStatus::Stale`], or [`CacheStatus::Missing`].
    ///
    /// This reads metadata from disk but does **not** load the payload.
    pub fn check_status<P>(&self, path: P) -> Result<CacheStatus, LocalFileCacheError>
    where
        P: AsRef<Path>,
    {
        let canonical = match normalize_path(path.as_ref()) {
            Ok(p) => p,
            Err(LocalFileCacheError::FileNotFound { .. }) => return Ok(CacheStatus::Missing),
            Err(e) => return Err(e),
        };
        let path_str = path_to_str(&canonical)?;

        let Some(row) = repository::find_file(&self.conn, &self.namespace, path_str)? else {
            return Ok(CacheStatus::Missing);
        };

        if is_expired(row.updated_at, self.ttl) {
            return Ok(CacheStatus::Stale);
        }

        detect_change(&canonical, &row.metadata, self.mode)
    }

    // ------------------------------------------------------------------
    // Maintenance
    // ------------------------------------------------------------------

    /// Delete cache entries (in the current namespace) whose source files no
    /// longer exist on disk.
    ///
    /// Returns the number of entries removed.
    pub fn cleanup_missing_files(&self) -> Result<usize, LocalFileCacheError> {
        let paths = repository::all_paths_in_namespace(&self.conn, &self.namespace)?;
        let mut removed = 0;
        for p in &paths {
            if !std::path::Path::new(p).exists() {
                repository::delete_path(&self.conn, &self.namespace, p)?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// Delete cache entries (in the current namespace) that have exceeded the
    /// configured TTL.
    ///
    /// Returns the number of entries removed.  If no TTL is configured the
    /// method is a no-op and returns `0`.
    pub fn cleanup_expired(&self) -> Result<usize, LocalFileCacheError> {
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

    /// Reclaim disk space by running SQLite's `VACUUM` command.
    ///
    /// This is an explicit, potentially slow operation and is never called
    /// automatically by the library.
    pub fn shrink_database(&self) -> Result<(), LocalFileCacheError> {
        self.conn.execute_batch("VACUUM;")?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

fn path_to_str(path: &Path) -> Result<&str, LocalFileCacheError> {
    path.to_str()
        .ok_or_else(|| LocalFileCacheError::InvalidPath {
            path: path.to_path_buf(),
        })
}

fn needs_hash(mode: ChangeDetectionMode) -> bool {
    matches!(
        mode,
        ChangeDetectionMode::MetadataThenPartialHash
            | ChangeDetectionMode::MetadataThenFullHash
            | ChangeDetectionMode::StrictFullHash
    )
}

/// Return `true` if `updated_at` is older than `ttl` from now.
fn is_expired(updated_at: i64, ttl: Option<Duration>) -> bool {
    let Some(ttl) = ttl else {
        return false;
    };
    let now = repository::now_secs();
    let age = now.saturating_sub(updated_at);
    age as u64 >= ttl.as_secs()
}
