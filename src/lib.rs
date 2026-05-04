//! # localcache
//!
//! A Rust library for caching the results of expensive computations derived
//! from local files.
//!
//! ```no_run
//! use localcache::{CacheEngine, ChangeDetectionMode};
//!
//! let engine = CacheEngine::<Vec<f32>>::builder()
//!     .database("cache.sqlite3")
//!     .change_detection(ChangeDetectionMode::MetadataThenFullHash)
//!     .max_entries(1000)
//!     .build()?;
//!
//! engine.set("sample.txt", &vec![0.1_f32, 0.2, 0.3])?;
//!
//! if let Some(entry) = engine.get_if_fresh("sample.txt")? {
//!     println!("cached vector: {:?}", entry.payload);
//! }
//! # Ok::<(), localcache::LocalFileCacheError>(())
//! ```

mod cache;
mod db;
mod detection;
mod error;
mod path;
mod pool;
mod serialization;

#[cfg(feature = "async")]
pub use cache::async_engine::AsyncCacheEngine;
pub use cache::builder::CacheEngineBuilder;
pub use cache::engine::{BatchSetReport, CacheEngine};
pub use cache::entry::{
    CacheEntry, CacheStats, CacheStatus, Diagnosis, EntryInfo, ExportRecord, FileMetadata,
    MetadataDiff, PayloadVersionInfo, PreloadReport,
};
pub use cache::options::{
    CacheOptions, ChangeDetectionMode, Codec, JournalMode, ScanOptions, SynchronousMode,
};
pub use cache::query::{QueryBuilder, SortOrder};
pub use error::LocalFileCacheError;
pub use pool::{CacheOptionsExt, ConnectionPool, SharedEngine, shared_engine};

#[cfg(feature = "watching")]
pub use cache::entry::{InvalidationReason, WatchEvent};
#[cfg(feature = "watching")]
pub use cache::watcher::{CacheDebouncedWatcher, CacheWatcher};
