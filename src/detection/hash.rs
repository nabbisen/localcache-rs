//! File hash computation using BLAKE3.

use std::io::Read;
use std::path::Path;

use crate::error::LocalFileCacheError;

/// Compute a BLAKE3 hash of the entire file at `path`.
///
/// The file is read in chunks rather than loaded into memory all at once, which
/// keeps memory usage bounded even for very large files.
///
/// Returns the hash as a lowercase hex string.
pub(crate) fn compute_full_hash(path: &Path) -> Result<String, LocalFileCacheError> {
    const BUF_SIZE: usize = 64 * 1024; // 64 KiB chunks

    let mut file = std::fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; BUF_SIZE];

    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    Ok(hasher.finalize().to_hex().to_string())
}
