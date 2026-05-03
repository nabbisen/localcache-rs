//! Low-level database operations (the repository layer).
//!
//! All SQL statements are confined to this module.  Higher layers work with
//! Rust types only and never construct raw SQL.

use std::time::UNIX_EPOCH;

use rusqlite::{Connection, OptionalExtension, params};

use crate::detection::metadata::FileMetadata;
use crate::error::LocalFileCacheError;

// ---------------------------------------------------------------------------
// Row types (internal)
// ---------------------------------------------------------------------------

/// A row from the `files` table combined with its payload from `payloads`.
pub(crate) struct FileRow {
    pub id: i64,
    pub path: String,
    pub metadata: FileMetadata,
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

/// Look up the `files` row for `path` (no payload loaded).
pub(crate) fn find_file(
    conn: &Connection,
    path: &str,
) -> Result<Option<FileRow>, LocalFileCacheError> {
    let mut stmt =
        conn.prepare_cached("SELECT id, path, mtime, file_size, hash FROM files WHERE path = ?1")?;
    let row = stmt
        .query_row(params![path], |r| {
            Ok(FileRow {
                id: r.get(0)?,
                path: r.get(1)?,
                metadata: FileMetadata {
                    mtime: r.get(2)?,
                    file_size: r.get::<_, i64>(3)? as u64,
                    hash: r.get(4)?,
                },
            })
        })
        .optional()?;
    Ok(row)
}

/// Load the raw payload bytes for `file_id`.
pub(crate) fn load_payload(
    conn: &Connection,
    file_id: i64,
) -> Result<Option<Vec<u8>>, LocalFileCacheError> {
    let mut stmt = conn.prepare_cached("SELECT content FROM payloads WHERE file_id = ?1")?;
    let bytes = stmt
        .query_row(params![file_id], |r| r.get::<_, Vec<u8>>(0))
        .optional()?;
    Ok(bytes)
}

/// Upsert a `files` row and its corresponding `payloads` row inside a single
/// transaction.
///
/// `updated_at` is set to the current Unix timestamp automatically.
pub(crate) fn upsert(
    conn: &Connection,
    path: &str,
    metadata: &FileMetadata,
    payload_bytes: &[u8],
) -> Result<(), LocalFileCacheError> {
    let updated_at = UNIX_EPOCH
        .elapsed()
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let tx = conn.unchecked_transaction()?;

    tx.execute(
        "INSERT INTO files (path, mtime, file_size, hash, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(path) DO UPDATE SET
             mtime      = excluded.mtime,
             file_size  = excluded.file_size,
             hash       = excluded.hash,
             updated_at = excluded.updated_at",
        params![
            path,
            metadata.mtime,
            metadata.file_size as i64,
            metadata.hash,
            updated_at,
        ],
    )?;

    // Retrieve the id that was just inserted or already existed.
    let file_id: i64 =
        tx.query_row("SELECT id FROM files WHERE path = ?1", params![path], |r| {
            r.get(0)
        })?;

    tx.execute(
        "INSERT INTO payloads (file_id, content)
         VALUES (?1, ?2)
         ON CONFLICT(file_id) DO UPDATE SET content = excluded.content",
        params![file_id, payload_bytes],
    )?;

    tx.commit()?;
    Ok(())
}

/// Delete the `files` row for `path` (payload is removed via CASCADE).
///
/// Returns `true` if a row was deleted.
pub(crate) fn delete_by_path(conn: &Connection, path: &str) -> Result<bool, LocalFileCacheError> {
    let n = conn.execute("DELETE FROM files WHERE path = ?1", params![path])?;
    Ok(n > 0)
}

/// Return all paths stored in the `files` table.
pub(crate) fn all_paths(conn: &Connection) -> Result<Vec<String>, LocalFileCacheError> {
    let mut stmt = conn.prepare_cached("SELECT path FROM files")?;
    let paths: Result<Vec<String>, _> = stmt.query_map([], |r| r.get(0))?.collect();
    Ok(paths?)
}

/// Delete the `files` row for `path` by its stored path string.
///
/// This is a thin wrapper around [`delete_by_path`] kept for clarity at the
/// call site when working with raw path strings.
pub(crate) fn delete_path(conn: &Connection, path: &str) -> Result<(), LocalFileCacheError> {
    conn.execute("DELETE FROM files WHERE path = ?1", params![path])?;
    Ok(())
}
