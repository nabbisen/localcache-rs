//! Read-only connection pool for concurrent cache lookups.
//!
//! [`ReadPool`] holds N independent (or shared-cache) read-only
//! [`CacheEngine`] connections over one database file.  Each slot can be
//! checked out concurrently by different threads; SQLite WAL mode allows
//! unlimited simultaneous readers, so point lookups fan out without
//! serialization.
//!
//! Write methods are **absent from this type** — read-onlyness is a
//! compile-time property, not a runtime guard.
//!
//! # When to use ReadPool vs ConnectionPool
//!
//! | Scenario | Recommended |
//! |---|---|
//! | Single writer + single thread | [`CacheEngine<T>`][crate::CacheEngine] |
//! | Mixed read/write across threads | [`ConnectionPool<T>`][crate::ConnectionPool] |
//! | Read-heavy fan-out with a separate writer | **`ReadPool<T>`** |
//! | Async | [`AsyncCacheEngine<T>`][crate::AsyncCacheEngine] |
//!
//! # Connection backends
//!
//! - **Independent** (default, `shared_cache = false`): each slot is a plain
//!   `read_only` connection with its own SQLite page cache — maximum
//!   read parallelism, higher per-slot memory.
//! - **Shared-cache** (`shared_cache = true` on the `CacheOptions`): slots
//!   share one page cache (RFC 0004 mode) — lower memory on large pools.
//!
//! # WAL requirement
//!
//! `ReadPool` requires the database to have been opened at least once by a
//! read-write engine so that the WAL sidecar files exist.  In normal
//! producer-consumer deployments (writer opens first, pool opens after) this
//! holds automatically.
//!
//! `:memory:` databases are rejected at construction — N independent
//! connections to plain `:memory:` each see a different empty database.
//!
//! # Example
//!
//! ```no_run
//! use std::thread;
//! use localcache::{CacheEngine, CacheOptions, ReadPool};
//!
//! // Writer (owned by main thread or dedicated writer thread):
//! let writer: CacheEngine<Vec<f32>> = CacheEngine::builder()
//!     .database("cache.sqlite3")
//!     .build()?;
//!
//! // Read-only pool shared by worker threads:
//! let pool: ReadPool<Vec<f32>> = ReadPool::open(
//!     CacheOptions { database_path: "cache.sqlite3".into(), ..CacheOptions::default() },
//!     4,   // 4 independent connections
//! )?;
//!
//! let pool2 = pool.clone(); // Arc clone — same slots
//! thread::spawn(move || {
//!     let entry = pool2.get_if_fresh("file.txt")?;
//!     Ok::<_, localcache::LocalFileCacheError>(entry)
//! });
//! # Ok::<(), localcache::LocalFileCacheError>(())
//! ```

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Serialize, de::DeserializeOwned};

use crate::CacheStatus;
use crate::cache::engine::CacheEngine;
use crate::cache::entry::{CacheEntry, CacheStats, Diagnosis, EntryInfo, ExportRecord};
use crate::cache::options::{CacheOptions, ScanOptions};
use crate::cache::query::QueryBuilder;
use crate::error::LocalFileCacheError;

// ---------------------------------------------------------------------------
// ReadPool
// ---------------------------------------------------------------------------

/// A cloneable pool of read-only [`CacheEngine`] connections.
///
/// See the [module documentation][crate::read_pool] for details.
#[derive(Clone)]
pub struct ReadPool<T> {
    slots: Arc<Vec<Mutex<CacheEngine<T>>>>,
    next: Arc<AtomicUsize>,
}

impl<T> ReadPool<T>
where
    T: Serialize + DeserializeOwned,
{
    // ------------------------------------------------------------------
    // Construction
    // ------------------------------------------------------------------

    /// Open a pool of `size` read-only connections using `options`.
    ///
    /// `options.read_only` is forced `true`; `options.shared_cache`
    /// selects the connection backend (see module docs).
    ///
    /// # Errors
    ///
    /// - `size == 0` → [`LocalFileCacheError::UnsupportedFeature`].
    /// - `:memory:` database → [`LocalFileCacheError::UnsupportedFeature`]
    ///   (N independent in-memory connections each see a different DB).
    pub fn open(options: CacheOptions, size: usize) -> Result<Self, LocalFileCacheError> {
        if size == 0 {
            return Err(LocalFileCacheError::UnsupportedFeature(
                "ReadPool size must be >= 1".into(),
            ));
        }
        if crate::cache::options::is_memory_path(&options.database_path) {
            return Err(LocalFileCacheError::UnsupportedFeature(
                "ReadPool does not support ':memory:' databases — each slot \
                 would open a separate empty database.  Use ConnectionPool \
                 or a shared-cache engine pair for in-process sharing."
                    .into(),
            ));
        }

        // Force read-only; shared_cache is honoured as-is.
        let base = CacheOptions {
            read_only: true,
            ..options
        };

        let mut slots = Vec::with_capacity(size);
        for _ in 0..size {
            slots.push(Mutex::new(CacheEngine::open(base.clone())?));
        }
        Ok(Self {
            slots: Arc::new(slots),
            next: Arc::new(AtomicUsize::new(0)),
        })
    }

    // ------------------------------------------------------------------
    // Internal: slot checkout
    // ------------------------------------------------------------------

    /// Check out a slot using a round-robin start, scanning with
    /// `try_lock` and falling back to a blocking `lock` on the
    /// round-robin slot if all are busy.  No lock-ordering issues
    /// because only one slot is ever held at a time.
    fn checkout(&self) -> MutexGuard<'_, CacheEngine<T>> {
        let len = self.slots.len();
        let start = self.next.fetch_add(1, Ordering::Relaxed) % len;
        for i in 0..len {
            let idx = (start + i) % len;
            if let Ok(g) = self.slots[idx].try_lock() {
                return g;
            }
        }
        // All slots busy — block on the round-robin slot.
        self.slots[start].lock().unwrap_or_else(|e| e.into_inner())
    }

    // ------------------------------------------------------------------
    // Read API
    // ------------------------------------------------------------------

    /// Return the cached entry for `path`, if one exists.
    ///
    /// Updates `last_accessed_at` — the only state change permitted on a
    /// read-only connection (it is a read on an in-memory field…
    /// technically `last_accessed_at` write requires read-write access,
    /// so on read-only connections the LRU timestamp update is skipped by
    /// the engine guard).  Use [`ConnectionPool`][crate::ConnectionPool]
    /// if LRU tracking matters for your workload.
    pub fn get<P: AsRef<Path>>(&self, path: P) -> Result<Option<CacheEntry<T>>, LocalFileCacheError>
    where
        T: Clone,
    {
        self.checkout().get(path)
    }

    /// Return the cached entry only if it is still fresh.
    pub fn get_if_fresh<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<Option<CacheEntry<T>>, LocalFileCacheError>
    where
        T: Clone,
    {
        self.checkout().get_if_fresh(path)
    }

    /// Fetch multiple entries by path.
    pub fn batch_get<P: AsRef<Path>>(
        &self,
        paths: &[P],
    ) -> Vec<Result<Option<CacheEntry<T>>, LocalFileCacheError>>
    where
        T: Clone,
    {
        self.checkout().batch_get(paths)
    }

    /// Fetch multiple entries, returning `None` for stale or missing ones.
    pub fn batch_get_fresh<P: AsRef<Path>>(
        &self,
        paths: &[P],
    ) -> Vec<Result<Option<CacheEntry<T>>, LocalFileCacheError>>
    where
        T: Clone,
    {
        self.checkout().batch_get_fresh(paths)
    }

    /// Check whether a cached entry for `path` is fresh, stale, or absent.
    pub fn check_status<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<CacheStatus, LocalFileCacheError> {
        self.checkout().check_status(path)
    }

    /// Check status for multiple paths.
    pub fn check_status_batch<P: AsRef<Path>>(
        &self,
        paths: &[P],
    ) -> Vec<Result<CacheStatus, LocalFileCacheError>> {
        self.checkout().check_status_batch(paths)
    }

    /// Return `true` if `path` has a cached entry (fresh or stale).
    pub fn contains<P: AsRef<Path>>(&self, path: P) -> Result<bool, LocalFileCacheError> {
        self.checkout().contains(path)
    }

    /// Return a staleness diagnosis for `path`.
    pub fn explain<P: AsRef<Path>>(&self, path: P) -> Result<Diagnosis, LocalFileCacheError>
    where
        T: Clone,
    {
        self.checkout().explain(path)
    }

    /// Return all cached paths, optionally filtered by a LIKE pattern.
    pub fn keys(&self, path_like: Option<&str>) -> Result<Vec<PathBuf>, LocalFileCacheError> {
        self.checkout().keys(path_like)
    }

    /// List all entries' metadata (no payloads loaded).
    pub fn list_entries(&self) -> Result<Vec<EntryInfo>, LocalFileCacheError> {
        self.checkout().list_entries()
    }

    /// Total number of entries in the current namespace.
    pub fn entry_count(&self) -> Result<usize, LocalFileCacheError> {
        self.checkout().entry_count()
    }

    /// Hit-rate and count statistics for the current namespace.
    pub fn cache_stats(&self) -> Result<CacheStats, LocalFileCacheError> {
        self.checkout().cache_stats()
    }

    /// Export all entries as portable records.
    pub fn export_entries(&self) -> Result<Vec<ExportRecord>, LocalFileCacheError> {
        self.checkout().export_entries()
    }

    /// Scan a directory and return each file's path and cache status.
    pub fn scan_dir<P: AsRef<Path>>(
        &self,
        dir: P,
        recursive: bool,
    ) -> Result<Vec<(PathBuf, CacheStatus)>, LocalFileCacheError> {
        self.checkout().scan_dir(dir, recursive)
    }

    /// Scan with extension, glob, and depth filters.
    pub fn scan_dir_filtered<P: AsRef<Path>>(
        &self,
        dir: P,
        opts: ScanOptions,
    ) -> Result<Vec<(PathBuf, CacheStatus)>, LocalFileCacheError> {
        self.checkout().scan_dir_filtered(dir, opts)
    }

    // ------------------------------------------------------------------
    // Query builder
    // ------------------------------------------------------------------

    /// Run a [`QueryBuilder`] closure against one pool slot.
    ///
    /// The closure receives a `QueryBuilder` and must return it (possibly
    /// with filters applied); the query is executed synchronously on the
    /// checked-out connection.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use localcache::{ReadPool, CacheOptions};
    /// # let pool: ReadPool<Vec<f32>> = ReadPool::open(
    /// #     CacheOptions { database_path: "cache.sqlite3".into(), ..CacheOptions::default() }, 2)?;
    /// let results = pool.query_run(|q| q.path_in_dir("/data", true))?;
    /// # Ok::<(), localcache::LocalFileCacheError>(())
    /// ```
    pub fn query_run<F>(&self, build: F) -> Result<Vec<CacheEntry<T>>, LocalFileCacheError>
    where
        F: for<'e> FnOnce(QueryBuilder<'e, T>) -> QueryBuilder<'e, T>,
    {
        let guard = self.checkout();
        let q = guard.query();
        build(q).run()
    }

    /// Return the EXPLAIN QUERY PLAN output without loading payloads.
    ///
    /// Useful for verifying that index hints take effect under pool
    /// conditions.
    pub fn query_dry_run<F>(&self, build: F) -> Result<String, LocalFileCacheError>
    where
        F: for<'e> FnOnce(QueryBuilder<'e, T>) -> QueryBuilder<'e, T>,
    {
        let guard = self.checkout();
        let q = guard.query();
        build(q).dry_run()
    }

    // ------------------------------------------------------------------
    // Pool metadata
    // ------------------------------------------------------------------

    /// Number of connection slots in this pool.
    pub fn size(&self) -> usize {
        self.slots.len()
    }
}
