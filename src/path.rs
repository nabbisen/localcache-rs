//! Path normalization utilities.
//!
//! All paths stored in the database are canonical absolute paths. This module
//! provides the single function used throughout the library to obtain them.

use std::path::{Path, PathBuf};

use crate::error::LocalFileCacheError;

/// Returns the canonical, absolute form of `path`.
///
/// This calls [`std::fs::canonicalize`], which requires the file to exist on
/// disk.  If the file is not found an [`LocalFileCacheError::FileNotFound`] is
/// returned; other I/O failures are wrapped in [`LocalFileCacheError::Io`].
pub(crate) fn normalize_path(path: &Path) -> Result<PathBuf, LocalFileCacheError> {
    match path.canonicalize() {
        Ok(p) => Ok(p),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(LocalFileCacheError::FileNotFound {
                path: path.to_path_buf(),
            })
        }
        Err(e) => Err(LocalFileCacheError::Io(e)),
    }
}
