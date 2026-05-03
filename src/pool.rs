//! Thread-safe connection pool for [`CacheEngine`].
//!
//! [`ConnectionPool`] wraps a `CacheEngine<T>` behind an `Arc<Mutex<…>>` and
//! provides the same API surface — making it ergonomic to share a single
//! cache engine across multiple threads without callers having to manage the
//! mutex themselves.
//!
//! ## Relationship to `AsyncCacheEngine`
//!
//! [`AsyncCacheEngine`][crate::AsyncCacheEngine] already uses the same
//! `Arc<Mutex<CacheEngine<T>>>` pattern and is the recommended approach for
//! `async` code.  `ConnectionPool` targets synchronous multi-threaded
//! applications (e.g. Actix-web handlers, Rayon workers) where an async
//! runtime may not be available or desirable.
//!
//! ## Example
//!
//! ```no_run
//! use std::thread;
//! use localcache::{CacheOptions, ConnectionPool};
//!
//! let pool = ConnectionPool::<Vec<f32>>::open(CacheOptions {
//!     database_path: "shared.sqlite3".into(),
//!     ..CacheOptions::default()
//! })?;
//!
//! let pool2 = pool.clone();
//! thread::spawn(move || {
//!     // pool2 shares the underlying engine with pool.
//! });
//! # Ok::<(), localcache::LocalFileCacheError>(())
//! ```

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde::{Serialize, de::DeserializeOwned};

use crate::cache::engine::{BatchSetReport, CacheEngine};
use crate::cache::entry::{CacheEntry, CacheStats, EntryInfo, ExportRecord};
use crate::cache::options::CacheOptions;
use crate::cache::query::QueryBuilder;
use crate::error::LocalFileCacheError;

use crate::{CacheStatus, ScanOptions};

// ---------------------------------------------------------------------------
// ConnectionPool
// ---------------------------------------------------------------------------

/// A cloneable, thread-safe wrapper around [`CacheEngine<T>`].
///
/// All clones share the same underlying engine and SQLite connection.
/// Operations acquire the internal mutex for their duration.
#[derive(Clone)]
pub struct ConnectionPool<T> {
    inner: Arc<Mutex<CacheEngine<T>>>,
}

impl<T> ConnectionPool<T>
where
    T: Serialize + DeserializeOwned,
{
    // ------------------------------------------------------------------
    // Construction
    // ------------------------------------------------------------------

    /// Open (or create) a connection pool backed by a single [`CacheEngine`].
    pub fn open(options: CacheOptions) -> Result<Self, LocalFileCacheError> {
        CacheEngine::open(options).map(|e| Self {
            inner: Arc::new(Mutex::new(e)),
        })
    }

    /// Acquire the mutex and call `f` with a reference to the inner engine.
    ///
    /// This is the escape hatch for operations not yet exposed on the pool.
    pub fn with<R, F>(&self, f: F) -> Result<R, LocalFileCacheError>
    where
        F: FnOnce(&CacheEngine<T>) -> Result<R, LocalFileCacheError>,
    {
        let guard = self.lock()?;
        f(&*guard)
    }

    /// Acquire the mutex and call `f` with a mutable reference to the inner
    /// engine.
    pub fn with_mut<R, F>(&self, f: F) -> Result<R, LocalFileCacheError>
    where
        F: FnOnce(&mut CacheEngine<T>) -> Result<R, LocalFileCacheError>,
    {
        let mut guard = self.lock()?;
        f(&mut *guard)
    }

    // ------------------------------------------------------------------
    // Reads
    // ------------------------------------------------------------------

    /// Pooled version of [`CacheEngine::get`].
    pub fn get<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<Option<CacheEntry<T>>, LocalFileCacheError> {
        self.lock()?.get(path)
    }

    /// Pooled version of [`CacheEngine::get_if_fresh`].
    pub fn get_if_fresh<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<Option<CacheEntry<T>>, LocalFileCacheError> {
        self.lock()?.get_if_fresh(path)
    }

    /// Pooled version of [`CacheEngine::batch_get`].
    pub fn batch_get<P: AsRef<Path>>(
        &self,
        paths: &[P],
    ) -> Vec<Result<Option<CacheEntry<T>>, LocalFileCacheError>> {
        match self.lock() {
            Ok(g) => g.batch_get(paths),
            Err(e) => vec![Err(e)],
        }
    }

    /// Pooled version of [`CacheEngine::batch_get_fresh`].
    pub fn batch_get_fresh<P: AsRef<Path>>(
        &self,
        paths: &[P],
    ) -> Vec<Result<Option<CacheEntry<T>>, LocalFileCacheError>> {
        match self.lock() {
            Ok(g) => g.batch_get_fresh(paths),
            Err(e) => vec![Err(e)],
        }
    }

    // ------------------------------------------------------------------
    // Writes
    // ------------------------------------------------------------------

    /// Pooled version of [`CacheEngine::set`].
    pub fn set<P: AsRef<Path>>(&self, path: P, payload: &T) -> Result<(), LocalFileCacheError> {
        self.lock()?.set(path, payload)
    }

    /// Pooled version of [`CacheEngine::batch_set`].
    pub fn batch_set<P: AsRef<Path>>(
        &self,
        items: &[(P, T)],
    ) -> Result<BatchSetReport, LocalFileCacheError> {
        self.lock()?.batch_set(items)
    }

    // ------------------------------------------------------------------
    // Removal
    // ------------------------------------------------------------------

    /// Pooled version of [`CacheEngine::remove`].
    pub fn remove<P: AsRef<Path>>(&self, path: P) -> Result<bool, LocalFileCacheError> {
        self.lock()?.remove(path)
    }

    // ------------------------------------------------------------------
    // Status
    // ------------------------------------------------------------------

    /// Pooled version of [`CacheEngine::check_status`].
    pub fn check_status<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<CacheStatus, LocalFileCacheError> {
        self.lock()?.check_status(path)
    }

    /// Pooled version of [`CacheEngine::check_status_batch`].
    pub fn check_status_batch<P: AsRef<Path>>(
        &self,
        paths: &[P],
    ) -> Vec<Result<CacheStatus, LocalFileCacheError>> {
        match self.lock() {
            Ok(g) => g.check_status_batch(paths),
            Err(e) => vec![Err(e)],
        }
    }

    /// Pooled version of [`CacheEngine::contains`].
    pub fn contains<P: AsRef<Path>>(&self, path: P) -> Result<bool, LocalFileCacheError> {
        self.lock()?.contains(path)
    }

    // ------------------------------------------------------------------
    // Directory scan
    // ------------------------------------------------------------------

    /// Pooled version of [`CacheEngine::scan_dir`].
    pub fn scan_dir<P: AsRef<Path>>(
        &self,
        dir: P,
        recursive: bool,
    ) -> Result<Vec<(PathBuf, CacheStatus)>, LocalFileCacheError> {
        self.lock()?.scan_dir(dir, recursive)
    }

    /// Pooled version of [`CacheEngine::scan_dir_filtered`].
    pub fn scan_dir_filtered<P: AsRef<Path>>(
        &self,
        dir: P,
        options: ScanOptions,
    ) -> Result<Vec<(PathBuf, CacheStatus)>, LocalFileCacheError> {
        self.lock()?.scan_dir_filtered(dir, options)
    }

    // ------------------------------------------------------------------
    // Keys
    // ------------------------------------------------------------------

    /// Pooled version of [`CacheEngine::keys`].
    pub fn keys(&self, path_like: Option<&str>) -> Result<Vec<PathBuf>, LocalFileCacheError> {
        self.lock()?.keys(path_like)
    }

    // ------------------------------------------------------------------
    // Touch / TTL
    // ------------------------------------------------------------------

    /// Pooled version of [`CacheEngine::touch`].
    pub fn touch<P: AsRef<Path>>(&self, path: P) -> Result<bool, LocalFileCacheError> {
        self.lock()?.touch(path)
    }

    // ------------------------------------------------------------------
    // Observability
    // ------------------------------------------------------------------

    /// Pooled version of [`CacheEngine::list_entries`].
    pub fn list_entries(&self) -> Result<Vec<EntryInfo>, LocalFileCacheError> {
        self.lock()?.list_entries()
    }

    /// Pooled version of [`CacheEngine::entry_count`].
    pub fn entry_count(&self) -> Result<usize, LocalFileCacheError> {
        self.lock()?.entry_count()
    }

    /// Pooled version of [`CacheEngine::entry_count_by_version`].
    pub fn entry_count_by_version(&self) -> Result<Vec<(u32, usize)>, LocalFileCacheError> {
        self.lock()?.entry_count_by_version()
    }

    /// Pooled version of [`CacheEngine::cache_stats`].
    pub fn cache_stats(&self) -> Result<CacheStats, LocalFileCacheError> {
        self.lock()?.cache_stats()
    }

    // ------------------------------------------------------------------
    // Export / import
    // ------------------------------------------------------------------

    /// Pooled version of [`CacheEngine::export_entries`].
    pub fn export_entries(&self) -> Result<Vec<ExportRecord>, LocalFileCacheError> {
        self.lock()?.export_entries()
    }

    /// Pooled version of [`CacheEngine::import_entries`].
    pub fn import_entries(&self, records: &[ExportRecord]) -> Result<usize, LocalFileCacheError> {
        self.lock()?.import_entries(records)
    }

    // ------------------------------------------------------------------
    // Query
    // ------------------------------------------------------------------

    /// Execute a query built from a closure.
    ///
    /// The closure receives a `QueryBuilder<'_, T>` and must return one.
    /// The pool holds the mutex for the duration of the build **and** the
    /// `run()` call.
    pub fn query_run<F>(&self, build: F) -> Result<Vec<CacheEntry<T>>, LocalFileCacheError>
    where
        F: FnOnce(QueryBuilder<'_, T>) -> QueryBuilder<'_, T>,
    {
        let guard = self.lock()?;
        let q = guard.query();
        let q = build(q);
        crate::cache::query::execute_query(q)
    }

    // ------------------------------------------------------------------
    // Maintenance
    // ------------------------------------------------------------------

    /// Pooled version of [`CacheEngine::cleanup_missing_files`].
    pub fn cleanup_missing_files(&self) -> Result<usize, LocalFileCacheError> {
        self.lock()?.cleanup_missing_files()
    }

    /// Pooled version of [`CacheEngine::cleanup_expired`].
    pub fn cleanup_expired(&self) -> Result<usize, LocalFileCacheError> {
        self.lock()?.cleanup_expired()
    }

    /// Pooled version of [`CacheEngine::purge_stale_versions`].
    pub fn purge_stale_versions(&self) -> Result<usize, LocalFileCacheError> {
        self.lock()?.purge_stale_versions()
    }

    /// Pooled version of [`CacheEngine::shrink_database`].
    pub fn shrink_database(&self) -> Result<(), LocalFileCacheError> {
        self.lock()?.shrink_database()
    }

    // ------------------------------------------------------------------
    // Private helpers
    // ------------------------------------------------------------------

    fn lock(&self) -> Result<MutexGuard<'_, CacheEngine<T>>, LocalFileCacheError> {
        self.inner.lock().map_err(|_| {
            LocalFileCacheError::UnsupportedFeature("ConnectionPool mutex was poisoned".into())
        })
    }
}

/// Convenience alias: a [`ConnectionPool`] is just `Arc<Mutex<CacheEngine<T>>>`.
pub type SharedEngine<T> = Arc<Mutex<CacheEngine<T>>>;

/// Create a [`SharedEngine`] from a [`CacheOptions`].
pub fn shared_engine<T>(options: CacheOptions) -> Result<SharedEngine<T>, LocalFileCacheError>
where
    T: Serialize + DeserializeOwned,
{
    CacheEngine::open(options).map(|e| Arc::new(Mutex::new(e)))
}

// ---------------------------------------------------------------------------
// Duration-based TTL constructor helper on CacheOptions
// ---------------------------------------------------------------------------

/// Extension trait for ergonomic [`CacheOptions`] construction.
pub trait CacheOptionsExt: Sized {
    /// Set TTL from seconds.
    fn with_ttl_secs(self, secs: u64) -> Self;
    /// Set TTL from minutes.
    fn with_ttl_mins(self, mins: u64) -> Self;
    /// Set TTL from hours.
    fn with_ttl_hours(self, hours: u64) -> Self;
}

impl CacheOptionsExt for CacheOptions {
    fn with_ttl_secs(mut self, secs: u64) -> Self {
        self.ttl = Some(Duration::from_secs(secs));
        self
    }
    fn with_ttl_mins(mut self, mins: u64) -> Self {
        self.ttl = Some(Duration::from_secs(mins * 60));
        self
    }
    fn with_ttl_hours(mut self, hours: u64) -> Self {
        self.ttl = Some(Duration::from_secs(hours * 3600));
        self
    }
}
