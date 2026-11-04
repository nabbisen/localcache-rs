//! Database layer.

mod error;
pub(crate) mod indexes;
pub(crate) mod repository;
pub(crate) mod schema;
mod types;

pub use error::DatabaseError;
pub(crate) use error::StorageError;
pub use types::{EntryInfo, FileMetadata, JournalMode, SynchronousMode};
