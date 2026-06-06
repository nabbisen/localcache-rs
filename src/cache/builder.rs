//! Fluent builder for [`CacheEngine`].
//!
//! # Example
//!
//! ```no_run
//! use std::time::Duration;
//! use localcache::{CacheEngine, ChangeDetectionMode};
//!
//! let engine = CacheEngine::<Vec<f32>>::builder()
//!     .database("cache.sqlite3")
//!     .namespace("embeddings")
//!     .change_detection(ChangeDetectionMode::MetadataThenFullHash)
//!     .ttl(Duration::from_secs(3600))
//!     .max_entries(1000)
//!     .payload_version(2)
//!     .on_evict(|path| eprintln!("evicted: {}", path.display()))
//!     .build()?;
//! # Ok::<(), localcache::LocalFileCacheError>(())
//! ```

use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Serialize, de::DeserializeOwned};

use crate::cache::engine::{CacheEngine, EvictCallback};
use crate::cache::options::{
    CacheOptions, ChangeDetectionMode, Codec, JournalMode, SynchronousMode,
};
use crate::error::LocalFileCacheError;

/// Fluent builder for [`CacheEngine<T>`].
///
/// Obtain one via [`CacheEngine::builder`].
pub struct CacheEngineBuilder<T> {
    opts: CacheOptions,
    evict_callback: Option<EvictCallback>,
    _phantom: PhantomData<T>,
}

impl<T> CacheEngineBuilder<T>
where
    T: Serialize + DeserializeOwned,
{
    pub(crate) fn new() -> Self {
        Self {
            opts: CacheOptions::default(),
            evict_callback: None,
            _phantom: PhantomData,
        }
    }

    /// Set the path to the SQLite database file.
    pub fn database<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.opts.database_path = path.into();
        self
    }

    /// Namespace.  Defaults to `"default"`.
    pub fn namespace(mut self, ns: impl Into<String>) -> Self {
        self.opts.namespace = ns.into();
        self
    }

    /// File change-detection algorithm.
    pub fn change_detection(mut self, mode: ChangeDetectionMode) -> Self {
        self.opts.change_detection_mode = mode;
        self
    }

    /// Payload serialization codec.
    pub fn codec(mut self, codec: Codec) -> Self {
        self.opts.codec = codec;
        self
    }

    /// SQLite journal mode.
    pub fn journal_mode(mut self, mode: JournalMode) -> Self {
        self.opts.journal_mode = mode;
        self
    }

    /// SQLite `synchronous` pragma.
    pub fn synchronous(mut self, mode: SynchronousMode) -> Self {
        self.opts.synchronous = mode;
        self
    }

    /// Time-to-live for cache entries.
    pub fn ttl(mut self, duration: Duration) -> Self {
        self.opts.ttl = Some(duration);
        self
    }

    /// Remove the TTL so entries never expire by age.
    pub fn no_ttl(mut self) -> Self {
        self.opts.ttl = None;
        self
    }

    /// Maximum entries in the namespace (LRU eviction).
    pub fn max_entries(mut self, n: usize) -> Self {
        self.opts.max_entries = Some(n);
        self
    }

    /// Payload schema version.
    pub fn payload_version(mut self, v: u32) -> Self {
        self.opts.payload_version = v;
        self
    }

    /// Register a callback invoked with the path of each LRU-evicted entry.
    ///
    /// The callback is called **after** the entry is deleted from the database.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use localcache::CacheEngine;
    ///
    /// let engine = CacheEngine::<Vec<f32>>::builder()
    ///     .database(":memory:")
    ///     .max_entries(100)
    ///     .on_evict(|path| eprintln!("evicted: {}", path.display()))
    ///     .build()?;
    /// # Ok::<(), localcache::LocalFileCacheError>(())
    /// ```
    pub fn on_evict<F>(mut self, callback: F) -> Self
    where
        F: Fn(&Path) + Send + Sync + 'static,
    {
        self.evict_callback = Some(Arc::new(callback));
        self
    }

    /// Open the database in read-only mode.
    pub fn read_only(mut self) -> Self {
        self.opts.read_only = true;
        self
    }

    /// Open in read-only **shared-cache** mode (see
    /// [`CacheOptions::shared_cache`]).
    ///
    /// Engines opened with this option on the same database file within one
    /// process share the SQLite page cache.  Implies read-only for
    /// file-backed databases; write methods return
    /// [`LocalFileCacheError::ReadOnly`].
    ///
    /// With `":memory:"`, opens a named shared in-memory database in
    /// read-write mode instead — all engines opened this way within the
    /// process share the same data.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use localcache::CacheEngine;
    ///
    /// let reader = CacheEngine::<Vec<f32>>::builder()
    ///     .database("cache.sqlite3")
    ///     .shared_cache()
    ///     .build()?;
    /// # Ok::<(), localcache::LocalFileCacheError>(())
    /// ```
    pub fn shared_cache(mut self) -> Self {
        self.opts.shared_cache = true;
        self
    }

    /// Pre-register every cached path's **parent directory** for recursive
    /// watching when [`CacheEngine::watcher`] /
    /// [`CacheEngine::debounced_watcher`] is called, instead of registering
    /// each file individually.
    ///
    /// Directory watching covers files cached after the watcher starts and
    /// reduces O(n) per-file registrations to one OS watch per directory.
    /// Events for uncached files in the watched subtrees are filtered out by
    /// the watcher callback.
    ///
    /// Defaults to `false` (per-file registration).
    ///
    /// Requires the `watching` Cargo feature.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use localcache::CacheEngine;
    ///
    /// let engine = CacheEngine::<Vec<f32>>::builder()
    ///     .database("cache.sqlite3")
    ///     .watch_dirs(true)
    ///     .build()?;
    /// let watcher = engine.watcher()?; // registers directories recursively
    /// # Ok::<(), localcache::LocalFileCacheError>(())
    /// ```
    #[cfg(feature = "watching")]
    pub fn watch_dirs(mut self, enable: bool) -> Self {
        self.opts.watch_dirs = enable;
        self
    }

    /// Enable zstd compression of stored payloads.
    ///
    /// Requires the `compression` Cargo feature.
    #[cfg(feature = "compression")]
    pub fn compress(mut self) -> Self {
        self.opts.compress_payloads = true;
        self
    }

    /// Set the AES-256-GCM encryption key (exactly 32 bytes).
    ///
    /// Requires the `encryption` Cargo feature.
    #[cfg(feature = "encryption")]
    pub fn encryption_key(mut self, key: Vec<u8>) -> Self {
        self.opts.encryption_key = Some(key);
        self
    }

    /// Consume the builder and open the [`CacheEngine`].
    pub fn build(self) -> Result<CacheEngine<T>, LocalFileCacheError> {
        let mut engine = CacheEngine::open(self.opts)?;
        engine.evict_callback = self.evict_callback;
        Ok(engine)
    }

    /// Consume the builder and open a read-only [`ReadPool`] of `size`
    /// connections.
    ///
    /// The `read_only` flag is forced `true`; all other options (namespace,
    /// change detection, codec, `shared_cache`, …) are forwarded to each
    /// slot.  See [`ReadPool`][crate::ReadPool] for details.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use localcache::CacheEngine;
    ///
    /// let pool = CacheEngine::<Vec<f32>>::builder()
    ///     .database("cache.sqlite3")
    ///     .namespace("embeddings")
    ///     .build_read_pool(4)?;  // 4 concurrent read-only connections
    /// # Ok::<(), localcache::LocalFileCacheError>(())
    /// ```
    pub fn build_read_pool(self, size: usize) -> Result<crate::ReadPool<T>, LocalFileCacheError> {
        crate::ReadPool::open(self.opts, size)
    }
}
