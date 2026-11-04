//! Database layer.

pub(crate) mod indexes;
pub(crate) mod repository;
pub(crate) mod schema;
mod types;

pub use types::{EntryInfo, FileMetadata, JournalMode, SynchronousMode};
