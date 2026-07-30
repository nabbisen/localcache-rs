//! Async wrapper around [`CacheEngine`].

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

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

    /// Lock the shared engine, mapping mutex poisoning to a recoverable
    /// error instead of propagating the panic to every subsequent caller.
    ///
    /// Mirrors [`crate::pool::ConnectionPool`]'s poison-handling contract: a
    /// poisoned mutex still means the data behind it may reflect a partially
    /// completed operation, but this only stops the panic from propagating
    /// to callers who did nothing wrong — it does not attempt to repair
    /// engine state.
    fn lock(
        inner: &Mutex<CacheEngine<T>>,
    ) -> Result<MutexGuard<'_, CacheEngine<T>>, LocalFileCacheError> {
        inner.lock().map_err(|_| LocalFileCacheError::Poisoned {
            resource: "AsyncCacheEngine",
        })
    }

    pub async fn get(&self, path: PathBuf) -> Result<Option<CacheEntry<T>>, LocalFileCacheError>
    where
        T: Clone,
    {
        let inner = Arc::clone(&self.inner);
        spawn(move || Self::lock(&inner)?.get(&path)).await
    }

    pub async fn get_if_fresh(
        &self,
        path: PathBuf,
    ) -> Result<Option<CacheEntry<T>>, LocalFileCacheError>
    where
        T: Clone,
    {
        let inner = Arc::clone(&self.inner);
        spawn(move || Self::lock(&inner)?.get_if_fresh(&path)).await
    }

    pub async fn batch_get(
        &self,
        paths: Vec<PathBuf>,
    ) -> Vec<Result<Option<CacheEntry<T>>, LocalFileCacheError>>
    where
        T: Clone,
    {
        let inner = Arc::clone(&self.inner);
        match spawn(move || Ok(Self::lock(&inner)?.batch_get(&paths))).await {
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
        match spawn(move || Ok(Self::lock(&inner)?.batch_get_fresh(&paths))).await {
            Ok(r) => r,
            Err(e) => vec![Err(e)],
        }
    }

    pub async fn set(&self, path: PathBuf, payload: T) -> Result<(), LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || Self::lock(&inner)?.set(&path, &payload)).await
    }

    pub async fn batch_set(
        &self,
        items: Vec<(PathBuf, T)>,
    ) -> Result<BatchSetReport, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || Self::lock(&inner)?.batch_set(&items)).await
    }

    pub async fn remove(&self, path: PathBuf) -> Result<bool, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || Self::lock(&inner)?.remove(&path)).await
    }

    pub async fn check_status(&self, path: PathBuf) -> Result<CacheStatus, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || Self::lock(&inner)?.check_status(&path)).await
    }

    pub async fn scan_dir(
        &self,
        dir: PathBuf,
        recursive: bool,
    ) -> Result<Vec<(PathBuf, CacheStatus)>, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || Self::lock(&inner)?.scan_dir(&dir, recursive)).await
    }

    pub async fn scan_dir_filtered(
        &self,
        dir: PathBuf,
        options: ScanOptions,
    ) -> Result<Vec<(PathBuf, CacheStatus)>, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || Self::lock(&inner)?.scan_dir_filtered(&dir, options)).await
    }

    pub async fn list_entries(&self) -> Result<Vec<EntryInfo>, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || Self::lock(&inner)?.list_entries()).await
    }

    pub async fn cleanup_missing_files(&self) -> Result<usize, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || Self::lock(&inner)?.cleanup_missing_files()).await
    }

    pub async fn cleanup_expired(&self) -> Result<usize, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || Self::lock(&inner)?.cleanup_expired()).await
    }

    pub async fn purge_stale_versions(&self) -> Result<usize, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || Self::lock(&inner)?.purge_stale_versions()).await
    }

    pub async fn shrink_database(&self) -> Result<(), LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || Self::lock(&inner)?.shrink_database()).await
    }

    pub async fn entry_count(&self) -> Result<usize, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || Self::lock(&inner)?.entry_count()).await
    }

    pub async fn entry_count_by_version(&self) -> Result<Vec<(u32, usize)>, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || Self::lock(&inner)?.entry_count_by_version()).await
    }

    /// Async version of [`CacheEngine::cache_stats`].
    pub async fn cache_stats(
        &self,
    ) -> Result<crate::cache::entry::CacheStats, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || Self::lock(&inner)?.cache_stats()).await
    }

    /// Async version of [`CacheEngine::check_status_batch`].
    pub async fn check_status_batch(
        &self,
        paths: Vec<PathBuf>,
    ) -> Vec<Result<CacheStatus, LocalFileCacheError>> {
        let inner = Arc::clone(&self.inner);
        match spawn(move || Ok(Self::lock(&inner)?.check_status_batch(&paths))).await {
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
        spawn(move || Self::lock(&inner)?.rotate_encryption_key(&new_key)).await
    }

    /// Async version of [`CacheEngine::export_entries`].
    pub async fn export_entries(
        &self,
    ) -> Result<Vec<crate::cache::entry::ExportRecord>, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || Self::lock(&inner)?.export_entries()).await
    }

    /// Async version of [`CacheEngine::import_entries`].
    pub async fn import_entries(
        &self,
        records: Vec<crate::cache::entry::ExportRecord>,
    ) -> Result<usize, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || Self::lock(&inner)?.import_entries(&records)).await
    }

    /// Async version of [`CacheEngine::touch`].
    pub async fn touch(&self, path: PathBuf) -> Result<bool, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || Self::lock(&inner)?.touch(&path)).await
    }

    /// Async version of [`CacheEngine::contains`].
    pub async fn contains(&self, path: PathBuf) -> Result<bool, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || Self::lock(&inner)?.contains(&path)).await
    }

    /// Async version of [`CacheEngine::explain`].
    pub async fn explain(
        &self,
        path: PathBuf,
    ) -> Result<crate::cache::entry::Diagnosis, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || Self::lock(&inner)?.explain(&path)).await
    }

    /// Async version of [`CacheEngine::keys`].
    pub async fn keys(
        &self,
        path_like: Option<String>,
    ) -> Result<Vec<PathBuf>, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || Self::lock(&inner)?.keys(path_like.as_deref())).await
    }

    /// Execute a pre-built [`crate::QueryBuilder`] asynchronously.
    ///
    /// Because `QueryBuilder` borrows the engine, building it synchronously
    /// and then calling this method avoids lifetime issues across await points.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use localcache::{AsyncCacheEngine, CacheOptions};
    /// # #[tokio::main] async fn main() -> Result<(), localcache::LocalFileCacheError> {
    /// let engine: AsyncCacheEngine<Vec<f32>> = AsyncCacheEngine::open(CacheOptions {
    ///     database_path: ":memory:".into(),
    ///     ..CacheOptions::default()
    /// }).await?;
    ///
    /// let results: Vec<localcache::CacheEntry<Vec<f32>>> =
    ///     engine.query_run(|q| q.path_like("%.txt")).await?;
    /// # Ok(()) }
    /// ```
    pub async fn query_run<F, U>(
        &self,
        build: F,
    ) -> Result<Vec<crate::cache::entry::CacheEntry<U>>, LocalFileCacheError>
    where
        U: serde::Serialize + serde::de::DeserializeOwned + Send + 'static,
        F: FnOnce(
                crate::cache::query::QueryBuilder<'_, U>,
            ) -> crate::cache::query::QueryBuilder<'_, U>
            + Send
            + 'static,
        T: Send + 'static,
    {
        let inner = Arc::clone(&self.inner);
        spawn(move || {
            let guard = Self::lock(&inner)?;
            // `EngineCore` borrows only the payload-type-independent parts
            // of the engine (connection, namespace, and — if enabled — the
            // encryption key), so a `QueryBuilder<'_, U>` can be built
            // directly here even though `guard`'s engine is `CacheEngine<T>`
            // and `U` may differ from `T`. No reinterpretation of
            // `CacheEngine<T>` as `CacheEngine<U>` is needed or performed.
            let q = crate::cache::query::QueryBuilder {
                core: guard.core(),
                _phantom: std::marker::PhantomData::<U>,
                #[cfg(feature = "json")]
                predicates: Vec::new(),
                limit: None,
                offset: 0,
                path_like: None,
                index_hint: None,
                path_in_dir: None,
                path_glob: None,
                order_by: Vec::new(),
            };
            let q = build(q);
            crate::cache::query::execute_query(q)
        })
        .await
    }

    /// Async version of [`crate::CacheEngine::query`] + [`crate::QueryBuilder::dry_run`].
    ///
    /// Returns the SQLite `EXPLAIN QUERY PLAN` output for the configured
    /// query without loading any payloads.
    ///
    /// ```no_run
    /// # use localcache::{AsyncCacheEngine, CacheOptions};
    /// # #[tokio::main] async fn main() -> Result<(), localcache::LocalFileCacheError> {
    /// let engine = AsyncCacheEngine::<Vec<f32>>::open(CacheOptions {
    ///     database_path: ":memory:".into(),
    ///     ..CacheOptions::default()
    /// }).await?;
    /// let plan = engine.query_dry_run(|q| q.path_like("%.txt")).await?;
    /// println!("{plan}");
    /// # Ok(()) }
    /// ```
    pub async fn query_dry_run<F>(&self, build: F) -> Result<String, LocalFileCacheError>
    where
        F: FnOnce(
                crate::cache::query::QueryBuilder<'_, T>,
            ) -> crate::cache::query::QueryBuilder<'_, T>
            + Send
            + 'static,
        T: Send + 'static,
    {
        let inner = Arc::clone(&self.inner);
        spawn(move || {
            let guard = Self::lock(&inner)?;
            let engine_ref: &crate::cache::engine::CacheEngine<T> = &guard;
            let q = engine_ref.query();
            let q = build(q);
            q.dry_run()
        })
        .await
    }

    /// Async version of [`CacheEngine::create_path_index`].
    pub async fn create_path_index(&self, name: String) -> Result<String, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || Self::lock(&inner)?.create_path_index(&name)).await
    }

    /// Async version of [`CacheEngine::drop_path_index`].
    pub async fn drop_path_index(&self, name: String) -> Result<bool, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || Self::lock(&inner)?.drop_path_index(&name)).await
    }

    /// Async version of [`CacheEngine::list_path_indexes`].
    pub async fn list_path_indexes(&self) -> Result<Vec<String>, LocalFileCacheError> {
        let inner = Arc::clone(&self.inner);
        spawn(move || Self::lock(&inner)?.list_path_indexes()).await
    }
}

async fn spawn<F, R>(f: F) -> Result<R, LocalFileCacheError>
where
    F: FnOnce() -> Result<R, LocalFileCacheError> + Send + 'static,
    R: Send + 'static,
{
    crate::cache::runtime::spawn_blocking(f).await
}
