//! File hash computation using BLAKE3.
//!
//! ## Hash types
//!
//! Two kinds of hash are supported:
//!
//! | Kind    | Coverage                         | Storage prefix |
//! |---------|----------------------------------|----------------|
//! | Full    | Entire file                      | *(none)*       |
//! | Partial | First `SAMPLE_SIZE` bytes + last `SAMPLE_SIZE` bytes | `"partial:"` |
//!
//! The prefix scheme lets the detection layer distinguish hash types stored in
//! the database without a schema change.  Hashes written by v0.1/v0.2 (no
//! prefix) are treated as full hashes.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::error::LocalFileCacheError;

/// Prefix stored in the database to mark a partial hash.
pub(crate) const PARTIAL_PREFIX: &str = "partial:";

/// Number of bytes sampled from the head and tail of the file for a partial
/// hash.  64 KiB each → 128 KiB maximum I/O per check.
const SAMPLE_SIZE: u64 = 64 * 1024;

/// Streaming I/O buffer size used when reading the full file.
const BUF_SIZE: usize = 64 * 1024;

// ---------------------------------------------------------------------------
// Public helpers
// ---------------------------------------------------------------------------

/// Compute a BLAKE3 hash of the **entire** file at `path`.
///
/// The file is processed in 64 KiB chunks to keep memory usage bounded.
///
/// Returns a lowercase hex string (no prefix).
pub(crate) fn compute_full_hash(path: &Path) -> Result<String, LocalFileCacheError> {
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

/// Compute a **partial** BLAKE3 hash of `path`.
///
/// Reads at most [`SAMPLE_SIZE`] bytes from the beginning of the file and
/// [`SAMPLE_SIZE`] bytes from the end (with a one-byte separator so that
/// head-only and tail-only files produce different digests).
/// If the file is smaller than `2 * SAMPLE_SIZE` the entire content is hashed
/// (equivalent to a full hash, but still stored with the `"partial:"` prefix).
///
/// Returns a string of the form `"partial:<hex>"`.
pub(crate) fn compute_partial_hash(path: &Path) -> Result<String, LocalFileCacheError> {
    let mut file = std::fs::File::open(path)?;
    let file_len = file.metadata()?.len();

    let mut hasher = blake3::Hasher::new();

    if file_len <= SAMPLE_SIZE * 2 {
        // Small file: hash everything (same coverage as full hash).
        let mut buf = vec![0u8; BUF_SIZE];
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
    } else {
        // Large file: sample head + tail.
        let mut head = vec![0u8; SAMPLE_SIZE as usize];
        file.read_exact(&mut head)?;
        hasher.update(&head);

        // A single 0xFF separator byte prevents collisions between
        // files whose head equals another file's tail.
        hasher.update(&[0xFF]);

        file.seek(SeekFrom::End(-(SAMPLE_SIZE as i64)))?;
        let mut tail = vec![0u8; SAMPLE_SIZE as usize];
        file.read_exact(&mut tail)?;
        hasher.update(&tail);
    }

    Ok(format!("{}{}", PARTIAL_PREFIX, hasher.finalize().to_hex()))
}

// ---------------------------------------------------------------------------
// Helper: extract the raw hex from either hash format
// ---------------------------------------------------------------------------

/// Return `true` if `stored` is a partial hash string.
pub(crate) fn is_partial_hash(stored: &str) -> bool {
    stored.starts_with(PARTIAL_PREFIX)
}
