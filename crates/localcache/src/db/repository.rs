//! Low-level database operations (repository layer).

use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::cache::entry::EntryInfo;
use crate::detection::metadata::FileMetadata;
use crate::error::{LocalFileCacheError, SqliteResultExt};

mod candidates;
#[cfg(test)]
pub(crate) mod test_clock;
#[cfg(test)]
mod tests;
#[cfg(feature = "json")]
pub(crate) use candidates::{
    namespace_all_json, query_candidates_json_pushdown, query_candidates_with_payloads,
};
pub(crate) use candidates::{payloads_for_ids, query_candidates};

/// Shared chunk size for every `id`-list SQL statement in this module
/// (`payloads_for_ids`, `evict_lru`) and in `cache::query::materialize`.
/// Kept below SQLite's historical `SQLITE_MAX_VARIABLE_NUMBER` (999 on
/// older builds), so a large `limit`, eviction count, or query result
/// cannot produce a malformed statement. RFC 022 R6: one definition,
/// three uses, so the three stay equal by construction rather than by
/// a comment.
pub(crate) const ID_LIST_CHUNK: usize = 500;

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
    let mut stmt = conn
        .prepare_cached(
            "SELECT id, path, mtime, file_size, hash, updated_at, payload_version
         FROM main.files
         WHERE namespace = ?1 AND path = ?2",
        )
        .db()?;
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
        .optional()
        .db()?;
    Ok(row)
}

pub(crate) fn load_payload(
    conn: &Connection,
    file_id: i64,
) -> Result<Option<PayloadRow>, LocalFileCacheError> {
    let mut stmt = conn
        .prepare_cached("SELECT content, encoding FROM main.payloads WHERE file_id = ?1")
        .db()?;
    let row = stmt
        .query_row(params![file_id], |r| {
            Ok(PayloadRow {
                content: r.get(0)?,
                encoding: r.get(1)?,
            })
        })
        .optional()
        .db()?;
    Ok(row)
}

// The use order (RFC 026 R1). A use of an entry of a namespace assigns it
//
//     max(now_us, (SELECT COALESCE(MAX(last_used_us), 0) + 1 FROM files WHERE namespace = ?))
//
// in the statement that writes it: the larger of the clock and one more than
// every value in the namespace. It is strictly greater than all of them, so the
// order is total and does not depend on the clock's resolution or direction,
// and SQLite's single writer makes the statement atomic across connections and
// processes. `RECORD_USE_SQL` and `UPSERT_SQL` both spell it out; they must
// stay equal.

/// A read hit or a `touch`: `?1` the clock, `?2` the namespace, `?3` the id.
const RECORD_USE_SQL: &str = "UPDATE files SET last_used_us = max(?1, (SELECT COALESCE(MAX(last_used_us), 0) + 1 FROM files WHERE namespace = ?2)) WHERE id = ?3";

/// A write: a new row and an overwrite alike take the next use value
/// (`?8` is the clock; `excluded.last_used_us` carries the computed value into
/// the overwrite branch).
const UPSERT_SQL: &str = "INSERT INTO files
         (namespace, path, mtime, file_size, hash, updated_at, payload_version, last_used_us)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7,
             max(?8, (SELECT COALESCE(MAX(last_used_us), 0) + 1 FROM files WHERE namespace = ?1)))
     ON CONFLICT(namespace, path) DO UPDATE SET
         mtime            = excluded.mtime,
         file_size        = excluded.file_size,
         hash             = excluded.hash,
         updated_at       = excluded.updated_at,
         payload_version  = excluded.payload_version,
         last_used_us     = excluded.last_used_us";

/// The oldest uses of a namespace, for eviction (`?1` namespace, `?2` count).
/// `idx_files_use (namespace, last_used_us)` serves it without a sort; the `id`
/// decides only the ties an import can create.
const EVICTION_CANDIDATES_SQL: &str = "SELECT id, path FROM files
     WHERE namespace = ?1
     ORDER BY last_used_us ASC, id ASC
     LIMIT ?2";

/// The exclusive upper bound of a use value (2^62), also written in the
/// column's `CHECK`. Above it `MAX + 1` would leave the integers.
pub(crate) const USE_VALUE_LIMIT: i64 = 1 << 62;

/// Microseconds in the seconds the public fields report.
const MICROS_PER_SECOND: i64 = 1_000_000;

/// The use value an imported record stands for (RFC 026 R2): its last-use
/// second times 10^6, where the last-use second is `last_accessed_at` when
/// above 0 and `updated_at` otherwise. `None` for a second that is negative or
/// whose product is not below [`USE_VALUE_LIMIT`]; the arithmetic is checked,
/// so no float is ever involved.
pub(crate) fn use_value_of_record(last_accessed_at: i64, updated_at: i64) -> Option<i64> {
    let seconds = if last_accessed_at > 0 {
        last_accessed_at
    } else {
        updated_at
    };
    if seconds < 0 {
        return None;
    }
    seconds
        .checked_mul(MICROS_PER_SECOND)
        .filter(|value| *value < USE_VALUE_LIMIT)
}

/// Map a failure of a statement that assigns a use value. The column's
/// `CHECK` (an integer below 2^62) is the only way `MAX + 1` can fail, and it
/// is the one error that means the use order is exhausted; every other failure
/// is an ordinary database error.
fn use_error(error: rusqlite::Error) -> LocalFileCacheError {
    match &error {
        rusqlite::Error::SqliteFailure(failure, Some(message))
            if failure.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_CHECK
                && message.contains("last_used_us") =>
        {
            LocalFileCacheError::DatabaseState(
                "use order exhausted: a use value would reach 2^62; the database was not modified"
                    .into(),
            )
        }
        _ => LocalFileCacheError::Database(crate::error::DatabaseError::from_sqlite(error)),
    }
}

/// Record a use of the entry `file_id` of `namespace` (a read hit or a
/// `touch`): it becomes the newest entry of its namespace.
///
/// A read ignores this result (a read must not fail for want of bookkeeping);
/// `touch` returns it.
pub(crate) fn record_use(
    conn: &Connection,
    namespace: &str,
    file_id: i64,
) -> Result<(), LocalFileCacheError> {
    conn.prepare_cached(RECORD_USE_SQL)
        .and_then(|mut statement| statement.execute(params![now_us(), namespace, file_id]))
        .map(|_| ())
        .map_err(use_error)
}

// ---------------------------------------------------------------------------
// Writes
// ---------------------------------------------------------------------------

/// Insert or update one entry within `tx`, returning its row id so the
/// caller can protect it from the same call's own eviction (RFC 022 R6).
pub(crate) fn upsert_in_tx(
    tx: &Transaction,
    namespace: &str,
    path: &str,
    metadata: &FileMetadata,
    payload_bytes: &[u8],
    encoding: &str,
    payload_version: u32,
) -> Result<i64, LocalFileCacheError> {
    let updated_at = now_secs();

    // A write is a use (RFC 026 R1), a new row and an overwrite alike.
    tx.prepare_cached(UPSERT_SQL)
        .and_then(|mut statement| {
            statement.execute(params![
                namespace,
                path,
                metadata.mtime,
                metadata.file_size as i64,
                metadata.hash,
                updated_at,
                payload_version as i64,
                now_us(),
            ])
        })
        .map_err(use_error)?;

    let file_id: i64 = tx
        .query_row(
            "SELECT id FROM files WHERE namespace = ?1 AND path = ?2",
            params![namespace, path],
            |r| r.get(0),
        )
        .db()?;

    tx.execute(
        "INSERT INTO payloads (file_id, content, encoding)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(file_id) DO UPDATE SET
             content  = excluded.content,
             encoding = excluded.encoding",
        params![file_id, payload_bytes, encoding],
    )
    .db()?;

    Ok(file_id)
}

// ---------------------------------------------------------------------------
// Deletes
// ---------------------------------------------------------------------------

pub(crate) fn delete_by_path(
    conn: &Connection,
    namespace: &str,
    path: &str,
) -> Result<bool, LocalFileCacheError> {
    let n = conn
        .execute(
            "DELETE FROM files WHERE namespace = ?1 AND path = ?2",
            params![namespace, path],
        )
        .db()?;
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
    let mut stmt = tx
        .prepare_cached("DELETE FROM files WHERE namespace = ?1 AND path = ?2")
        .db()?;
    let mut removed = 0usize;
    for path in paths {
        removed += stmt.execute(params![namespace, path]).db()?;
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
    let tx = conn.unchecked_transaction().db()?;
    let removed = delete_paths_in_tx(&tx, namespace, paths)?;
    tx.commit().db()?;
    Ok(removed)
}

pub(crate) fn delete_by_other_version(
    conn: &Connection,
    namespace: &str,
    current_version: u32,
) -> Result<usize, LocalFileCacheError> {
    let n = conn
        .execute(
            "DELETE FROM files WHERE namespace = ?1 AND payload_version != ?2",
            params![namespace, current_version as i64],
        )
        .db()?;
    Ok(n)
}

/// RFC 022 R6, RFC 026 R1: evict up to `n` rows of `namespace` in
/// least-recently-**used** order (`last_used_us` ascending, then `id`, which
/// decides only the ties an import can create), never touching `protected`
/// ids. Returns the paths of the rows actually
/// deleted; if fewer than `n` unprotected rows exist, that is not an
/// error — every one of them is deleted.
///
/// Takes the caller's `&Transaction` and opens none of its own (RFC 022 R6
/// item 3, Q0c review C1): the write that produced `protected`, the count
/// that produced `n`, and this selection-and-deletion must all run in the
/// **same** `IMMEDIATE` transaction, so a concurrent writer waits under the
/// busy timeout rather than a deferred read-then-write here racing it and
/// failing with `SQLITE_BUSY_SNAPSHOT`.
///
/// `protected` is excluded in Rust after one ordered `SELECT`, not via a
/// SQL `NOT IN (...)` list: `batch_set` can protect an arbitrarily large
/// number of ids (every row it just wrote in one call), and a
/// parameter-bound `NOT IN` risks SQLite's historical
/// `SQLITE_MAX_VARIABLE_NUMBER` on a large batch. Over-selecting by
/// `protected.len()` before filtering is equivalent to a `NOT IN` clause:
/// at most `protected.len()` of the fetched rows can be excluded, so at
/// least `n` unprotected candidates are considered whenever that many
/// exist in the namespace.
pub(crate) fn evict_lru(
    tx: &Transaction,
    namespace: &str,
    n: usize,
    protected: &[i64],
) -> Result<Vec<PathBuf>, LocalFileCacheError> {
    use std::collections::HashSet;

    let protected: HashSet<i64> = protected.iter().copied().collect();

    let fetch = n.saturating_add(protected.len());
    let candidates: Vec<(i64, String)> = {
        let mut stmt = tx.prepare_cached(EVICTION_CANDIDATES_SQL).db()?;
        let rows: Result<Vec<_>, _> = stmt
            .query_map(params![namespace, fetch as i64], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })
            .db()?
            .collect();
        rows.db()?
    };

    let selected: Vec<(i64, String)> = candidates
        .into_iter()
        .filter(|(id, _)| !protected.contains(id))
        .take(n)
        .collect();

    let ids: Vec<i64> = selected.iter().map(|(id, _)| *id).collect();
    for chunk in ids.chunks(ID_LIST_CHUNK) {
        let placeholders = vec!["?"; chunk.len()].join(",");
        let sql = format!("DELETE FROM files WHERE id IN ({placeholders})");
        tx.execute(&sql, rusqlite::params_from_iter(chunk.iter()))
            .db()?;
    }

    Ok(selected
        .into_iter()
        .map(|(_, path)| PathBuf::from(path))
        .collect())
}

// ---------------------------------------------------------------------------
// Scans / aggregates
// ---------------------------------------------------------------------------

/// RFC 020: one page of paths in `namespace`, ordered by `path`, strictly
/// after `after`. Pass `""` for the first page — every stored path is a
/// non-empty absolute path, so `path > ''` selects all of them. Served by
/// the index SQLite builds for `UNIQUE(namespace, path)`; no new index.
pub(crate) fn paths_page_in_namespace(
    conn: &Connection,
    namespace: &str,
    after: &str,
    limit: usize,
) -> Result<Vec<String>, LocalFileCacheError> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT path FROM files
         WHERE namespace = ?1 AND path > ?2
         ORDER BY path
         LIMIT ?3",
        )
        .db()?;
    let paths: Result<Vec<String>, _> = stmt
        .query_map(params![namespace, after, limit as i64], |r| r.get(0))
        .db()?
        .collect();
    paths.db()
}

/// RFC 020: same page shape as [`paths_page_in_namespace`], carrying
/// `updated_at` for `cleanup_expired`.
pub(crate) fn path_rows_page_in_namespace(
    conn: &Connection,
    namespace: &str,
    after: &str,
    limit: usize,
) -> Result<Vec<(String, i64)>, LocalFileCacheError> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT path, updated_at FROM files
         WHERE namespace = ?1 AND path > ?2
         ORDER BY path
         LIMIT ?3",
        )
        .db()?;
    let rows: Result<Vec<_>, _> = stmt
        .query_map(params![namespace, after, limit as i64], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })
        .db()?
        .collect();
    rows.db()
}

pub(crate) fn count_in_namespace(
    conn: &Connection,
    namespace: &str,
) -> Result<usize, LocalFileCacheError> {
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM files WHERE namespace = ?1",
            params![namespace],
            |r| r.get(0),
        )
        .db()?;
    Ok(n as usize)
}

pub(crate) fn count_by_version(
    conn: &Connection,
    namespace: &str,
) -> Result<Vec<(u32, usize)>, LocalFileCacheError> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT payload_version, COUNT(*)
         FROM files
         WHERE namespace = ?1
         GROUP BY payload_version
         ORDER BY payload_version ASC",
        )
        .db()?;
    let rows: Result<Vec<_>, _> = stmt
        .query_map(params![namespace], |r| {
            Ok((r.get::<_, i64>(0)? as u32, r.get::<_, i64>(1)? as usize))
        })
        .db()?
        .collect();
    rows.db()
}

/// Return lightweight metadata for all entries in `namespace`, joined with
/// their encoding from `payloads`.  Does **not** load payload content.
pub(crate) fn list_entries(
    conn: &Connection,
    namespace: &str,
) -> Result<Vec<EntryInfo>, LocalFileCacheError> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT f.path, f.mtime, f.file_size, f.hash,
                f.updated_at, f.payload_version, f.last_used_us / 1000000,
                p.encoding
         FROM files f
         JOIN payloads p ON p.file_id = f.id
         WHERE f.namespace = ?1
         ORDER BY f.updated_at DESC",
        )
        .db()?;
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
        })
        .db()?
        .collect();
    rows.db()
}

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

/// The wall clock in microseconds since the Unix epoch. It is one input of the
/// use order, never the whole of it (RFC 026 R1), so a clock that is coarse,
/// stuck or steps backwards cannot produce a tie or a regression.
pub(crate) fn now_us() -> i64 {
    #[cfg(test)]
    if let Some(value) = test_clock::next() {
        return value;
    }
    UNIX_EPOCH
        .elapsed()
        .map(|d| i64::try_from(d.as_micros()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

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
    let row = conn
        .query_row(
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
        )
        .db()?;
    Ok(row)
}

pub(crate) fn encoding_breakdown(
    conn: &Connection,
    namespace: &str,
) -> Result<Vec<(String, usize)>, LocalFileCacheError> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT p.encoding, COUNT(*)
         FROM files f
         JOIN payloads p ON p.file_id = f.id
         WHERE f.namespace = ?1
         GROUP BY p.encoding
         ORDER BY p.encoding ASC",
        )
        .db()?;
    let rows: Result<Vec<_>, _> = stmt
        .query_map(params![namespace], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize))
        })
        .db()?
        .collect();
    rows.db()
}

// ---------------------------------------------------------------------------
// Key rotation support
// ---------------------------------------------------------------------------

/// A row of `payloads` with its path and tag, as key rotation reads it.
#[cfg(feature = "encryption")]
pub(crate) struct PayloadPageRow {
    pub file_id: i64,
    pub path: String,
    pub content: Vec<u8>,
    pub encoding: String,
}

/// One page of the payload rows of `namespace`, in `path` order, strictly
/// after `after` (RFC 029 R3).
///
/// The page holds at most `max_rows` rows and ends as soon as its content
/// reaches `max_bytes`, but always holds at least one row when any is left.
/// It pages by `(namespace, path)`, the pattern of RFC 020: served by
/// `sqlite_autoindex_files_1` with no sort. Pass `""` for the first page
/// (every stored path is non-empty). Paging by `file_id` instead plans a scan
/// of the whole namespace and a sort for every page.
#[cfg(feature = "encryption")]
pub(crate) fn load_payload_page(
    conn: &Connection,
    namespace: &str,
    after: &str,
    max_rows: usize,
    max_bytes: usize,
) -> Result<Vec<PayloadPageRow>, LocalFileCacheError> {
    let mut stmt = conn.prepare_cached(PAYLOAD_PAGE_SQL).db()?;
    let mut rows = stmt
        .query(params![namespace, after, max_rows as i64])
        .db()?;
    let mut page = Vec::new();
    let mut bytes = 0_usize;
    while let Some(r) = rows.next().db()? {
        let content: Vec<u8> = r.get(3).db()?;
        bytes = bytes.saturating_add(content.len());
        page.push(PayloadPageRow {
            file_id: r.get(0).db()?,
            path: r.get(1).db()?,
            encoding: r.get(2).db()?,
            content,
        });
        if bytes >= max_bytes {
            break;
        }
    }
    Ok(page)
}

/// The statement behind [`load_payload_page`]; named so a test can ask SQLite
/// for its plan.
#[cfg(feature = "encryption")]
pub(crate) const PAYLOAD_PAGE_SQL: &str = "SELECT p.file_id, f.path, p.encoding, p.content \
     FROM files f JOIN payloads p ON p.file_id = f.id \
     WHERE f.namespace = ?1 AND f.path > ?2 \
     ORDER BY f.path LIMIT ?3";

/// Update a payload row's content and tag (used by key rotation).
#[cfg(feature = "encryption")]
pub(crate) fn update_payload_content(
    tx: &Transaction,
    file_id: i64,
    new_content: &[u8],
    new_tag: &str,
) -> Result<(), LocalFileCacheError> {
    tx.execute(
        "UPDATE payloads SET content = ?1, encoding = ?2 WHERE file_id = ?3",
        params![new_content, new_tag, file_id],
    )
    .db()?;
    Ok(())
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
    let mut stmt = conn
        .prepare_cached(
            "SELECT f.path, p.content, p.encoding,
                f.mtime, f.file_size, f.hash,
                f.payload_version, f.updated_at, f.last_used_us / 1000000
         FROM files f
         JOIN payloads p ON p.file_id = f.id
         WHERE f.namespace = ?1
         ORDER BY f.last_used_us ASC, f.id ASC",
        )
        .db()?;
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
        })
        .db()?
        .collect();
    rows.db()
}

/// The error for a record whose recency the use order cannot hold.
pub(crate) fn unrepresentable_recency() -> LocalFileCacheError {
    crate::error::invalid_argument(
        crate::error::ArgumentName::Records,
        "the recency of a record (last_accessed_at or updated_at) is out of range".to_owned(),
    )
}

/// Import a batch of rows into `namespace` inside the caller's transaction.
///
/// Existing entries (matched on `namespace + path`) are replaced. Returns the
/// id of the row written for each input row, in order (a path that appears
/// twice yields its id twice), so the caller can protect them from eviction
/// and commit when it is done.
pub(crate) fn import_rows_in_tx(
    tx: &Transaction,
    namespace: &str,
    rows: &[FullRow],
) -> Result<Vec<i64>, LocalFileCacheError> {
    let mut written = Vec::with_capacity(rows.len());
    for row in rows {
        // An import is a restore, not a use (RFC 026 R2): the row keeps the
        // recency it was exported with, at the resolution of the exported
        // seconds. `import_bounded` has already refused a record this cannot
        // express, before writing anything.
        let use_value = use_value_of_record(row.last_accessed_at, row.updated_at)
            .ok_or_else(unrepresentable_recency)?;
        tx.execute(
            "INSERT INTO files
                 (namespace, path, mtime, file_size, hash, updated_at,
                  payload_version, last_used_us)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(namespace, path) DO UPDATE SET
                 mtime            = excluded.mtime,
                 file_size        = excluded.file_size,
                 hash             = excluded.hash,
                 updated_at       = excluded.updated_at,
                 payload_version  = excluded.payload_version,
                 last_used_us     = excluded.last_used_us",
            params![
                namespace,
                row.path,
                row.mtime,
                row.file_size as i64,
                row.hash,
                row.updated_at,
                row.payload_version as i64,
                use_value,
            ],
        )
        .db()?;

        let file_id: i64 = tx
            .query_row(
                "SELECT id FROM files WHERE namespace = ?1 AND path = ?2",
                params![namespace, row.path],
                |r| r.get(0),
            )
            .db()?;

        tx.execute(
            "INSERT INTO payloads (file_id, content, encoding)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(file_id) DO UPDATE SET
                 content  = excluded.content,
                 encoding = excluded.encoding",
            params![file_id, row.content, row.encoding],
        )
        .db()?;
        written.push(file_id);
    }
    Ok(written)
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
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM files WHERE namespace = ?1 AND path = ?2",
            params![namespace, path],
            |r| r.get(0),
        )
        .db()?;
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
    path_in_dir: Option<(&str, bool)>,
    path_glob: Option<&[String]>,
) -> Result<Vec<std::path::PathBuf>, LocalFileCacheError> {
    let (sql, params_vec) = build_path_sql(namespace, pattern, path_in_dir, path_glob);
    let mut stmt = conn.prepare(&sql).db()?;
    let paths: Result<Vec<_>, _> = stmt
        .query_map(
            rusqlite::params_from_iter(params_vec.iter().map(String::as_str)),
            |r| Ok(std::path::PathBuf::from(r.get::<_, String>(0)?)),
        )
        .db()?
        .collect();
    paths.db()
}

/// Run `EXPLAIN QUERY PLAN <sql>` and return the human-readable plan as a
/// newline-joined string, one detail line per step.
pub(crate) fn explain_query(
    conn: &Connection,
    namespace: &str,
    pattern: Option<&str>,
    path_in_dir: Option<(&str, bool)>,
    path_glob: Option<&[String]>,
) -> Result<String, LocalFileCacheError> {
    let (sql, params_vec) = build_path_sql(namespace, pattern, path_in_dir, path_glob);
    let explain_sql = format!("EXPLAIN QUERY PLAN {sql}");
    let mut stmt = conn.prepare(&explain_sql).db()?;
    let rows: Result<Vec<String>, _> = stmt
        .query_map(
            rusqlite::params_from_iter(params_vec.iter().map(String::as_str)),
            |row| row.get::<_, String>(3),
        )
        .db()?
        .collect();
    Ok(rows.db()?.join("\n"))
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
    path_in_dir: Option<(&str, bool)>,
    path_glob: Option<&[String]>,
) -> (String, Vec<String>) {
    let (clauses, params) = path_filter_clauses(namespace, pattern, path_in_dir, path_glob);
    let sql = format!(
        "SELECT path FROM main.files WHERE {} ORDER BY path",
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

    // path_like — SQL LIKE with `\` as the ESCAPE character, so a caller can
    // match a literal `%`, `_`, or `\` by writing `\%`, `\_`, or `\\`.
    if let Some(pat) = pattern {
        clauses.push("path LIKE ? ESCAPE '\\'".to_owned());
        params.push(pat.to_owned());
    }

    // path_in_dir — an exact, binary-collated range on `path`, optionally
    // excluding sub-subdirectories. `LIKE` is not used: it ignores ASCII case
    // and cannot use the binary-collated index, so it returned case-variant
    // directories and scanned the whole namespace (RFC 006 Erratum 1).
    if let Some((prefix, recursive)) = path_in_dir {
        // Recursive: every path in `[prefix, upper)`, i.e. every path that
        // starts with the prefix. `upper` is the prefix with its last char
        // (the separator, one ASCII byte) replaced by the next code point, so
        // the bound is exact under byte-wise UTF-8 comparison.
        clauses.push("path >= ? AND path < ?".to_owned());
        params.push(prefix.to_owned());
        params.push(range_upper_bound(prefix));
        if !recursive {
            // Non-recursive: the text after the prefix holds no further
            // separator. `substr` and `instr` count characters on TEXT, so
            // the offset is the prefix length in Unicode scalar values plus
            // one. Every value is bound: the SQL text never varies with the
            // directory.
            clauses.push("instr(substr(path, CAST(? AS INTEGER)), ?) = 0".to_owned());
            params.push((prefix.chars().count() + 1).to_string());
            params.push(std::path::MAIN_SEPARATOR_STR.to_owned());
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

/// The exclusive upper bound of the range of strings that start with `prefix`:
/// `prefix` with its last character replaced by the next code point
/// (`/` becomes `0`, `\` becomes `]`). `prefix` ends in a path separator, a
/// single ASCII byte, so no carry and no surrogate gap can arise.
fn range_upper_bound(prefix: &str) -> String {
    let mut upper = prefix.to_owned();
    if let Some(last) = upper.pop() {
        // The successor of an ASCII separator is always a valid `char`; the
        // fallback only keeps this function total, it is never taken.
        upper.push(char::from_u32(u32::from(last) + 1).unwrap_or(char::MAX));
    }
    upper
}

// ---------------------------------------------------------------------------
// Namespace management
// ---------------------------------------------------------------------------

/// Return all distinct namespace names in the database, sorted.
pub(crate) fn list_namespaces(conn: &Connection) -> Result<Vec<String>, LocalFileCacheError> {
    let mut stmt = conn
        .prepare_cached("SELECT DISTINCT namespace FROM files ORDER BY namespace")
        .db()?;
    let ns: Result<Vec<String>, _> = stmt.query_map([], |r| r.get(0)).db()?.collect();
    ns.db()
}
