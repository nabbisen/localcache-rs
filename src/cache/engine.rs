//! [`CacheEngine`] implementation.

use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};
use serde::{Serialize, de::DeserializeOwned};

use crate::cache::entry::{CacheEntry, CacheStatus};
use crate::cache::options::{CacheOptions, ChangeDetectionMode, is_memory_path};
use crate::db::{repository, schema};
use crate::detection::hash::{compute_full_hash, compute_partial_hash};
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
    /// Per-item errors: `(path, error)`.
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
/// ## In-memory databases
///
/// Pass `database_path: ":memory:".into()` to use an in-memory SQLite
/// database.  The cache only persists for the lifetime of the engine instance
/// and is not shared with other instances.  This mode is particularly useful
/// in unit tests.
///
/// ## Read-only mode
///
/// Set `read_only: true` in [`CacheOptions`] to open an existing database
/// without write access.  All mutation methods return
/// [`LocalFileCacheError::ReadOnly`].
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
    read_only: bool,
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
    /// * For a regular file path the SQLite database is created if it does not
    ///   exist yet, and the schema is applied (and migrated if necessary).
    /// * For `":memory:"` an in-memory database is created fresh.
    /// * For `read_only: true` the database is opened with
    ///   `SQLITE_OPEN_READ_ONLY`; the schema is **not** modified.
    pub fn open(options: CacheOptions) -> Result<Self, LocalFileCacheError> {
        let is_memory = is_memory_path(&options.database_path);

        let conn = if is_memory {
            Connection::open_in_memory()?
        } else if options.read_only {
            Connection::open_with_flags(
                &options.database_path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?
        } else {
            Connection::open(&options.database_path)?
        };

        if is_memory || !options.read_only {
            // Apply configurable PRAGMAs for writable connections.
            if !is_memory {
                conn.execute_batch(&format!(
                    "PRAGMA journal_mode = {}; PRAGMA synchronous = {};",
                    options.journal_mode.as_str(),
                    options.synchronous.as_str(),
                ))?;
            }
            schema::initialize(&conn)?;
        } else {
            // Read-only: enable FK enforcement but skip DDL.
            schema::enable_foreign_keys(&conn)?;
        }

        Ok(Self {
            conn,
            mode: options.change_detection_mode,
            namespace: options.namespace,
            ttl: options.ttl,
            read_only: options.read_only,
            _phantom: PhantomData,
        })
    }

    // ------------------------------------------------------------------
    // Reads
    // ------------------------------------------------------------------

    /// Return the cached entry for `path`, if one exists.
    ///
    /// No change-detection is performed.  Use
    /// [`get_if_fresh`](Self::get_if_fresh) for that.
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
    /// Returns `Ok(None)` when the file is missing, the entry is missing,
    /// the entry is stale, or the entry has exceeded the configured TTL.
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

    /// Retrieve multiple entries in a single pass (no change-detection).
    pub fn batch_get<P>(
        &self,
        paths: &[P],
    ) -> Vec<Result<Option<CacheEntry<T>>, LocalFileCacheError>>
    where
        P: AsRef<Path>,
    {
        paths.iter().map(|p| self.get(p.as_ref())).collect()
    }

    /// Retrieve multiple entries, returning only those that are still fresh.
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
    pub fn set<P>(&self, path: P, payload: &T) -> Result<(), LocalFileCacheError>
    where
        P: AsRef<Path>,
    {
        self.guard_write()?;
        let canonical = normalize_path(path.as_ref())?;
        let path_str = path_to_str(&canonical)?;

        let mut metadata = collect_metadata(&canonical)?;
        metadata.hash = compute_hash_for_mode(&canonical, self.mode)?;

        let bytes = serialize_payload(payload)?;
        repository::upsert(&self.conn, &self.namespace, path_str, &metadata, &bytes)?;
        Ok(())
    }

    /// Store multiple `(path, payload)` pairs in a single transaction.
    pub fn batch_set<P>(&self, items: &[(P, T)]) -> Result<BatchSetReport, LocalFileCacheError>
    where
        P: AsRef<Path>,
    {
        self.guard_write()?;

        let mut report = BatchSetReport::default();
        let mut prepared: Vec<(String, crate::detection::metadata::FileMetadata, Vec<u8>)> =
            Vec::with_capacity(items.len());

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
            let bytes = match serialize_payload(payload) {
                Ok(b) => b,
                Err(e) => {
                    report.failed.push((canonical.clone(), e));
                    continue;
                }
            };
            prepared.push((path_str, metadata, bytes));
        }

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
    /// Returns `true` if an entry was deleted.  Works even when the source
    /// file no longer exists on disk.
    pub fn remove<P>(&self, path: P) -> Result<bool, LocalFileCacheError>
    where
        P: AsRef<Path>,
    {
        self.guard_write()?;
        match normalize_path(path.as_ref()) {
            Ok(canonical) => {
                let path_str = path_to_str(&canonical)?;
                return repository::delete_by_path(&self.conn, &self.namespace, path_str);
            }
            Err(LocalFileCacheError::FileNotFound { .. }) => {}
            Err(e) => return Err(e),
        }

        // File is gone — search the DB by stored path string.
        let raw = path.as_ref().to_string_lossy();
        let stored_paths = repository::all_paths_in_namespace(&self.conn, &self.namespace)?;
        for stored in &stored_paths {
            if stored.as_str() == raw.as_ref()
                || (stored.ends_with(raw.as_ref())
                    && std::path::Path::new(stored)
                        .file_name()
                        .and_then(|n| n.to_str())
                        == std::path::Path::new(raw.as_ref())
                            .file_name()
                            .and_then(|n| n.to_str()))
            {
                return repository::delete_by_path(&self.conn, &self.namespace, stored);
            }
        }
        repository::delete_by_path(&self.conn, &self.namespace, raw.as_ref())
    }

    // ------------------------------------------------------------------
    // Status
    // ------------------------------------------------------------------

    /// Check the freshness of the cache entry for `path`.
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

    /// Delete entries in the current namespace whose source files are missing.
    pub fn cleanup_missing_files(&self) -> Result<usize, LocalFileCacheError> {
        self.guard_write()?;
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

    /// Delete entries in the current namespace that have exceeded the TTL.
    ///
    /// Returns `0` if no TTL is configured.
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

    /// Reclaim disk space via SQLite `VACUUM`.
    pub fn shrink_database(&self) -> Result<(), LocalFileCacheError> {
        self.guard_write()?;
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

/// Compute the appropriate hash (or `None`) for the given detection mode.
fn compute_hash_for_mode(
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

fn is_expired(updated_at: i64, ttl: Option<Duration>) -> bool {
    let Some(ttl) = ttl else {
        return false;
    };
    let now = repository::now_secs();
    now.saturating_sub(updated_at) as u64 >= ttl.as_secs()
}

impl<T> CacheEngine<T>
where
    T: Serialize + DeserializeOwned,
{
    /// Return `Err(ReadOnly)` if the engine is in read-only mode.
    #[inline]
    fn guard_write(&self) -> Result<(), LocalFileCacheError> {
        if self.read_only {
            Err(LocalFileCacheError::ReadOnly)
        } else {
            Ok(())
        }
    }
}
