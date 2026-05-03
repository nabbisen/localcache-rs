//! # localcache
//!
//! A Rust library for caching the results of expensive computations derived
//! from local files.
//!
//! ```no_run
//! use localcache::{CacheEngine, CacheOptions, ChangeDetectionMode};
//!
//! let engine = CacheEngine::<Vec<f32>>::open(CacheOptions {
//!     database_path: "cache.sqlite3".into(),
//!     change_detection_mode: ChangeDetectionMode::MetadataThenFullHash,
//!     ..CacheOptions::default()
//! })?;
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
mod serialization;

#[cfg(test)]
mod tests;

#[cfg(feature = "async")]
pub use cache::async_engine::AsyncCacheEngine;
pub use cache::engine::{BatchSetReport, CacheEngine};
pub use cache::entry::{CacheEntry, CacheStatus, EntryInfo, FileMetadata};
pub use cache::options::{
    CacheOptions, ChangeDetectionMode, Codec, JournalMode, ScanOptions, SynchronousMode,
};
pub use error::LocalFileCacheError;
