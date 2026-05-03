//! [`CacheEngine`] implementation.

use std::marker::PhantomData;
use std::path::{Path, PathBuf};

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
    /// is applied on every open so that missing tables are created
    /// automatically.
    pub fn open(options: CacheOptions) -> Result<Self, LocalFileCacheError> {
        let conn = Connection::open(&options.database_path)?;
        // Enable WAL for better concurrent read performance.
        conn.execute_batch("PRAGMA journal_mode = WAL;")?;
        schema::initialize(&conn)?;
        Ok(Self {
            conn,
            mode: options.change_detection_mode,
            _phantom: PhantomData,
        })
    }

    // ------------------------------------------------------------------
    // Reads
    // ------------------------------------------------------------------

    /// Return the cached entry for `path`, if one exists.
    ///
    /// This is a pure database read; it does **not** check whether the
    /// on-disk file has changed.  Use [`get_if_fresh`](Self::get_if_fresh) if
    /// you need change detection.
    pub fn get<P>(&self, path: P) -> Result<Option<CacheEntry<T>>, LocalFileCacheError>
    where
        P: AsRef<Path>,
    {
        let canonical = normalize_path(path.as_ref())?;
        let path_str = path_to_str(&canonical)?;

        let Some(row) = repository::find_file(&self.conn, path_str)? else {
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
    /// * no cache entry exists in the database,
    /// * or the change-detection check reports the entry as [`CacheStatus::Stale`].
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

        let Some(row) = repository::find_file(&self.conn, path_str)? else {
            return Ok(None);
        };

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
    // Writes
    // ------------------------------------------------------------------

    /// Store `payload` for `path`.
    ///
    /// Current file metadata (mtime, size, and optionally a hash) is captured
    /// automatically.  If an entry already exists it is replaced atomically
    /// within a single transaction.
    pub fn set<P>(&self, path: P, payload: &T) -> Result<(), LocalFileCacheError>
    where
        P: AsRef<Path>,
    {
        let canonical = normalize_path(path.as_ref())?;
        let path_str = path_to_str(&canonical)?;

        let mut metadata = collect_metadata(&canonical)?;

        // Compute a hash when the detection mode demands one.
        if needs_hash(self.mode) {
            metadata.hash = Some(compute_full_hash(&canonical)?);
        }

        let bytes = serialize_payload(payload)?;
        repository::upsert(&self.conn, path_str, &metadata, &bytes)?;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Removal
    // ------------------------------------------------------------------

    /// Remove the cache entry for `path`.
    ///
    /// Returns `true` if an entry existed and was deleted, `false` if no entry
    /// was found.  The associated payload row is removed automatically via the
    /// `ON DELETE CASCADE` foreign-key constraint.
    ///
    /// If `path` no longer exists on disk this method still attempts to delete
    /// the entry using the path string as provided (after light normalisation).
    pub fn remove<P>(&self, path: P) -> Result<bool, LocalFileCacheError>
    where
        P: AsRef<Path>,
    {
        // Try canonical form first; fall back to the raw path string so that
        // entries can be removed even after the file has been deleted.
        let path_str: String = match normalize_path(path.as_ref()) {
            Ok(canonical) => path_to_str(&canonical)?.to_owned(),
            Err(LocalFileCacheError::FileNotFound { .. }) => {
                path.as_ref().to_string_lossy().into_owned()
            }
            Err(e) => return Err(e),
        };
        repository::delete_by_path(&self.conn, &path_str)
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
        // If the file itself is gone, report Missing immediately.
        let canonical = match normalize_path(path.as_ref()) {
            Ok(p) => p,
            Err(LocalFileCacheError::FileNotFound { .. }) => return Ok(CacheStatus::Missing),
            Err(e) => return Err(e),
        };
        let path_str = path_to_str(&canonical)?;

        let Some(row) = repository::find_file(&self.conn, path_str)? else {
            return Ok(CacheStatus::Missing);
        };

        detect_change(&canonical, &row.metadata, self.mode)
    }

    // ------------------------------------------------------------------
    // Maintenance
    // ------------------------------------------------------------------

    /// Delete cache entries whose source files no longer exist on disk.
    ///
    /// Returns the number of entries that were removed.  Payload rows are
    /// removed automatically via `ON DELETE CASCADE`.
    pub fn cleanup_missing_files(&self) -> Result<usize, LocalFileCacheError> {
        let paths = repository::all_paths(&self.conn)?;
        let mut removed = 0;
        for p in &paths {
            if !std::path::Path::new(p).exists() {
                repository::delete_path(&self.conn, p)?;
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

/// Convert a [`PathBuf`] to a `&str`, returning an error for non-UTF-8 paths.
fn path_to_str(path: &Path) -> Result<&str, LocalFileCacheError> {
    path.to_str()
        .ok_or_else(|| LocalFileCacheError::InvalidPath {
            path: path.to_path_buf(),
        })
}

/// Return `true` if the detection mode requires a hash to be stored.
fn needs_hash(mode: ChangeDetectionMode) -> bool {
    matches!(
        mode,
        ChangeDetectionMode::MetadataThenPartialHash
            | ChangeDetectionMode::MetadataThenFullHash
            | ChangeDetectionMode::StrictFullHash
    )
}
