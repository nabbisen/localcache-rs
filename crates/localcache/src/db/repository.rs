//! Low-level database operations (repository layer).

use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::cache::entry::EntryInfo;
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
    #[allow(dead_code)]
    pub last_accessed_at: i64,
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
        "SELECT id, path, mtime, file_size, hash, updated_at, payload_version, last_accessed_at
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
                last_accessed_at: r.get::<_, i64>(7)?,
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

/// Update `last_accessed_at` for a file row, recording the current time.
///
/// This is called after every successful `get` / `get_if_fresh` read so that
/// LRU eviction has accurate access-time data.
pub(crate) fn touch_last_accessed(
    conn: &Connection,
    file_id: i64,
) -> Result<(), LocalFileCacheError> {
    let now = now_secs();
    conn.execute(
        "UPDATE files SET last_accessed_at = ?1 WHERE id = ?2",
        params![now, file_id],
    )?;
    Ok(())
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
             (namespace, path, mtime, file_size, hash, updated_at, payload_version,
              last_accessed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(namespace, path) DO UPDATE SET
             mtime            = excluded.mtime,
             file_size        = excluded.file_size,
             hash             = excluded.hash,
             updated_at       = excluded.updated_at,
             payload_version  = excluded.payload_version",
        params![
            namespace,
            path,
            metadata.mtime,
            metadata.file_size as i64,
            metadata.hash,
            updated_at,
            payload_version as i64,
            0i64, // last_accessed_at reset to 0 on write (entry is "fresh from write")
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

/// Delete the `n` **least recently accessed** entries in `namespace`.
///
/// Entries with `last_accessed_at = 0` (never read since last write) are
/// evicted first, then by ascending `last_accessed_at`, using `updated_at`
/// as a tiebreaker.
pub(crate) fn delete_lru_n(
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
               ORDER BY last_accessed_at ASC, updated_at ASC
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

/// Return lightweight metadata for all entries in `namespace`, joined with
/// their encoding from `payloads`.  Does **not** load payload content.
pub(crate) fn list_entries(
    conn: &Connection,
    namespace: &str,
) -> Result<Vec<EntryInfo>, LocalFileCacheError> {
    let mut stmt = conn.prepare_cached(
        "SELECT f.path, f.mtime, f.file_size, f.hash,
                f.updated_at, f.payload_version, f.last_accessed_at,
                p.encoding
         FROM files f
         JOIN payloads p ON p.file_id = f.id
         WHERE f.namespace = ?1
         ORDER BY f.updated_at DESC",
    )?;
    let rows: Result<Vec<EntryInfo>, _> = stmt
        .query_map(params![namespace], |r| {
            Ok(EntryInfo {
                path: PathBuf::from(r.get::<_, String>(0)?),
                metadata: FileMetadata {
                    mtime: r.get(1)?,
                    file_size: r.get::<_, i64>(2)? as u64,
                    hash: r.get(3)?,
                },
                updated_at: r.get(4)?,
                payload_version: r.get::<_, i64>(5)? as u32,
                last_accessed_at: r.get::<_, i64>(6)?,
                encoding: r.get(7)?,
            })
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

// ---------------------------------------------------------------------------
// Cache statistics
// ---------------------------------------------------------------------------

/// Aggregate statistics for `namespace`.
pub(crate) struct RawStats {
    pub total_entries: usize,
    pub total_payload_bytes: u64,
    pub oldest_updated_at: Option<i64>,
    pub newest_updated_at: Option<i64>,
}

pub(crate) fn aggregate_stats(
    conn: &Connection,
    namespace: &str,
) -> Result<RawStats, LocalFileCacheError> {
    let row = conn.query_row(
        "SELECT COUNT(*),
                COALESCE(SUM(LENGTH(p.content)), 0),
                MIN(f.updated_at),
                MAX(f.updated_at)
         FROM files f
         JOIN payloads p ON p.file_id = f.id
         WHERE f.namespace = ?1",
        params![namespace],
        |r| {
            Ok(RawStats {
                total_entries: r.get::<_, i64>(0)? as usize,
                total_payload_bytes: r.get::<_, i64>(1)? as u64,
                oldest_updated_at: r.get::<_, Option<i64>>(2)?,
                newest_updated_at: r.get::<_, Option<i64>>(3)?,
            })
        },
    )?;
    Ok(row)
}

pub(crate) fn encoding_breakdown(
    conn: &Connection,
    namespace: &str,
) -> Result<Vec<(String, usize)>, LocalFileCacheError> {
    let mut stmt = conn.prepare_cached(
        "SELECT p.encoding, COUNT(*)
         FROM files f
         JOIN payloads p ON p.file_id = f.id
         WHERE f.namespace = ?1
         GROUP BY p.encoding
         ORDER BY p.encoding ASC",
    )?;
    let rows: Result<Vec<_>, _> = stmt
        .query_map(params![namespace], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize))
        })?
        .collect();
    Ok(rows?)
}

// ---------------------------------------------------------------------------
// Key rotation support
// ---------------------------------------------------------------------------

/// A row from `payloads` needed for re-encryption.
#[cfg(feature = "encryption")]
pub(crate) struct EncryptedPayloadRow {
    pub file_id: i64,
    pub content: Vec<u8>,
    #[allow(dead_code)]
    pub encoding: String,
}

/// Load all payload rows in `namespace` whose encoding ends with `-aes256gcm`.
#[cfg(feature = "encryption")]
pub(crate) fn load_encrypted_payloads(
    conn: &Connection,
    namespace: &str,
) -> Result<Vec<EncryptedPayloadRow>, LocalFileCacheError> {
    let mut stmt = conn.prepare_cached(
        "SELECT p.file_id, p.content, p.encoding
         FROM payloads p
         JOIN files f ON f.id = p.file_id
         WHERE f.namespace = ?1
           AND p.encoding LIKE '%-aes256gcm'",
    )?;
    let rows: Result<Vec<_>, _> = stmt
        .query_map(params![namespace], |r| {
            Ok(EncryptedPayloadRow {
                file_id: r.get(0)?,
                content: r.get(1)?,
                encoding: r.get(2)?,
            })
        })?
        .collect();
    Ok(rows?)
}

/// Update a payload row with new content (used by key rotation).
#[cfg(feature = "encryption")]
pub(crate) fn update_payload_content(
    tx: &Transaction,
    file_id: i64,
    new_content: &[u8],
) -> Result<(), LocalFileCacheError> {
    tx.execute(
        "UPDATE payloads SET content = ?1 WHERE file_id = ?2",
        params![new_content, file_id],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// on_evict support
// ---------------------------------------------------------------------------

/// Return the paths of the `n` least recently accessed entries in `namespace`
/// **without** deleting them.  Used to call `on_evict` callbacks before
/// the actual deletion.
pub(crate) fn list_lru_n_paths(
    conn: &Connection,
    namespace: &str,
    n: usize,
) -> Result<Vec<std::path::PathBuf>, LocalFileCacheError> {
    let mut stmt = conn.prepare_cached(
        "SELECT path FROM files
         WHERE namespace = ?1
         ORDER BY last_accessed_at ASC, updated_at ASC
         LIMIT ?2",
    )?;
    let paths: Result<Vec<std::path::PathBuf>, _> = stmt
        .query_map(params![namespace, n as i64], |r| {
            Ok(std::path::PathBuf::from(r.get::<_, String>(0)?))
        })?
        .collect();
    Ok(paths?)
}

// ---------------------------------------------------------------------------
// Export / import support
// ---------------------------------------------------------------------------

/// A raw database row used for export — includes payload content.
pub(crate) struct FullRow {
    pub path: String,
    pub content: Vec<u8>,
    pub encoding: String,
    pub mtime: i64,
    pub file_size: u64,
    pub hash: Option<String>,
    pub payload_version: u32,
    pub updated_at: i64,
    pub last_accessed_at: i64,
}

/// Load every entry in `namespace` including its payload bytes.
pub(crate) fn load_all_full(
    conn: &Connection,
    namespace: &str,
) -> Result<Vec<FullRow>, LocalFileCacheError> {
    let mut stmt = conn.prepare_cached(
        "SELECT f.path, p.content, p.encoding,
                f.mtime, f.file_size, f.hash,
                f.payload_version, f.updated_at, f.last_accessed_at
         FROM files f
         JOIN payloads p ON p.file_id = f.id
         WHERE f.namespace = ?1
         ORDER BY f.updated_at DESC",
    )?;
    let rows: Result<Vec<_>, _> = stmt
        .query_map(params![namespace], |r| {
            Ok(FullRow {
                path: r.get(0)?,
                content: r.get(1)?,
                encoding: r.get(2)?,
                mtime: r.get(3)?,
                file_size: r.get::<_, i64>(4)? as u64,
                hash: r.get(5)?,
                payload_version: r.get::<_, i64>(6)? as u32,
                updated_at: r.get(7)?,
                last_accessed_at: r.get(8)?,
            })
        })?
        .collect();
    Ok(rows?)
}

/// Import a batch of rows into `namespace` inside a single transaction.
///
/// Existing entries (matched on `namespace + path`) are replaced.
pub(crate) fn import_rows(
    conn: &Connection,
    namespace: &str,
    rows: &[FullRow],
) -> Result<usize, LocalFileCacheError> {
    if rows.is_empty() {
        return Ok(0);
    }
    let tx = conn.unchecked_transaction()?;
    for row in rows {
        tx.execute(
            "INSERT INTO files
                 (namespace, path, mtime, file_size, hash, updated_at,
                  payload_version, last_accessed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(namespace, path) DO UPDATE SET
                 mtime            = excluded.mtime,
                 file_size        = excluded.file_size,
                 hash             = excluded.hash,
                 updated_at       = excluded.updated_at,
                 payload_version  = excluded.payload_version,
                 last_accessed_at = excluded.last_accessed_at",
            params![
                namespace,
                row.path,
                row.mtime,
                row.file_size as i64,
                row.hash,
                row.updated_at,
                row.payload_version as i64,
                row.last_accessed_at,
            ],
        )?;

        let file_id: i64 = tx.query_row(
            "SELECT id FROM files WHERE namespace = ?1 AND path = ?2",
            params![namespace, row.path],
            |r| r.get(0),
        )?;

        tx.execute(
            "INSERT INTO payloads (file_id, content, encoding)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(file_id) DO UPDATE SET
                 content  = excluded.content,
                 encoding = excluded.encoding",
            params![file_id, row.content, row.encoding],
        )?;
    }
    let n = rows.len();
    tx.commit()?;
    Ok(n)
}

// ---------------------------------------------------------------------------
// Lightweight key / existence helpers
// ---------------------------------------------------------------------------

/// Return `true` if a row exists for `(namespace, path)`.
pub(crate) fn exists(
    conn: &Connection,
    namespace: &str,
    path: &str,
) -> Result<bool, LocalFileCacheError> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM files WHERE namespace = ?1 AND path = ?2",
        params![namespace, path],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// Return all stored paths in `namespace`, optionally filtered by a SQL
/// `LIKE` pattern on the `path` column.
///
/// `pattern` uses standard SQLite LIKE semantics (`%` = any sequence,
/// `_` = one character).  Pass `None` to return all paths.
pub(crate) fn keys(
    conn: &Connection,
    namespace: &str,
    pattern: Option<&str>,
    index_hint: Option<&str>,
    path_in_dir: Option<(&str, bool)>,
    path_glob: Option<&[String]>,
) -> Result<Vec<std::path::PathBuf>, LocalFileCacheError> {
    let (sql, params_vec) = build_path_sql(namespace, pattern, index_hint, path_in_dir, path_glob);
    let mut stmt = conn.prepare(&sql)?;
    let paths: Result<Vec<_>, _> = stmt
        .query_map(
            rusqlite::params_from_iter(params_vec.iter().map(String::as_str)),
            |r| Ok(std::path::PathBuf::from(r.get::<_, String>(0)?)),
        )?
        .collect();
    Ok(paths?)
}

/// Run `EXPLAIN QUERY PLAN <sql>` and return the human-readable plan as a
/// newline-joined string, one detail line per step.
pub(crate) fn explain_query(
    conn: &Connection,
    namespace: &str,
    pattern: Option<&str>,
    index_hint: Option<&str>,
    path_in_dir: Option<(&str, bool)>,
    path_glob: Option<&[String]>,
) -> Result<String, LocalFileCacheError> {
    let (sql, params_vec) = build_path_sql(namespace, pattern, index_hint, path_in_dir, path_glob);
    let explain_sql = format!("EXPLAIN QUERY PLAN {sql}");
    let mut stmt = conn.prepare(&explain_sql)?;
    let rows: Result<Vec<String>, _> = stmt
        .query_map(
            rusqlite::params_from_iter(params_vec.iter().map(String::as_str)),
            |row| row.get::<_, String>(3),
        )?
        .collect();
    Ok(rows?.join("\n"))
}

// ---------------------------------------------------------------------------
// Path-listing SQL builder (shared by keys, explain_query)
// ---------------------------------------------------------------------------

/// Build the `SELECT path FROM files …` SQL and its bind parameters for all
/// path-filtering options.  All filters AND-combine.
///
/// `path_in_dir`  — `(prefix, recursive)` where `prefix` is the canonical
///                  directory path including a trailing platform separator.
/// `path_glob`    — pre-expanded, `[`-escaped SQLite GLOB alternatives.
fn build_path_sql(
    namespace: &str,
    pattern: Option<&str>,
    index_hint: Option<&str>,
    path_in_dir: Option<(&str, bool)>,
    path_glob: Option<&[String]>,
) -> (String, Vec<String>) {
    let table = match index_hint {
        Some(idx) => format!("files INDEXED BY {idx}"),
        None => "files".to_owned(),
    };

    let mut clauses: Vec<String> = vec!["namespace = ?".to_owned()];
    let mut params: Vec<String> = vec![namespace.to_owned()];

    // path_like — SQL LIKE with no ESCAPE (caller controls metacharacters).
    if let Some(pat) = pattern {
        clauses.push("path LIKE ? ESCAPE '\\'".to_owned());
        params.push(pat.to_owned());
    }

    // path_in_dir — exact prefix LIKE, optionally excluding sub-subdirectories.
    if let Some((prefix, recursive)) = path_in_dir {
        let escaped = escape_like(prefix);
        // Recursive: all paths that start with the directory prefix.
        clauses.push("path LIKE ? ESCAPE '\\'".to_owned());
        params.push(format!("{escaped}%"));
        if !recursive {
            // Non-recursive: exclude paths that contain another separator
            // after the prefix (i.e. paths deeper than one level).
            let sep_esc = escape_like(std::path::MAIN_SEPARATOR_STR);
            clauses.push("path NOT LIKE ? ESCAPE '\\'".to_owned());
            params.push(format!("{escaped}%{sep_esc}%"));
        }
    }

    // path_glob — one SQLite GLOB term per brace-expanded alternative, OR-combined.
    if let Some(globs) = path_glob {
        if !globs.is_empty() {
            let terms: Vec<String> = globs.iter().map(|_| "path GLOB ?".to_owned()).collect();
            clauses.push(format!("({})", terms.join(" OR ")));
            params.extend(globs.iter().cloned());
        }
    }

    let sql = format!(
        "SELECT path FROM {table} WHERE {} ORDER BY path",
        clauses.join(" AND ")
    );
    (sql, params)
}

/// Escape characters that are special in a SQL `LIKE` expression when using
/// backslash as the `ESCAPE` character: `\`, `%`, `_`.
///
/// The result is safe to embed in a `LIKE` pattern where literal prefix/suffix
/// characters must not act as wildcards.
fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '%' => out.push_str("\\%"),
            '_' => out.push_str("\\_"),
            c => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Namespace management
// ---------------------------------------------------------------------------

/// Return all distinct namespace names in the database, sorted.
pub(crate) fn list_namespaces(conn: &Connection) -> Result<Vec<String>, LocalFileCacheError> {
    let mut stmt =
        conn.prepare_cached("SELECT DISTINCT namespace FROM files ORDER BY namespace")?;
    let ns: Result<Vec<String>, _> = stmt.query_map([], |r| r.get(0))?.collect();
    Ok(ns?)
}
