//! Async wrapper around [`CacheEngine`].

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Serialize, de::DeserializeOwned};

use crate::cache::engine::{BatchSetReport, CacheEngine};
use crate::cache::entry::{CacheEntry, CacheStatus, EntryInfo};
use crate::cache::options::{CacheOptions, ScanOptions};
use crate::error::LocalFileCacheError;

/// Async wrapper around [`CacheEngine`].
///
/// Every blocking operation runs on `tokio::task::spawn_blocking`.
/// `AsyncCacheEngine` is `Clone` — all clones share the same engine.
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
    pub async fn open(options: CacheOptions) -> Result<Self, LocalFileCacheError> {
        spawn(move || CacheEngine::open(options))
            .await
            .map(|engine| Self {
                inner: Arc::new(Mutex::new(engine)),
            })
    }

    pub async fn get(&self, path: PathBuf) -> Result<Option<CacheEntry<T>>, LocalFileCacheError>
    where
        T: Clone,
    {
        let inner = Arc::clone(&self.inner);
        spawn(move || inner.lock().unwrap().get(&path)).await
    }

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

    pub async fn batch_get(
        &self,
        paths: Vec<PathBuf>,
    ) -> Vec<Result<Option<CacheEntry<T>>, LocalFileCacheError>>
    where
        T: Clone,
    {
        let inner = Arc::clone(&self.inner);
        match spawn(move || Ok(inner.lock().unwrap().batch_get(&paths))).await {
            Ok(r) => r,
            Err(e) => vec![Err(e)],
        }
    }

    pub async fn batch_get_fresh(
        &self,
        paths: Vec<PathBuf>,
    ) -> Vec<Result<Option<CacheEntry<T>>, LocalFileCacheError>>
    where
        T: Clone,
    {
        let inner = Arc::clone(&self.inner);
        match spawn(move || Ok(inner.lock().unwrap().batch_get_fresh(&paths))).await {
            Ok(r) => r,
            Err(e) => vec![Err(e)],
        }
    }

    pub async fn set(&self, path: PathBuf, payload: T) -> Result<(), LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || inner.lock().unwrap().set(&path, &payload)).await
    }

    pub async fn batch_set(
        &self,
        items: Vec<(PathBuf, T)>,
    ) -> Result<BatchSetReport, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || inner.lock().unwrap().batch_set(&items)).await
    }

    pub async fn remove(&self, path: PathBuf) -> Result<bool, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || inner.lock().unwrap().remove(&path)).await
    }

    pub async fn check_status(&self, path: PathBuf) -> Result<CacheStatus, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || inner.lock().unwrap().check_status(&path)).await
    }

    pub async fn scan_dir(
        &self,
        dir: PathBuf,
        recursive: bool,
    ) -> Result<Vec<(PathBuf, CacheStatus)>, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || inner.lock().unwrap().scan_dir(&dir, recursive)).await
    }

    pub async fn scan_dir_filtered(
        &self,
        dir: PathBuf,
        options: ScanOptions,
    ) -> Result<Vec<(PathBuf, CacheStatus)>, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || inner.lock().unwrap().scan_dir_filtered(&dir, options)).await
    }

    pub async fn list_entries(&self) -> Result<Vec<EntryInfo>, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || inner.lock().unwrap().list_entries()).await
    }

    pub async fn cleanup_missing_files(&self) -> Result<usize, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || inner.lock().unwrap().cleanup_missing_files()).await
    }

    pub async fn cleanup_expired(&self) -> Result<usize, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || inner.lock().unwrap().cleanup_expired()).await
    }

    pub async fn purge_stale_versions(&self) -> Result<usize, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || inner.lock().unwrap().purge_stale_versions()).await
    }

    pub async fn shrink_database(&self) -> Result<(), LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || inner.lock().unwrap().shrink_database()).await
    }

    pub async fn entry_count(&self) -> Result<usize, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || inner.lock().unwrap().entry_count()).await
    }

    pub async fn entry_count_by_version(&self) -> Result<Vec<(u32, usize)>, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || inner.lock().unwrap().entry_count_by_version()).await
    }

    /// Async version of [`CacheEngine::cache_stats`].
    pub async fn cache_stats(
        &self,
    ) -> Result<crate::cache::entry::CacheStats, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || inner.lock().unwrap().cache_stats()).await
    }

    /// Async version of [`CacheEngine::check_status_batch`].
    pub async fn check_status_batch(
        &self,
        paths: Vec<PathBuf>,
    ) -> Vec<Result<CacheStatus, LocalFileCacheError>> {
        let inner = Arc::clone(&self.inner);
        match spawn(move || Ok(inner.lock().unwrap().check_status_batch(&paths))).await {
            Ok(r) => r,
            Err(e) => vec![Err(e)],
        }
    }

    /// Async version of [`CacheEngine::rotate_encryption_key`].
    #[cfg(feature = "encryption")]
    pub async fn rotate_encryption_key(
        &self,
        new_key: Vec<u8>,
    ) -> Result<usize, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || inner.lock().unwrap().rotate_encryption_key(&new_key)).await
    }
}

async fn spawn<F, R>(f: F) -> Result<R, LocalFileCacheError>
where
    F: FnOnce() -> Result<R, LocalFileCacheError> + Send + 'static,
    R: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| LocalFileCacheError::AsyncTaskPanicked)?
}
