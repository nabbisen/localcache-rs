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
use crate::serialization::{decode_payload, encode_payload};

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
/// `CacheEngine<T>` manages a SQLite-backed store that associates canonical
/// file paths with arbitrary serialisable payloads.
///
/// ## In-memory databases
///
/// Set `database_path: ":memory:".into()` in [`CacheOptions`] for an
/// ephemeral, in-process database — ideal for unit tests.
///
/// ## Read-only mode
///
/// Set `read_only: true` to open an existing database without write access.
///
/// ## Async support
///
/// Enable the `async` Cargo feature to access [`crate::AsyncCacheEngine`],
/// which wraps this engine in a `tokio::task::spawn_blocking` adapter.
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
    pub(crate) conn: Connection,
    pub(crate) mode: ChangeDetectionMode,
    pub(crate) namespace: String,
    pub(crate) ttl: Option<Duration>,
    pub(crate) read_only: bool,
    pub(crate) payload_version: u32,
    pub(crate) compress: bool,
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
            if !is_memory {
                conn.execute_batch(&format!(
                    "PRAGMA journal_mode = {}; PRAGMA synchronous = {};",
                    options.journal_mode.as_str(),
                    options.synchronous.as_str(),
                ))?;
            }
            schema::initialize(&conn)?;
        } else {
            schema::enable_foreign_keys(&conn)?;
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

        Ok(Self {
            conn,
            mode: options.change_detection_mode,
            namespace: options.namespace,
            ttl: options.ttl,
            read_only: options.read_only,
            payload_version: options.payload_version,
            compress,
            _phantom: PhantomData,
        })
    }

    // ------------------------------------------------------------------
    // Reads
    // ------------------------------------------------------------------

    /// Return the cached entry for `path`, if one exists.
    ///
    /// No change-detection or version check is performed.
    pub fn get<P>(&self, path: P) -> Result<Option<CacheEntry<T>>, LocalFileCacheError>
    where
        P: AsRef<Path>,
    {
        let canonical = normalize_path(path.as_ref())?;
        let path_str = path_to_str(&canonical)?;

        let Some(row) = repository::find_file(&self.conn, &self.namespace, path_str)? else {
            return Ok(None);
        };
        let Some(payload_row) = repository::load_payload(&self.conn, row.id)? else {
            return Ok(None);
        };
        let payload: T = decode_payload(&payload_row.content, &payload_row.encoding)?;
        Ok(Some(CacheEntry {
            path: PathBuf::from(&row.path),
            metadata: row.metadata,
            payload,
        }))
    }

    /// Return the cached entry for `path` only if it is still fresh.
    ///
    /// Returns `Ok(None)` when the file or entry is missing, the entry is
    /// stale, the TTL has elapsed, or the stored payload version does not
    /// match [`CacheOptions::payload_version`].
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
        if self.payload_version > 0 && row.payload_version != self.payload_version {
            return Ok(None);
        }
        match detect_change(&canonical, &row.metadata, self.mode)? {
            CacheStatus::Stale | CacheStatus::Missing => return Ok(None),
            CacheStatus::Fresh => {}
        }
        let Some(payload_row) = repository::load_payload(&self.conn, row.id)? else {
            return Ok(None);
        };
        let payload: T = decode_payload(&payload_row.content, &payload_row.encoding)?;
        Ok(Some(CacheEntry {
            path: PathBuf::from(&row.path),
            metadata: row.metadata,
            payload,
        }))
    }

    // ------------------------------------------------------------------
    // Batch reads
    // ------------------------------------------------------------------

    /// Retrieve multiple entries (no change-detection).
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
        let (bytes, encoding) = encode_payload(payload, self.compress)?;
        repository::upsert(
            &self.conn,
            &self.namespace,
            path_str,
            &metadata,
            &bytes,
            encoding,
            self.payload_version,
        )?;
        Ok(())
    }

    /// Store multiple `(path, payload)` pairs in a single transaction.
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
            let (bytes, encoding) = match encode_payload(payload, self.compress) {
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
        Ok(report)
    }

    // ------------------------------------------------------------------
    // Removal
    // ------------------------------------------------------------------

    /// Remove the cache entry for `path`.  Works even when the file no longer
    /// exists on disk.  Returns `true` if an entry was deleted.
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
        let raw = path.as_ref().to_string_lossy();
        let stored = repository::all_paths_in_namespace(&self.conn, &self.namespace)?;
        for s in &stored {
            if s.as_str() == raw.as_ref()
                || (s.ends_with(raw.as_ref())
                    && Path::new(s).file_name().and_then(|n| n.to_str())
                        == Path::new(raw.as_ref()).file_name().and_then(|n| n.to_str()))
            {
                return repository::delete_by_path(&self.conn, &self.namespace, s);
            }
        }
        repository::delete_by_path(&self.conn, &self.namespace, raw.as_ref())
    }

    // ------------------------------------------------------------------
    // Status
    // ------------------------------------------------------------------

    /// Check the freshness of the cache entry for `path`.
    ///
    /// Returns [`CacheStatus::Stale`] when the entry exists but the payload
    /// version does not match [`CacheOptions::payload_version`].
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
        if self.payload_version > 0 && row.payload_version != self.payload_version {
            return Ok(CacheStatus::Stale);
        }
        detect_change(&canonical, &row.metadata, self.mode)
    }

    // ------------------------------------------------------------------
    // Directory scan
    // ------------------------------------------------------------------

    /// Scan `dir` and return the [`CacheStatus`] of every regular file found.
    ///
    /// Each tuple is `(canonical_path, status)`.  Files not in the cache have
    /// status [`CacheStatus::Missing`].
    ///
    /// Set `recursive` to `true` to descend into subdirectories.
    pub fn scan_dir<P: AsRef<Path>>(
        &self,
        dir: P,
        recursive: bool,
    ) -> Result<Vec<(PathBuf, CacheStatus)>, LocalFileCacheError> {
        let dir = dir.as_ref();
        if !dir.is_dir() {
            return Err(LocalFileCacheError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("not a directory: {}", dir.display()),
            )));
        }
        let files = walk_dir(dir, recursive)?;
        let mut results = Vec::with_capacity(files.len());
        for file in files {
            let status = self.check_status(&file)?;
            results.push((file, status));
        }
        Ok(results)
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
            if !Path::new(p).exists() {
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
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

pub(crate) fn path_to_str(path: &Path) -> Result<&str, LocalFileCacheError> {
    path.to_str()
        .ok_or_else(|| LocalFileCacheError::InvalidPath {
            path: path.to_path_buf(),
        })
}

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

/// Recursively (or non-recursively) collect all regular files under `dir`.
fn walk_dir(dir: &Path, recursive: bool) -> Result<Vec<PathBuf>, LocalFileCacheError> {
    let mut files = Vec::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(d) = dirs.pop() {
        for entry in std::fs::read_dir(&d)? {
            let entry = entry?;
            let ft = entry.file_type()?;
            if ft.is_file() {
                files.push(entry.path());
            } else if recursive && ft.is_dir() {
                dirs.push(entry.path());
            }
        }
    }
    Ok(files)
}
