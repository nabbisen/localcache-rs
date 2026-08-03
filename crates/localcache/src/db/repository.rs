//! Low-level database operations (repository layer).

use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::cache::entry::EntryInfo;
use crate::db::indexes::{self, QuotedIdentifier};
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

/// RFC 021 pass 1: a candidate row with everything the query comparator
/// reads and **no payload content**. Same predicates and index-hint
/// handling as [`keys`].
pub(crate) struct CandidateRow {
    pub id: i64,
    pub path: String,
    pub mtime: i64,
    pub file_size: u64,
    pub hash: Option<String>,
    pub last_accessed_at: i64,
}

/// RFC 021 tier-3 fallback: one row per path-filtered candidate, joined
/// with its payload (if any). `content`/`encoding` are `None` when the file
/// row has no payload row, mirroring today's `load_payload -> None` skip.
#[cfg(feature = "json")]
pub(crate) struct FullCandidateRow {
    pub path: String,
    pub mtime: i64,
    pub file_size: u64,
    pub hash: Option<String>,
    pub last_accessed_at: i64,
    pub content: Option<Vec<u8>>,
    pub encoding: Option<String>,
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
         FROM main.files
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
        conn.prepare_cached("SELECT content, encoding FROM main.payloads WHERE file_id = ?1")?;
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

/// RFC 020: delete every path in `paths` from `namespace`, inside `tx`,
/// preparing the statement once via `prepare_cached` and executing it per
/// path. Returns the number of rows actually affected (not the number
/// attempted). Caller commits `tx`.
pub(crate) fn delete_paths_in_tx(
    tx: &Transaction,
    namespace: &str,
    paths: &[String],
) -> Result<usize, LocalFileCacheError> {
    let mut stmt = tx.prepare_cached("DELETE FROM files WHERE namespace = ?1 AND path = ?2")?;
    let mut removed = 0usize;
    for path in paths {
        removed += stmt.execute(params![namespace, path])?;
    }
    Ok(removed)
}

/// RFC 020: `delete_paths_in_tx` wrapped in one committed transaction, so a
/// page of deletes costs one commit instead of one per row. Follows the
/// `upsert`/`upsert_in_tx` split already established in this file.
pub(crate) fn delete_paths(
    conn: &Connection,
    namespace: &str,
    paths: &[String],
) -> Result<usize, LocalFileCacheError> {
    let tx = conn.unchecked_transaction()?;
    let removed = delete_paths_in_tx(&tx, namespace, paths)?;
    tx.commit()?;
    Ok(removed)
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

/// RFC 020: one page of paths in `namespace`, ordered by `path`, strictly
/// after `after`. Pass `""` for the first page — every stored path is a
/// non-empty absolute path, so `path > ''` selects all of them. Served by
/// the existing `idx_files_namespace_path`; no new index.
pub(crate) fn paths_page_in_namespace(
    conn: &Connection,
    namespace: &str,
    after: &str,
    limit: usize,
) -> Result<Vec<String>, LocalFileCacheError> {
    let mut stmt = conn.prepare_cached(
        "SELECT path FROM files
         WHERE namespace = ?1 AND path > ?2
         ORDER BY path
         LIMIT ?3",
    )?;
    let paths: Result<Vec<String>, _> = stmt
        .query_map(params![namespace, after, limit as i64], |r| r.get(0))?
        .collect();
    Ok(paths?)
}

/// RFC 020: same page shape as [`paths_page_in_namespace`], carrying
/// `updated_at` for `cleanup_expired`.
pub(crate) fn path_rows_page_in_namespace(
    conn: &Connection,
    namespace: &str,
    after: &str,
    limit: usize,
) -> Result<Vec<(String, i64)>, LocalFileCacheError> {
    let mut stmt = conn.prepare_cached(
        "SELECT path, updated_at FROM files
         WHERE namespace = ?1 AND path > ?2
         ORDER BY path
         LIMIT ?3",
    )?;
    let rows: Result<Vec<_>, _> = stmt
        .query_map(params![namespace, after, limit as i64], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?
        .collect();
    Ok(rows?)
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
    let transaction =
        rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Deferred)?;
    let authorized = indexes::authorize_query_index_in_snapshot(&transaction, index_hint)?;
    let (sql, params_vec) = build_path_sql(
        namespace,
        pattern,
        authorized.as_ref(),
        path_in_dir,
        path_glob,
    );
    let mut stmt = transaction.prepare(&sql)?;
    let paths: Result<Vec<_>, _> = stmt
        .query_map(
            rusqlite::params_from_iter(params_vec.iter().map(String::as_str)),
            |r| Ok(std::path::PathBuf::from(r.get::<_, String>(0)?)),
        )?
        .collect();
    let paths = paths?;
    drop(stmt);
    transaction.commit()?;
    Ok(paths)
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
    let transaction =
        rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Deferred)?;
    let authorized = indexes::authorize_query_index_in_snapshot(&transaction, index_hint)?;
    let (sql, params_vec) = build_path_sql(
        namespace,
        pattern,
        authorized.as_ref(),
        path_in_dir,
        path_glob,
    );
    let explain_sql = format!("EXPLAIN QUERY PLAN {sql}");
    let mut stmt = transaction.prepare(&explain_sql)?;
    let rows: Result<Vec<String>, _> = stmt
        .query_map(
            rusqlite::params_from_iter(params_vec.iter().map(String::as_str)),
            |row| row.get::<_, String>(3),
        )?
        .collect();
    let plan = rows?.join("\n");
    drop(stmt);
    transaction.commit()?;
    Ok(plan)
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
    index_hint: Option<&QuotedIdentifier>,
    path_in_dir: Option<(&str, bool)>,
    path_glob: Option<&[String]>,
) -> (String, Vec<String>) {
    let table = match index_hint {
        Some(idx) => format!("main.files INDEXED BY {}", idx.as_sql()),
        None => "main.files".to_owned(),
    };
    let (clauses, params) = path_filter_clauses(namespace, pattern, path_in_dir, path_glob);
    let sql = format!(
        "SELECT path FROM {table} WHERE {} ORDER BY path",
        clauses.join(" AND ")
    );
    (sql, params)
}

/// Shared `WHERE`-clause and bind-parameter builder for the path-filtering
/// options, factored out of [`build_path_sql`] so RFC 021's candidate
/// queries (which need a different `SELECT`/`FROM` shape) can reuse the
/// exact same filtering semantics without duplicating it. All filters
/// AND-combine. Column names are left unqualified: every caller either
/// selects from `files` alone, or joins `payloads`, which has no
/// `namespace`/`path` columns to collide with.
///
/// `path_in_dir`  — `(prefix, recursive)` where `prefix` is the canonical
///                  directory path including a trailing platform separator.
/// `path_glob`    — pre-expanded, `[`-escaped SQLite GLOB alternatives.
fn path_filter_clauses(
    namespace: &str,
    pattern: Option<&str>,
    path_in_dir: Option<(&str, bool)>,
    path_glob: Option<&[String]>,
) -> (Vec<String>, Vec<String>) {
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

    (clauses, params)
}

// ---------------------------------------------------------------------------
// RFC 021: one-pass, late-materialization candidate queries
// ---------------------------------------------------------------------------

/// RFC 021 pass 1, tier 1: candidate rows for the path-filtered set, in
/// `ORDER BY path`, **without** payload content. Used when the query has no
/// field predicate and no field-based sort, so payload content is not
/// needed until the survivors of `offset`/`limit` are known.
pub(crate) fn query_candidates(
    conn: &Connection,
    namespace: &str,
    pattern: Option<&str>,
    index_hint: Option<&str>,
    path_in_dir: Option<(&str, bool)>,
    path_glob: Option<&[String]>,
) -> Result<Vec<CandidateRow>, LocalFileCacheError> {
    let transaction =
        rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Deferred)?;
    let authorized = indexes::authorize_query_index_in_snapshot(&transaction, index_hint)?;
    let table = match authorized.as_ref() {
        Some(idx) => format!("main.files INDEXED BY {}", idx.as_sql()),
        None => "main.files".to_owned(),
    };
    let (clauses, params) = path_filter_clauses(namespace, pattern, path_in_dir, path_glob);
    let sql = format!(
        "SELECT id, path, mtime, file_size, hash, last_accessed_at FROM {table} \
         WHERE {} ORDER BY path",
        clauses.join(" AND ")
    );
    let mut stmt = transaction.prepare(&sql)?;
    let rows: Result<Vec<_>, _> = stmt
        .query_map(
            rusqlite::params_from_iter(params.iter().map(String::as_str)),
            |r| {
                Ok(CandidateRow {
                    id: r.get(0)?,
                    path: r.get(1)?,
                    mtime: r.get(2)?,
                    file_size: r.get::<_, i64>(3)? as u64,
                    hash: r.get(4)?,
                    last_accessed_at: r.get::<_, i64>(5)?,
                })
            },
        )?
        .collect();
    let rows = rows?;
    drop(stmt);
    transaction.commit()?;
    Ok(rows)
}

/// One tier-2 candidate row plus its per-`select_fields` extracted values,
/// in the same order as `select_fields`.
#[cfg(feature = "json")]
pub(crate) type JsonPushdownRow = (CandidateRow, Vec<Option<f64>>);

/// RFC 021 pass 1, tier 2: like [`query_candidates`], plus one extracted
/// `json_extract` column per entry of `select_fields`, and one pushed-down
/// numeric predicate per entry of `where_fields`. Only correct when every
/// payload in `namespace` is `encoding = 'json'` — callers must check
/// [`namespace_all_json`] first; this function does not check it.
///
/// Each extracted column is `NULL` unless the JSON value at that path is a
/// number, matching `Value::as_f64()` exactly (a JSON string, bool, null,
/// array, or object all become `None` there; SQLite's `json_type` is used
/// here to draw the identical line, since a raw numeric comparison against
/// a JSON string would otherwise pass under SQLite's `TEXT > REAL` type
/// ordering — a divergence, not a translation).
///
/// Extracts each **distinct** field path — the union of `select_fields` and
/// every `where_fields` path — exactly once, in an inner subquery, and
/// applies the pushed-down predicates in the outer `WHERE` against the
/// already-extracted columns. The headline case this RFC targets
/// (`field_gt("score", …).order_by_field("score", …)`) names the same path
/// from both sides; extracting it twice would call `json_extract` — which
/// re-parses the whole stored JSON text on every call, since SQLite's json1
/// functions are not streaming — twice per row scanned for no reason. The
/// outer filter needs no repeated `json_type` guard either: the inner
/// `CASE` already maps every non-numeric value to `NULL`, and `NULL > x` /
/// `NULL < x` are both false in SQL, which is exactly `unwrap_or(false)`.
///
/// `field_path`s are trusted to have already been validated by the caller
/// (see `query::is_safe_json_field_path`): no `'`, `"`, `[`, `]`, `$`, or
/// empty segment, so `format!("$.{field_path}")` is a well-formed SQLite
/// JSON path. The path string itself is always bound as a parameter, never
/// interpolated into the SQL text.
#[cfg(feature = "json")]
#[allow(clippy::too_many_arguments)]
pub(crate) fn query_candidates_json_pushdown(
    conn: &Connection,
    namespace: &str,
    pattern: Option<&str>,
    index_hint: Option<&str>,
    path_in_dir: Option<(&str, bool)>,
    path_glob: Option<&[String]>,
    select_fields: &[String],
    where_fields: &[(String, &'static str, f64)],
) -> Result<Vec<JsonPushdownRow>, LocalFileCacheError> {
    use rusqlite::types::Value;

    let transaction =
        rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Deferred)?;
    let authorized = indexes::authorize_query_index_in_snapshot(&transaction, index_hint)?;
    let table = match authorized.as_ref() {
        Some(idx) => format!("main.files AS f INDEXED BY {}", idx.as_sql()),
        None => "main.files AS f".to_owned(),
    };

    // Every distinct field path that needs extracting, select_fields first
    // (so their column position matches the caller's `field_index`), then
    // any where_fields path not already present.
    let mut distinct_fields: Vec<&String> = select_fields.iter().collect();
    let mut distinct_index: std::collections::HashMap<&str, usize> = select_fields
        .iter()
        .enumerate()
        .map(|(i, f)| (f.as_str(), i))
        .collect();
    for (field, _, _) in where_fields {
        distinct_index.entry(field.as_str()).or_insert_with(|| {
            distinct_fields.push(field);
            distinct_fields.len() - 1
        });
    }

    let mut inner_cols: Vec<String> = vec![
        "f.id".to_owned(),
        "f.path".to_owned(),
        "f.mtime".to_owned(),
        "f.file_size".to_owned(),
        "f.hash".to_owned(),
        "f.last_accessed_at".to_owned(),
    ];
    let mut params: Vec<Value> = Vec::new();
    for (i, field) in distinct_fields.iter().enumerate() {
        inner_cols.push(format!(
            "CASE WHEN json_type(p.content, ?) IN ('integer','real') \
             THEN json_extract(p.content, ?) ELSE NULL END AS field_{i}"
        ));
        let path_expr = format!("$.{field}");
        params.push(Value::Text(path_expr.clone()));
        params.push(Value::Text(path_expr));
    }

    let (clauses, str_params) = path_filter_clauses(namespace, pattern, path_in_dir, path_glob);
    for p in str_params {
        params.push(Value::Text(p));
    }

    let inner_sql = format!(
        "SELECT {} FROM {table} JOIN main.payloads p ON p.file_id = f.id WHERE {}",
        inner_cols.join(", "),
        clauses.join(" AND "),
    );

    let mut outer_where: Vec<String> = Vec::new();
    for (field, cmp, threshold) in where_fields {
        let idx = distinct_index[field.as_str()];
        outer_where.push(format!("field_{idx} {cmp} ?"));
        params.push(Value::Real(*threshold));
    }
    let sql = if outer_where.is_empty() {
        format!("SELECT * FROM ({inner_sql}) ORDER BY path")
    } else {
        format!(
            "SELECT * FROM ({inner_sql}) WHERE {} ORDER BY path",
            outer_where.join(" AND ")
        )
    };

    let n_fields = select_fields.len();
    let mut stmt = transaction.prepare(&sql)?;
    let rows: Result<Vec<_>, _> = stmt
        .query_map(rusqlite::params_from_iter(params.iter()), |r| {
            let candidate = CandidateRow {
                id: r.get(0)?,
                path: r.get(1)?,
                mtime: r.get(2)?,
                file_size: r.get::<_, i64>(3)? as u64,
                hash: r.get(4)?,
                last_accessed_at: r.get::<_, i64>(5)?,
            };
            // `select_fields` occupy columns 0..n_fields of `distinct_fields`
            // by construction, so they are exactly the first `n_fields`
            // extracted columns here too.
            let mut extracted = Vec::with_capacity(n_fields);
            for i in 0..n_fields {
                extracted.push(r.get::<_, Option<f64>>(6 + i)?);
            }
            Ok((candidate, extracted))
        })?
        .collect();
    let rows = rows?;
    drop(stmt);
    transaction.commit()?;
    Ok(rows)
}

/// RFC 021 pass 2: payloads for an explicit id set, one statement per
/// chunk. Chunked at 500 so a `limit` above SQLite's default
/// `SQLITE_MAX_VARIABLE_NUMBER` (999 on older builds) cannot produce a
/// malformed statement. Returned rows are not guaranteed to preserve `ids`'
/// order; callers reassemble by id.
pub(crate) fn payloads_for_ids(
    conn: &Connection,
    ids: &[i64],
) -> Result<Vec<(i64, Vec<u8>, String)>, LocalFileCacheError> {
    const CHUNK: usize = 500;
    let mut out = Vec::with_capacity(ids.len());
    for chunk in ids.chunks(CHUNK) {
        let placeholders = vec!["?"; chunk.len()].join(",");
        let sql = format!(
            "SELECT file_id, content, encoding FROM main.payloads WHERE file_id IN ({placeholders})"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows: Result<Vec<_>, _> = stmt
            .query_map(rusqlite::params_from_iter(chunk.iter()), |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Vec<u8>>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect();
        out.extend(rows?);
    }
    Ok(out)
}

/// RFC 021 R1: one query for the "need every payload" fallback (tier 3) —
/// candidate rows joined with their payload, or `None` when the file row
/// has no payload row. `LEFT JOIN` so that gap is visible to the caller as
/// `None` rather than silently dropping the file row, matching today's
/// `load_payload -> None -> continue` skip.
#[cfg(feature = "json")]
pub(crate) fn query_candidates_with_payloads(
    conn: &Connection,
    namespace: &str,
    pattern: Option<&str>,
    index_hint: Option<&str>,
    path_in_dir: Option<(&str, bool)>,
    path_glob: Option<&[String]>,
) -> Result<Vec<FullCandidateRow>, LocalFileCacheError> {
    let transaction =
        rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Deferred)?;
    let authorized = indexes::authorize_query_index_in_snapshot(&transaction, index_hint)?;
    let table = match authorized.as_ref() {
        Some(idx) => format!("main.files AS f INDEXED BY {}", idx.as_sql()),
        None => "main.files AS f".to_owned(),
    };
    let (clauses, params) = path_filter_clauses(namespace, pattern, path_in_dir, path_glob);
    let sql = format!(
        "SELECT f.path, f.mtime, f.file_size, f.hash, f.last_accessed_at, p.content, p.encoding \
         FROM {table} LEFT JOIN main.payloads p ON p.file_id = f.id WHERE {} ORDER BY f.path",
        clauses.join(" AND ")
    );
    let mut stmt = transaction.prepare(&sql)?;
    let rows: Result<Vec<_>, _> = stmt
        .query_map(
            rusqlite::params_from_iter(params.iter().map(String::as_str)),
            |r| {
                Ok(FullCandidateRow {
                    path: r.get(0)?,
                    mtime: r.get(1)?,
                    file_size: r.get::<_, i64>(2)? as u64,
                    hash: r.get(3)?,
                    last_accessed_at: r.get::<_, i64>(4)?,
                    content: r.get(5)?,
                    encoding: r.get(6)?,
                })
            },
        )?
        .collect();
    let rows = rows?;
    drop(stmt);
    transaction.commit()?;
    Ok(rows)
}

/// RFC 021 tier-2 precondition: `true` when every payload row joined to
/// `namespace` has `encoding = 'json'`. Namespace-scoped and conservative —
/// checked over the whole namespace rather than the query's own path
/// filters, so a uniform namespace never needs re-checking per query shape,
/// and a mixed namespace never pushes down for a subset while silently
/// dropping the rest.
#[cfg(feature = "json")]
pub(crate) fn namespace_all_json(
    conn: &Connection,
    namespace: &str,
) -> Result<bool, LocalFileCacheError> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM main.payloads p JOIN main.files f ON f.id = p.file_id \
         WHERE f.namespace = ?1 AND p.encoding <> 'json'",
        params![namespace],
        |r| r.get(0),
    )?;
    Ok(n == 0)
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
