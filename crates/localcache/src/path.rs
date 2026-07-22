//! Path canonicalization helpers.
//!
//! # Path-handling contract
//!
//! Normal `set` operations store a valid UTF-8 canonical absolute path.
//! Imported records retain their supplied valid UTF-8 stored key.
//!
//! When a source no longer exists, observing and removal operations use only
//! the caller's exact valid UTF-8 string. They never guess by basename,
//! suffix, lossy conversion, symlink alias, or former relative spelling.
//!
//! Applications that need post-deletion access should retain the stored path
//! returned by cache entries, key listings, or queries.

use std::path::{Path, PathBuf};

use crate::error::LocalFileCacheError;

/// A database identity resolved without lossy path conversion.
#[derive(Debug)]
pub(crate) enum ResolvedPathKey {
    Existing { path: PathBuf, key: String },
    Missing { path: PathBuf, key: String },
}

impl ResolvedPathKey {
    pub(crate) fn path(&self) -> &Path {
        match self {
            Self::Existing { path, .. } | Self::Missing { path, .. } => path,
        }
    }

    pub(crate) fn key(&self) -> &str {
        match self {
            Self::Existing { key, .. } | Self::Missing { key, .. } => key,
        }
    }

    pub(crate) fn exists(&self) -> bool {
        matches!(self, Self::Existing { .. })
    }
}

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

/// Resolve an existing source to its canonical key or a missing source to the
/// caller's exact key. Both identities must be valid UTF-8.
pub(crate) fn resolve_path_key(path: &Path) -> Result<ResolvedPathKey, LocalFileCacheError> {
    match normalize_path(path) {
        Ok(canonical) => {
            let key = path_to_str(&canonical)?.to_owned();
            Ok(ResolvedPathKey::Existing {
                path: canonical,
                key,
            })
        }
        Err(LocalFileCacheError::FileNotFound { .. }) => {
            let key = path_to_str(path)?.to_owned();
            Ok(ResolvedPathKey::Missing {
                path: path.to_path_buf(),
                key,
            })
        }
        Err(error) => Err(error),
    }
}

/// Convert a path used as a SQLite `TEXT` identity without data loss.
pub(crate) fn path_to_str(path: &Path) -> Result<&str, LocalFileCacheError> {
    path.to_str()
        .ok_or_else(|| LocalFileCacheError::InvalidPath {
            path: path.to_path_buf(),
        })
}

#[cfg(test)]
#[path = "path/tests.rs"]
mod tests;
