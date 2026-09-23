//! [`SyncCacheEngine`]: one [`CacheEngine`] shared across threads.
//!
//! [`SyncCacheEngine`] wraps a `CacheEngine<T>` behind an `Arc<Mutex<…>>` and
//! delegates every public `CacheEngine` method under the same name, so a
//! single cache engine can be shared across threads without callers managing
//! the mutex themselves. The two exceptions are `builder` (use
//! [`SyncCacheEngine::open`]) and `query`, whose builder borrows the engine
//! (use [`SyncCacheEngine::query_run`] or [`SyncCacheEngine::query_dry_run`]). Every call
//! takes the lock for its duration, so calls from different threads run one
//! at a time; it is one engine, not a pool of connections. For many
//! concurrent readers, use [`ReadPool`][crate::ReadPool], which is a pool of
//! read-only connections.
//!
//! ## Relationship to `AsyncCacheEngine`
//!
//! It is the synchronous counterpart of
//! [`AsyncCacheEngine`][crate::AsyncCacheEngine], which uses the same
//! `Arc<Mutex<CacheEngine<T>>>` design and is the recommended choice for
//! `async` code. `SyncCacheEngine` targets synchronous multi-threaded
//! applications (e.g. Actix-web handlers, Rayon workers) where an async
//! runtime may not be available or desirable.
//!
//! ## Example
//!
//! ```no_run
//! use std::thread;
//! use localcache::{CacheOptions, SyncCacheEngine};
//!
//! let engine = SyncCacheEngine::<Vec<f32>>::open(CacheOptions {
//!     database_path: "shared.sqlite3".into(),
//!     ..CacheOptions::default()
//! })?;
//!
//! let engine2 = engine.clone();
//! thread::spawn(move || {
//!     // engine2 shares the underlying engine with engine.
//! });
//! # Ok::<(), localcache::LocalFileCacheError>(())
//! ```

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde::{Serialize, de::DeserializeOwned};

use crate::cache::engine::{BatchSetReport, CacheEngine};
use crate::cache::entry::{CacheEntry, CacheStats, EntryInfo, ExportRecord, PreloadReport};
use crate::cache::options::CacheOptions;
use crate::cache::query::{QueryBuilder, QueryReport};
use crate::error::LocalFileCacheError;

use crate::{CacheStatus, ScanOptions};

// ---------------------------------------------------------------------------
// SyncCacheEngine
// ---------------------------------------------------------------------------

/// A cloneable, thread-safe wrapper around one [`CacheEngine<T>`]: the
/// synchronous counterpart of [`AsyncCacheEngine`][crate::AsyncCacheEngine].
///
/// All clones share the same underlying engine and SQLite connection.
/// Operations acquire the internal mutex for their duration, so this is a
/// shared engine, not a pool. Use [`ReadPool`][crate::ReadPool] for a pool of
/// read-only connections.
#[derive(Clone)]
pub struct SyncCacheEngine<T> {
    inner: Arc<Mutex<CacheEngine<T>>>,
}

/// The former name of [`SyncCacheEngine`].
#[deprecated(
    since = "0.21.5",
    note = "renamed to SyncCacheEngine; it is a shared engine, not a pool"
)]
pub type ConnectionPool<T> = SyncCacheEngine<T>;

impl<T> SyncCacheEngine<T>
where
    T: Serialize + DeserializeOwned,
{
    // ------------------------------------------------------------------
    // Construction
    // ------------------------------------------------------------------

    /// Open (or create) a [`CacheEngine`] and wrap it for sharing across threads.
    pub fn open(options: CacheOptions) -> Result<Self, LocalFileCacheError> {
        CacheEngine::open(options).map(|e| Self {
            inner: Arc::new(Mutex::new(e)),
        })
    }

    /// Acquire the mutex and call `f` with a reference to the inner engine.
    ///
    /// This is the escape hatch for what the wrapper does not expose: a
    /// `CacheEngine` method used with borrowed state, such as `query`, or an
    /// argument that is itself a `&CacheEngine`. **Do not** call another
    /// method of this same `SyncCacheEngine` (or a clone) from inside `f`: the
    /// mutex is not reentrant, so that deadlocks.
    ///
    /// A read-only engine retains its normal mutation guards inside `f`.
    pub fn with<R, F>(&self, f: F) -> Result<R, LocalFileCacheError>
    where
        F: FnOnce(&CacheEngine<T>) -> Result<R, LocalFileCacheError>,
    {
        let guard = self.lock()?;
        f(&*guard)
    }

    /// Acquire the mutex and call `f` with a mutable reference to the inner
    /// engine.
    ///
    /// A read-only engine retains its normal mutation guards inside `f`.
    /// Because the closure receives `&mut CacheEngine<T>`, deliberately
    /// replacing the complete engine can change authority and is the caller's
    /// explicit responsibility rather than a wrapper operation.
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

    /// Locks the shared engine and calls [`CacheEngine::get`].
    pub fn get<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<Option<CacheEntry<T>>, LocalFileCacheError> {
        self.lock()?.get(path)
    }

    /// Locks the shared engine and calls [`CacheEngine::get_if_fresh`].
    pub fn get_if_fresh<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<Option<CacheEntry<T>>, LocalFileCacheError> {
        self.lock()?.get_if_fresh(path)
    }

    /// Locks the shared engine and calls [`CacheEngine::batch_get`].
    ///
    /// If the engine's lock is poisoned, every element is
    /// `Err(LocalFileCacheError::Poisoned { resource: "ConnectionPool" })`.
    pub fn batch_get<P: AsRef<Path>>(
        &self,
        paths: &[P],
    ) -> Vec<Result<Option<CacheEntry<T>>, LocalFileCacheError>> {
        match self.lock() {
            Ok(g) => g.batch_get(paths),
            Err(_) => paths
                .iter()
                .map(|_| {
                    Err(LocalFileCacheError::Poisoned {
                        // Still "ConnectionPool": the string is observable, and
                        // v0.21.5 changes no behaviour. It changes in v0.22.0.
                        resource: "ConnectionPool",
                    })
                })
                .collect(),
        }
    }

    /// Locks the shared engine and calls [`CacheEngine::batch_get_fresh`].
    ///
    /// If the engine's lock is poisoned, every element is
    /// `Err(LocalFileCacheError::Poisoned { resource: "ConnectionPool" })`.
    pub fn batch_get_fresh<P: AsRef<Path>>(
        &self,
        paths: &[P],
    ) -> Vec<Result<Option<CacheEntry<T>>, LocalFileCacheError>> {
        match self.lock() {
            Ok(g) => g.batch_get_fresh(paths),
            Err(_) => paths
                .iter()
                .map(|_| {
                    Err(LocalFileCacheError::Poisoned {
                        // Still "ConnectionPool": the string is observable, and
                        // v0.21.5 changes no behaviour. It changes in v0.22.0.
                        resource: "ConnectionPool",
                    })
                })
                .collect(),
        }
    }

    // ------------------------------------------------------------------
    // Writes
    // ------------------------------------------------------------------

    /// Locks the shared engine and calls [`CacheEngine::set`].
    pub fn set<P: AsRef<Path>>(&self, path: P, payload: &T) -> Result<(), LocalFileCacheError> {
        self.lock()?.set(path, payload)
    }

    /// Locks the shared engine and calls [`CacheEngine::batch_set`].
    pub fn batch_set<P: AsRef<Path>>(
        &self,
        items: &[(P, T)],
    ) -> Result<BatchSetReport, LocalFileCacheError> {
        self.lock()?.batch_set(items)
    }

    // ------------------------------------------------------------------
    // Removal
    // ------------------------------------------------------------------

    /// Locks the shared engine and calls [`CacheEngine::remove`].
    pub fn remove<P: AsRef<Path>>(&self, path: P) -> Result<bool, LocalFileCacheError> {
        self.lock()?.remove(path)
    }

    // ------------------------------------------------------------------
    // Status
    // ------------------------------------------------------------------

    /// Locks the shared engine and calls [`CacheEngine::check_status`].
    pub fn check_status<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<CacheStatus, LocalFileCacheError> {
        self.lock()?.check_status(path)
    }

    /// Locks the shared engine and calls [`CacheEngine::check_status_batch`].
    ///
    /// If the engine's lock is poisoned, every element is
    /// `Err(LocalFileCacheError::Poisoned { resource: "ConnectionPool" })`.
    pub fn check_status_batch<P: AsRef<Path>>(
        &self,
        paths: &[P],
    ) -> Vec<Result<CacheStatus, LocalFileCacheError>> {
        match self.lock() {
            Ok(g) => g.check_status_batch(paths),
            Err(_) => paths
                .iter()
                .map(|_| {
                    Err(LocalFileCacheError::Poisoned {
                        // Still "ConnectionPool": the string is observable, and
                        // v0.21.5 changes no behaviour. It changes in v0.22.0.
                        resource: "ConnectionPool",
                    })
                })
                .collect(),
        }
    }

    /// Locks the shared engine and calls [`CacheEngine::contains`].
    pub fn contains<P: AsRef<Path>>(&self, path: P) -> Result<bool, LocalFileCacheError> {
        self.lock()?.contains(path)
    }

    /// Locks the shared engine and calls [`CacheEngine::explain`].
    pub fn explain<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<crate::cache::entry::Diagnosis, LocalFileCacheError> {
        self.lock()?.explain(path)
    }

    // ------------------------------------------------------------------
    // Directory scan
    // ------------------------------------------------------------------

    /// Locks the shared engine and calls [`CacheEngine::scan_dir`].
    pub fn scan_dir<P: AsRef<Path>>(
        &self,
        dir: P,
        recursive: bool,
    ) -> Result<Vec<(PathBuf, CacheStatus)>, LocalFileCacheError> {
        self.lock()?.scan_dir(dir, recursive)
    }

    /// Locks the shared engine and calls [`CacheEngine::scan_dir_filtered`].
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

    /// Locks the shared engine and calls [`CacheEngine::keys`].
    pub fn keys(&self, path_like: Option<&str>) -> Result<Vec<PathBuf>, LocalFileCacheError> {
        self.lock()?.keys(path_like)
    }

    // ------------------------------------------------------------------
    // Touch / TTL
    // ------------------------------------------------------------------

    /// Locks the shared engine and calls [`CacheEngine::touch`].
    pub fn touch<P: AsRef<Path>>(&self, path: P) -> Result<bool, LocalFileCacheError> {
        self.lock()?.touch(path)
    }

    // ------------------------------------------------------------------
    // Observability
    // ------------------------------------------------------------------

    /// Locks the shared engine and calls [`CacheEngine::list_entries`].
    pub fn list_entries(&self) -> Result<Vec<EntryInfo>, LocalFileCacheError> {
        self.lock()?.list_entries()
    }

    /// Locks the shared engine and calls [`CacheEngine::entry_count`].
    pub fn entry_count(&self) -> Result<usize, LocalFileCacheError> {
        self.lock()?.entry_count()
    }

    /// Locks the shared engine and calls [`CacheEngine::entry_count_by_version`].
    pub fn entry_count_by_version(&self) -> Result<Vec<(u32, usize)>, LocalFileCacheError> {
        self.lock()?.entry_count_by_version()
    }

    /// Locks the shared engine and calls [`CacheEngine::cache_stats`].
    pub fn cache_stats(&self) -> Result<CacheStats, LocalFileCacheError> {
        self.lock()?.cache_stats()
    }

    // ------------------------------------------------------------------
    // Export / import
    // ------------------------------------------------------------------

    /// Locks the shared engine and calls [`CacheEngine::export_entries`].
    pub fn export_entries(&self) -> Result<Vec<ExportRecord>, LocalFileCacheError> {
        self.lock()?.export_entries()
    }

    /// Locks the shared engine and calls [`CacheEngine::import_entries`].
    pub fn import_entries(&self, records: &[ExportRecord]) -> Result<usize, LocalFileCacheError> {
        self.lock()?.import_entries(records)
    }

    /// Locks the shared engine and calls [`CacheEngine::import_from`].
    ///
    /// `source` is a plain [`CacheEngine`], separate from this one. **Do not**
    /// obtain it from this same `SyncCacheEngine` (for example through
    /// [`with`](Self::with)): the mutex is not reentrant, so that deadlocks.
    pub fn import_from<U>(&self, source: &CacheEngine<U>) -> Result<usize, LocalFileCacheError>
    where
        U: Serialize + DeserializeOwned,
    {
        self.lock()?.import_from(source)
    }

    /// Locks the shared engine and calls [`CacheEngine::namespace_list`].
    pub fn namespace_list(&self) -> Result<Vec<String>, LocalFileCacheError> {
        self.lock()?.namespace_list()
    }

    /// Locks the shared engine and calls [`CacheEngine::rotate_encryption_key`].
    ///
    /// The mutex is held for the whole rotation, so other threads wait. Every
    /// clone of this `SyncCacheEngine` shares one inner engine and continues
    /// with `new_key` together. Rotation covers **only this engine's
    /// namespace**; another open engine on the same database and namespace
    /// keeps its old key and must be reopened with `new_key`.
    ///
    /// Requires the `encryption` Cargo feature.
    #[cfg(feature = "encryption")]
    pub fn rotate_encryption_key(&self, new_key: &[u8]) -> Result<usize, LocalFileCacheError> {
        self.lock()?.rotate_encryption_key(new_key)
    }

    // ------------------------------------------------------------------
    // Query
    // ------------------------------------------------------------------

    /// Execute a query built from a closure.
    ///
    /// The closure receives a `QueryBuilder<'_, T>` and must return one.
    /// The wrapper holds the mutex for the duration of the build **and** the
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

    /// Execute a query built from a closure, and report the entries it could
    /// not decode. See [`QueryBuilder::run_report`].
    ///
    /// Holds the mutex for the build and the run, like
    /// [`query_run`](Self::query_run).
    pub fn query_run_report<F>(&self, build: F) -> Result<QueryReport<T>, LocalFileCacheError>
    where
        F: FnOnce(QueryBuilder<'_, T>) -> QueryBuilder<'_, T>,
    {
        let guard = self.lock()?;
        let q = guard.query();
        let q = build(q);
        crate::cache::query::execute_report(q)
    }

    /// Return the `EXPLAIN QUERY PLAN` output for a query built from a
    /// closure, without loading payloads.
    ///
    /// Holds the mutex while the closure runs, like
    /// [`query_run`](Self::query_run).
    pub fn query_dry_run<F>(&self, build: F) -> Result<String, LocalFileCacheError>
    where
        F: FnOnce(QueryBuilder<'_, T>) -> QueryBuilder<'_, T>,
    {
        let guard = self.lock()?;
        let q = guard.query();
        build(q).dry_run()
    }

    // ------------------------------------------------------------------
    // Preload
    // ------------------------------------------------------------------

    /// Locks the shared engine and calls [`CacheEngine::preload`].
    ///
    /// **The mutex is held for the whole preload, including every call to
    /// `factory`.** Other threads using this engine wait, and `factory` must
    /// not call back into this `SyncCacheEngine` (or a clone): the mutex is
    /// not reentrant, so that deadlocks.
    pub fn preload<P, F>(
        &self,
        dir: P,
        options: ScanOptions,
        force: bool,
        factory: F,
    ) -> Result<PreloadReport, LocalFileCacheError>
    where
        P: AsRef<Path>,
        F: Fn(&Path) -> Result<T, Box<dyn std::error::Error + Send + Sync>>,
    {
        self.lock()?.preload(dir, options, force, factory)
    }

    // ------------------------------------------------------------------
    // Watching (watching feature)
    // ------------------------------------------------------------------

    /// Locks the shared engine and calls [`CacheEngine::watcher`].
    ///
    /// The lock is released once the watcher is built. The watcher owns its
    /// own helper connection, so it does not hold this engine's mutex.
    ///
    /// Requires the `watching` Cargo feature.
    #[cfg(feature = "watching")]
    pub fn watcher(&self) -> Result<crate::cache::watcher::CacheWatcher<T>, LocalFileCacheError>
    where
        T: Send + 'static,
    {
        self.lock()?.watcher()
    }

    /// Locks the shared engine and calls [`CacheEngine::debounced_watcher`].
    ///
    /// The lock is released once the watcher is built.
    ///
    /// Requires the `watching` Cargo feature.
    #[cfg(feature = "watching")]
    pub fn debounced_watcher(
        &self,
        window: Duration,
    ) -> Result<crate::cache::watcher::CacheDebouncedWatcher<T>, LocalFileCacheError>
    where
        T: Send + 'static,
    {
        self.lock()?.debounced_watcher(window)
    }

    // ------------------------------------------------------------------
    // Maintenance
    // ------------------------------------------------------------------

    /// Locks the shared engine and calls [`CacheEngine::cleanup_missing_files`].
    pub fn cleanup_missing_files(&self) -> Result<usize, LocalFileCacheError> {
        self.lock()?.cleanup_missing_files()
    }

    /// Locks the shared engine and calls [`CacheEngine::cleanup_expired`].
    pub fn cleanup_expired(&self) -> Result<usize, LocalFileCacheError> {
        self.lock()?.cleanup_expired()
    }

    /// Locks the shared engine and calls [`CacheEngine::purge_stale_versions`].
    pub fn purge_stale_versions(&self) -> Result<usize, LocalFileCacheError> {
        self.lock()?.purge_stale_versions()
    }

    /// Locks the shared engine and calls [`CacheEngine::shrink_database`].
    pub fn shrink_database(&self) -> Result<(), LocalFileCacheError> {
        self.lock()?.shrink_database()
    }

    // ------------------------------------------------------------------
    // Private helpers
    // ------------------------------------------------------------------

    fn lock(&self) -> Result<MutexGuard<'_, CacheEngine<T>>, LocalFileCacheError> {
        self.inner
            .lock()
            .map_err(|_| LocalFileCacheError::Poisoned {
                // Still "ConnectionPool": the string is observable, and
                // v0.21.5 changes no behaviour. It changes in v0.22.0.
                resource: "ConnectionPool",
            })
    }
}

/// A bare `Arc<Mutex<CacheEngine<T>>>`: a second way to share an engine that
/// offers none of [`SyncCacheEngine`]'s methods.
#[deprecated(
    since = "0.21.5",
    note = "use SyncCacheEngine, or Arc::new(Mutex::new(CacheEngine::open(..)?)) for the bare form"
)]
pub type SharedEngine<T> = Arc<Mutex<CacheEngine<T>>>;

/// Create a [`SharedEngine`] from a [`CacheOptions`].
#[deprecated(
    since = "0.21.5",
    note = "use SyncCacheEngine::open, or Arc::new(Mutex::new(CacheEngine::open(..)?)) for the bare form"
)]
#[allow(deprecated)]
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
