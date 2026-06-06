//! Path normalization utilities.
//!
//! All paths stored in the database are canonical absolute paths. This module
//! provides the single function used throughout the library to obtain them.

//! Path canonicalization helpers.
//!
//! # Path-handling contract
//!
//! Every operation that accepts a file path (`set`, `get`, `get_if_fresh`,
//! `remove`, `contains`, `check_status`, …) calls `normalize_path` before
//! touching the database.  The stored key is therefore the **canonical
//! absolute path at write time** (via `Path::canonicalize()`).
//!
//! When the file no longer exists on disk, `normalize_path` returns
//! `Err(FileNotFound)` and callers fall back to the **raw path string** for
//! lookups.  This means entries for deleted files remain accessible for
//! read and delete operations, using the path as originally supplied.
//!
//! **Practical rule for applications:** always go through the `localcache`
//! API rather than comparing stored path strings directly.  Relative,
//! symlinked, or differently-cased path inputs all resolve through
//! canonicalization to the same stored key.

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
