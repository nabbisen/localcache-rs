//! Low-level database operations (repository layer).
//!
//! All SQL is confined to this module.

use std::time::UNIX_EPOCH;

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::detection::metadata::FileMetadata;
use crate::error::LocalFileCacheError;

// ---------------------------------------------------------------------------
// Row types
// ---------------------------------------------------------------------------

pub(crate) struct FileRow {
    pub id: i64,
    pub path: String,
    pub metadata: FileMetadata,
    pub updated_at: i64,
    pub payload_version: u32,
}

pub(crate) struct PayloadRow {
    pub content: Vec<u8>,
    pub encoding: String,
}

// ---------------------------------------------------------------------------
// Single-row queries
// ---------------------------------------------------------------------------

pub(crate) fn find_file(
    conn: &Connection,
    namespace: &str,
    path: &str,
) -> Result<Option<FileRow>, LocalFileCacheError> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, path, mtime, file_size, hash, updated_at, payload_version
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
                payload_version: r.get::<_, i64>(6)? as u32,
            })
        })
        .optional()?;
    Ok(row)
}

pub(crate) fn load_payload(
    conn: &Connection,
    file_id: i64,
) -> Result<Option<PayloadRow>, LocalFileCacheError> {
    let mut stmt =
        conn.prepare_cached("SELECT content, encoding FROM payloads WHERE file_id = ?1")?;
    let row = stmt
        .query_row(params![file_id], |r| {
            Ok(PayloadRow {
                content: r.get(0)?,
                encoding: r.get(1)?,
            })
        })
        .optional()?;
    Ok(row)
}

// ---------------------------------------------------------------------------
// Writes
// ---------------------------------------------------------------------------

pub(crate) fn upsert(
    conn: &Connection,
    namespace: &str,
    path: &str,
    metadata: &FileMetadata,
    payload_bytes: &[u8],
    encoding: &str,
    payload_version: u32,
) -> Result<(), LocalFileCacheError> {
    let tx = conn.unchecked_transaction()?;
    upsert_in_tx(
        &tx,
        namespace,
        path,
        metadata,
        payload_bytes,
        encoding,
        payload_version,
    )?;
    tx.commit()?;
    Ok(())
}

pub(crate) fn upsert_in_tx(
    tx: &Transaction,
    namespace: &str,
    path: &str,
    metadata: &FileMetadata,
    payload_bytes: &[u8],
    encoding: &str,
    payload_version: u32,
) -> Result<(), LocalFileCacheError> {
    let updated_at = now_secs();

    tx.execute(
        "INSERT INTO files
             (namespace, path, mtime, file_size, hash, updated_at, payload_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(namespace, path) DO UPDATE SET
             mtime           = excluded.mtime,
             file_size       = excluded.file_size,
             hash            = excluded.hash,
             updated_at      = excluded.updated_at,
             payload_version = excluded.payload_version",
        params![
            namespace,
            path,
            metadata.mtime,
            metadata.file_size as i64,
            metadata.hash,
            updated_at,
            payload_version as i64,
        ],
    )?;

    let file_id: i64 = tx.query_row(
        "SELECT id FROM files WHERE namespace = ?1 AND path = ?2",
        params![namespace, path],
        |r| r.get(0),
    )?;

    tx.execute(
        "INSERT INTO payloads (file_id, content, encoding)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(file_id) DO UPDATE SET
             content  = excluded.content,
             encoding = excluded.encoding",
        params![file_id, payload_bytes, encoding],
    )?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Deletes
// ---------------------------------------------------------------------------

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

/// Delete entries in `namespace` whose `payload_version != current_version`.
///
/// Returns the number of entries deleted.
pub(crate) fn delete_by_other_version(
    conn: &Connection,
    namespace: &str,
    current_version: u32,
) -> Result<usize, LocalFileCacheError> {
    let n = conn.execute(
        "DELETE FROM files WHERE namespace = ?1 AND payload_version != ?2",
        params![namespace, current_version as i64],
    )?;
    Ok(n)
}

/// Delete the `n` oldest entries (by `updated_at`) in `namespace`.
///
/// Returns the number of entries actually deleted.
pub(crate) fn delete_oldest_n(
    conn: &Connection,
    namespace: &str,
    n: usize,
) -> Result<usize, LocalFileCacheError> {
    let deleted = conn.execute(
        "DELETE FROM files
         WHERE namespace = ?1
           AND id IN (
               SELECT id FROM files
               WHERE namespace = ?1
               ORDER BY updated_at ASC
               LIMIT ?2
           )",
        params![namespace, n as i64],
    )?;
    Ok(deleted)
}

// ---------------------------------------------------------------------------
// Scans / aggregates
// ---------------------------------------------------------------------------

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

pub(crate) fn all_paths_in_namespace(
    conn: &Connection,
    namespace: &str,
) -> Result<Vec<String>, LocalFileCacheError> {
    let mut stmt = conn.prepare_cached("SELECT path FROM files WHERE namespace = ?1")?;
    let paths: Result<Vec<String>, _> = stmt.query_map(params![namespace], |r| r.get(0))?.collect();
    Ok(paths?)
}

/// Count the total number of entries in `namespace`.
pub(crate) fn count_in_namespace(
    conn: &Connection,
    namespace: &str,
) -> Result<usize, LocalFileCacheError> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM files WHERE namespace = ?1",
        params![namespace],
        |r| r.get(0),
    )?;
    Ok(n as usize)
}

/// Count entries in `namespace` grouped by `payload_version`.
///
/// Returns a list of `(version, count)` pairs sorted by version ascending.
pub(crate) fn count_by_version(
    conn: &Connection,
    namespace: &str,
) -> Result<Vec<(u32, usize)>, LocalFileCacheError> {
    let mut stmt = conn.prepare_cached(
        "SELECT payload_version, COUNT(*)
         FROM files
         WHERE namespace = ?1
         GROUP BY payload_version
         ORDER BY payload_version ASC",
    )?;
    let rows: Result<Vec<_>, _> = stmt
        .query_map(params![namespace], |r| {
            Ok((r.get::<_, i64>(0)? as u32, r.get::<_, i64>(1)? as usize))
        })?
        .collect();
    Ok(rows?)
}

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

pub(crate) fn now_secs() -> i64 {
    UNIX_EPOCH
        .elapsed()
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
