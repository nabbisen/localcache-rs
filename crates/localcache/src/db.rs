//! Database layer.

mod error;
pub(crate) mod indexes;
pub(crate) mod repository;
pub(crate) mod schema;
mod store;
mod types;

pub use error::DatabaseError;
pub(crate) use error::StorageError;
pub(crate) use repository::{
    CandidateRow, FullRow, ID_LIST_CHUNK, now_secs, unrepresentable_recency, use_value_of_record,
};
pub(crate) use store::{Location, Store, StoreConfig, WriteTx};
pub use types::{EntryInfo, FileMetadata, JournalMode, SynchronousMode};
