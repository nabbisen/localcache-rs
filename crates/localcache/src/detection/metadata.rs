//! File metadata types and collection.

use std::path::Path;
use std::time::UNIX_EPOCH;

use crate::db::FileMetadata;
use crate::error::LocalFileCacheError;

/// Read the current [`FileMetadata`] from the filesystem for `path`.
///
/// `hash` is left as `None`; callers that need a hash must compute it
/// separately and set the field themselves.
pub(crate) fn collect_metadata(path: &Path) -> Result<FileMetadata, LocalFileCacheError> {
    let meta = std::fs::metadata(path)?;
    let mtime = meta
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0);
    let file_size = meta.len();
    Ok(FileMetadata {
        mtime,
        file_size,
        hash: None,
    })
}
