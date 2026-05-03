//! # localcache
//!
//! `localcache` is a Rust library for caching the results of expensive
//! computations that are derived from local files.
//!
//! ## Quick start
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
//! let path = "sample.txt";
//! let embedding = vec![0.1_f32, 0.2, 0.3];
//!
//! engine.set(path, &embedding)?;
//!
//! if let Some(entry) = engine.get_if_fresh(path)? {
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

// ---------------------------------------------------------------------------
// Public re-exports
// ---------------------------------------------------------------------------

pub use cache::engine::{BatchSetReport, CacheEngine};
pub use cache::entry::{CacheEntry, CacheStatus, FileMetadata};
pub use cache::options::{CacheOptions, ChangeDetectionMode, JournalMode, SynchronousMode};
pub use error::LocalFileCacheError;
