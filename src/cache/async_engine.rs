//! Async wrapper around [`CacheEngine`].
//!
//! `AsyncCacheEngine<T>` wraps a synchronous [`CacheEngine<T>`] behind an
//! `Arc<Mutex<…>>` and offloads every blocking operation to
//! `tokio::task::spawn_blocking`.  This keeps the async executor free while
//! SQLite and filesystem work runs on the blocking thread pool.
//!
//! ## Requirements
//!
//! * The `async` feature must be enabled: `localcache = { features = ["async"] }`.
//! * A tokio runtime must be active when calling these methods.
//! * `T` must be `Send + 'static` in addition to the usual
//!   `Serialize + DeserializeOwned` bounds.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Serialize, de::DeserializeOwned};

use crate::cache::engine::{BatchSetReport, CacheEngine};
use crate::cache::entry::{CacheEntry, CacheStatus};
use crate::cache::options::CacheOptions;
use crate::error::LocalFileCacheError;

// ---------------------------------------------------------------------------
// AsyncCacheEngine
// ---------------------------------------------------------------------------

/// An async wrapper around [`CacheEngine`].
///
/// Every method that would block (filesystem I/O, SQLite operations) is
/// executed on tokio's blocking thread pool via
/// `tokio::task::spawn_blocking`, making it safe to call from async code
/// without blocking the executor.
///
/// # Example
///
/// ```no_run
/// use localcache::{AsyncCacheEngine, CacheOptions, ChangeDetectionMode};
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let engine = AsyncCacheEngine::<Vec<f32>>::open(CacheOptions {
///         database_path: "cache.sqlite3".into(),
///         change_detection_mode: ChangeDetectionMode::MetadataThenFullHash,
///         ..CacheOptions::default()
///     })
///     .await?;
///
///     engine.set("sample.txt".into(), vec![0.1_f32, 0.2, 0.3]).await?;
///
///     if let Some(entry) = engine.get_if_fresh("sample.txt".into()).await? {
///         println!("cached: {:?}", entry.payload);
///     }
///     Ok(())
/// }
/// ```
#[derive(Clone)]
pub struct AsyncCacheEngine<T> {
    inner: Arc<Mutex<CacheEngine<T>>>,
}

impl<T> AsyncCacheEngine<T>
where
    T: Serialize + DeserializeOwned + Send + 'static,
{
    // ------------------------------------------------------------------
    // Construction
    // ------------------------------------------------------------------

    /// Open (or create) an [`AsyncCacheEngine`].
    pub async fn open(options: CacheOptions) -> Result<Self, LocalFileCacheError> {
        spawn(move || CacheEngine::open(options))
            .await
            .map(|engine| Self {
                inner: Arc::new(Mutex::new(engine)),
            })
    }

    // ------------------------------------------------------------------
    // Reads
    // ------------------------------------------------------------------

    /// Async version of [`CacheEngine::get`].
    pub async fn get(&self, path: PathBuf) -> Result<Option<CacheEntry<T>>, LocalFileCacheError>
    where
        T: Clone,
    {
        let inner = Arc::clone(&self.inner);
        spawn(move || inner.lock().unwrap().get(&path)).await
    }

    /// Async version of [`CacheEngine::get_if_fresh`].
    pub async fn get_if_fresh(
        &self,
        path: PathBuf,
    ) -> Result<Option<CacheEntry<T>>, LocalFileCacheError>
    where
        T: Clone,
    {
        let inner = Arc::clone(&self.inner);
        spawn(move || inner.lock().unwrap().get_if_fresh(&path)).await
    }

    /// Async version of [`CacheEngine::batch_get`].
    pub async fn batch_get(
        &self,
        paths: Vec<PathBuf>,
    ) -> Vec<Result<Option<CacheEntry<T>>, LocalFileCacheError>>
    where
        T: Clone,
    {
        let inner = Arc::clone(&self.inner);
        match spawn(move || Ok(inner.lock().unwrap().batch_get(&paths))).await {
            Ok(results) => results,
            Err(e) => vec![Err(e)],
        }
    }

    /// Async version of [`CacheEngine::batch_get_fresh`].
    pub async fn batch_get_fresh(
        &self,
        paths: Vec<PathBuf>,
    ) -> Vec<Result<Option<CacheEntry<T>>, LocalFileCacheError>>
    where
        T: Clone,
    {
        let inner = Arc::clone(&self.inner);
        match spawn(move || Ok(inner.lock().unwrap().batch_get_fresh(&paths))).await {
            Ok(results) => results,
            Err(e) => vec![Err(e)],
        }
    }

    // ------------------------------------------------------------------
    // Writes
    // ------------------------------------------------------------------

    /// Async version of [`CacheEngine::set`].
    pub async fn set(&self, path: PathBuf, payload: T) -> Result<(), LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || inner.lock().unwrap().set(&path, &payload)).await
    }

    /// Async version of [`CacheEngine::batch_set`].
    pub async fn batch_set(
        &self,
        items: Vec<(PathBuf, T)>,
    ) -> Result<BatchSetReport, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || inner.lock().unwrap().batch_set(&items)).await
    }

    // ------------------------------------------------------------------
    // Removal
    // ------------------------------------------------------------------

    /// Async version of [`CacheEngine::remove`].
    pub async fn remove(&self, path: PathBuf) -> Result<bool, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || inner.lock().unwrap().remove(&path)).await
    }

    // ------------------------------------------------------------------
    // Status
    // ------------------------------------------------------------------

    /// Async version of [`CacheEngine::check_status`].
    pub async fn check_status(&self, path: PathBuf) -> Result<CacheStatus, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || inner.lock().unwrap().check_status(&path)).await
    }

    // ------------------------------------------------------------------
    // Directory scan
    // ------------------------------------------------------------------

    /// Async version of [`CacheEngine::scan_dir`].
    pub async fn scan_dir(
        &self,
        dir: PathBuf,
        recursive: bool,
    ) -> Result<Vec<(PathBuf, CacheStatus)>, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || inner.lock().unwrap().scan_dir(&dir, recursive)).await
    }

    // ------------------------------------------------------------------
    // Maintenance
    // ------------------------------------------------------------------

    /// Async version of [`CacheEngine::cleanup_missing_files`].
    pub async fn cleanup_missing_files(&self) -> Result<usize, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || inner.lock().unwrap().cleanup_missing_files()).await
    }

    /// Async version of [`CacheEngine::cleanup_expired`].
    pub async fn cleanup_expired(&self) -> Result<usize, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || inner.lock().unwrap().cleanup_expired()).await
    }

    /// Async version of [`CacheEngine::shrink_database`].
    pub async fn shrink_database(&self) -> Result<(), LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || inner.lock().unwrap().shrink_database()).await
    }
}

// ---------------------------------------------------------------------------
// Internal helper
// ---------------------------------------------------------------------------

/// Run `f` on the tokio blocking thread pool, propagating panics as
/// [`LocalFileCacheError::AsyncTaskPanicked`].
async fn spawn<F, R>(f: F) -> Result<R, LocalFileCacheError>
where
    F: FnOnce() -> Result<R, LocalFileCacheError> + Send + 'static,
    R: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| LocalFileCacheError::AsyncTaskPanicked)?
}

#[allow(dead_code)]
fn static_assertions_check() {}
