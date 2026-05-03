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
}
