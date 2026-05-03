//! Low-level database operations (repository layer).
//!
//! All SQL is confined to this module.  Higher layers work with Rust types only.

use std::time::UNIX_EPOCH;

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::detection::metadata::FileMetadata;
use crate::error::LocalFileCacheError;

// ---------------------------------------------------------------------------
// Row types (crate-internal)
// ---------------------------------------------------------------------------

/// A row from the `files` table (payload not included).
pub(crate) struct FileRow {
    pub id: i64,
    pub path: String,
    pub metadata: FileMetadata,
    /// Unix timestamp recorded when this entry was last written.
    pub updated_at: i64,
}

// ---------------------------------------------------------------------------
// Single-row queries
// ---------------------------------------------------------------------------

/// Look up the `files` row for `(namespace, path)`.  Payload is not loaded.
pub(crate) fn find_file(
    conn: &Connection,
    namespace: &str,
    path: &str,
) -> Result<Option<FileRow>, LocalFileCacheError> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, path, mtime, file_size, hash, updated_at
         FROM files
         WHERE namespace = ?1 AND path = ?2",
    )?;
    let row = stmt
        .query_row(params![namespace, path], |r| {
            Ok(FileRow {
                id: r.get(0)?,
                path: r.get(1)?,
                metadata: FileMetadata {
                    mtime: r.get(2)?,
                    file_size: r.get::<_, i64>(3)? as u64,
                    hash: r.get(4)?,
                },
                updated_at: r.get(5)?,
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

// ---------------------------------------------------------------------------
// Writes
// ---------------------------------------------------------------------------

/// Upsert a `files` row and its `payloads` row inside a single transaction.
pub(crate) fn upsert(
    conn: &Connection,
    namespace: &str,
    path: &str,
    metadata: &FileMetadata,
    payload_bytes: &[u8],
) -> Result<(), LocalFileCacheError> {
    let tx = conn.unchecked_transaction()?;
    upsert_in_tx(&tx, namespace, path, metadata, payload_bytes)?;
    tx.commit()?;
    Ok(())
}

/// Upsert inside a caller-supplied transaction (used by batch operations).
pub(crate) fn upsert_in_tx(
    tx: &Transaction,
    namespace: &str,
    path: &str,
    metadata: &FileMetadata,
    payload_bytes: &[u8],
) -> Result<(), LocalFileCacheError> {
    let updated_at = now_secs();

    tx.execute(
        "INSERT INTO files (namespace, path, mtime, file_size, hash, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(namespace, path) DO UPDATE SET
             mtime      = excluded.mtime,
             file_size  = excluded.file_size,
             hash       = excluded.hash,
             updated_at = excluded.updated_at",
        params![
            namespace,
            path,
            metadata.mtime,
            metadata.file_size as i64,
            metadata.hash,
            updated_at,
        ],
    )?;

    let file_id: i64 = tx.query_row(
        "SELECT id FROM files WHERE namespace = ?1 AND path = ?2",
        params![namespace, path],
        |r| r.get(0),
    )?;

    tx.execute(
        "INSERT INTO payloads (file_id, content)
         VALUES (?1, ?2)
         ON CONFLICT(file_id) DO UPDATE SET content = excluded.content",
        params![file_id, payload_bytes],
    )?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Deletes
// ---------------------------------------------------------------------------

/// Delete the row for `(namespace, path)`.  Returns `true` if a row was deleted.
pub(crate) fn delete_by_path(
    conn: &Connection,
    namespace: &str,
    path: &str,
) -> Result<bool, LocalFileCacheError> {
    let n = conn.execute(
        "DELETE FROM files WHERE namespace = ?1 AND path = ?2",
        params![namespace, path],
    )?;
    Ok(n > 0)
}

/// Delete a row by its stored path string within `namespace`.
pub(crate) fn delete_path(
    conn: &Connection,
    namespace: &str,
    path: &str,
) -> Result<(), LocalFileCacheError> {
    conn.execute(
        "DELETE FROM files WHERE namespace = ?1 AND path = ?2",
        params![namespace, path],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Scans
// ---------------------------------------------------------------------------

/// Return all `(id, path, updated_at)` triples stored in `namespace`.
pub(crate) fn all_file_rows_in_namespace(
    conn: &Connection,
    namespace: &str,
) -> Result<Vec<(i64, String, i64)>, LocalFileCacheError> {
    let mut stmt =
        conn.prepare_cached("SELECT id, path, updated_at FROM files WHERE namespace = ?1")?;
    let rows: Result<Vec<_>, _> = stmt
        .query_map(params![namespace], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?
        .collect();
    Ok(rows?)
}

/// Return all paths in `namespace` (lightweight version of the above).
pub(crate) fn all_paths_in_namespace(
    conn: &Connection,
    namespace: &str,
) -> Result<Vec<String>, LocalFileCacheError> {
    let mut stmt = conn.prepare_cached("SELECT path FROM files WHERE namespace = ?1")?;
    let paths: Result<Vec<String>, _> = stmt.query_map(params![namespace], |r| r.get(0))?.collect();
    Ok(paths?)
}

// ---------------------------------------------------------------------------
// TTL helpers
// ---------------------------------------------------------------------------

/// Return the current Unix timestamp in seconds.
pub(crate) fn now_secs() -> i64 {
    UNIX_EPOCH
        .elapsed()
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
