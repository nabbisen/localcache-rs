//! The RFC 021 one-pass, late-materialization candidate queries. Their only
//! consumer is query execution (`cache::query`); they share the row types,
//! `ID_LIST_CHUNK` and the path-filter SQL helpers with the rest of the
//! repository through the parent module.

use rusqlite::Connection;
#[cfg(feature = "json")]
use rusqlite::params;

#[cfg(feature = "json")]
use super::FullCandidateRow;
use super::{CandidateRow, ID_LIST_CHUNK, path_filter_clauses};
use crate::db::indexes;
use crate::error::LocalFileCacheError;

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
/// [`ID_LIST_CHUNK`]-sized chunk, so a `limit` above SQLite's historical
/// `SQLITE_MAX_VARIABLE_NUMBER` cannot produce a malformed statement.
/// Returned rows are not guaranteed to preserve `ids`' order; callers
/// reassemble by id.
pub(crate) fn payloads_for_ids(
    conn: &Connection,
    ids: &[i64],
) -> Result<Vec<(i64, Vec<u8>, String)>, LocalFileCacheError> {
    let mut out = Vec::with_capacity(ids.len());
    for chunk in ids.chunks(ID_LIST_CHUNK) {
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
