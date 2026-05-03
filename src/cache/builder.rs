//! Fluent builder for [`CacheEngine`].
//!
//! Prefer the builder over constructing [`CacheOptions`] by hand when you want
//! method chaining and compile-time guidance for which options are available.
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
//!     .build()?;
//! # Ok::<(), localcache::LocalFileCacheError>(())
//! ```

use std::marker::PhantomData;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Serialize, de::DeserializeOwned};

use crate::cache::engine::CacheEngine;
use crate::cache::options::{
    CacheOptions, ChangeDetectionMode, Codec, JournalMode, SynchronousMode,
};
use crate::error::LocalFileCacheError;

/// Fluent builder for [`CacheEngine<T>`].
///
/// Obtain one via [`CacheEngine::builder`].
pub struct CacheEngineBuilder<T> {
    opts: CacheOptions,
    _phantom: PhantomData<T>,
}

impl<T> CacheEngineBuilder<T>
where
    T: Serialize + DeserializeOwned,
{
    pub(crate) fn new() -> Self {
        Self {
            opts: CacheOptions::default(),
            _phantom: PhantomData,
        }
    }

    // ------------------------------------------------------------------
    // Storage
    // ------------------------------------------------------------------

    /// Set the path to the SQLite database file.
    ///
    /// Pass `":memory:"` for an ephemeral in-memory database.
    /// Defaults to `"localcache.sqlite3"`.
    pub fn database<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.opts.database_path = path.into();
        self
    }

    /// Namespace that logically partitions entries within one database file.
    /// Defaults to `"default"`.
    pub fn namespace(mut self, ns: impl Into<String>) -> Self {
        self.opts.namespace = ns.into();
        self
    }

    // ------------------------------------------------------------------
    // Change detection
    // ------------------------------------------------------------------

    /// File change-detection algorithm.  Defaults to `MetadataOnly`.
    pub fn change_detection(mut self, mode: ChangeDetectionMode) -> Self {
        self.opts.change_detection_mode = mode;
        self
    }

    // ------------------------------------------------------------------
    // Codec
    // ------------------------------------------------------------------

    /// Payload serialization codec.  Defaults to `Codec::Bincode`.
    pub fn codec(mut self, codec: Codec) -> Self {
        self.opts.codec = codec;
        self
    }

    // ------------------------------------------------------------------
    // SQLite tuning
    // ------------------------------------------------------------------

    /// SQLite journal mode.  Defaults to `JournalMode::Wal`.
    pub fn journal_mode(mut self, mode: JournalMode) -> Self {
        self.opts.journal_mode = mode;
        self
    }

    /// SQLite `synchronous` pragma.  Defaults to `SynchronousMode::Normal`.
    pub fn synchronous(mut self, mode: SynchronousMode) -> Self {
        self.opts.synchronous = mode;
        self
    }

    // ------------------------------------------------------------------
    // Entry lifecycle
    // ------------------------------------------------------------------

    /// Time-to-live for cache entries.  `None` (default) means no expiry.
    pub fn ttl(mut self, duration: Duration) -> Self {
        self.opts.ttl = Some(duration);
        self
    }

    /// Remove the TTL so entries never expire by age.
    pub fn no_ttl(mut self) -> Self {
        self.opts.ttl = None;
        self
    }

    /// Maximum number of entries in the namespace.  Excess entries are evicted
    /// in LRU order (least recently accessed) after each `set`.
    pub fn max_entries(mut self, n: usize) -> Self {
        self.opts.max_entries = Some(n);
        self
    }

    /// Payload schema version.  Non-zero enables version checks in freshness
    /// queries.  Defaults to `0` (disabled).
    pub fn payload_version(mut self, v: u32) -> Self {
        self.opts.payload_version = v;
        self
    }

    // ------------------------------------------------------------------
    // Access mode
    // ------------------------------------------------------------------

    /// Open the database in read-only mode.  Write operations return
    /// `LocalFileCacheError::ReadOnly`.
    pub fn read_only(mut self) -> Self {
        self.opts.read_only = true;
        self
    }

    // ------------------------------------------------------------------
    // Feature-gated options
    // ------------------------------------------------------------------

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

    // ------------------------------------------------------------------
    // Terminal
    // ------------------------------------------------------------------

    /// Consume the builder and open the [`CacheEngine`].
    pub fn build(self) -> Result<CacheEngine<T>, LocalFileCacheError> {
        CacheEngine::open(self.opts)
    }
}
