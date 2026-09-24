//! Query execution: turns a built [`QueryBuilder`](super::QueryBuilder) into a
//! [`QueryReport`](super::QueryReport). Holds the RFC 021 plan tiers, the
//! offset/limit materialization (RFC 022 R2), and the sort comparators.

use std::path::PathBuf;

use serde::{Serialize, de::DeserializeOwned};

use super::{OrderBy, QueryBuilder, QueryReport, SkippedEntry, SortOrder, decode_with};
#[cfg(feature = "json")]
use super::{Predicate, get_field};
use crate::cache::entry::CacheEntry;
use crate::db::repository::{self, CandidateRow};
use crate::detection::metadata::FileMetadata;
use crate::error::LocalFileCacheError;

pub(super) struct PreparedPathFilters {
    pub(super) path_in_dir: Option<(String, bool)>,
    pub(super) path_glob: Option<crate::cache::glob::CompiledGlob>,
}

impl PreparedPathFilters {
    pub(super) fn path_in_dir(&self) -> Option<(&str, bool)> {
        self.path_in_dir
            .as_ref()
            .map(|(prefix, recursive)| (prefix.as_str(), *recursive))
    }

    pub(super) fn path_glob(&self) -> Option<&[String]> {
        self.path_glob
            .as_ref()
            .map(crate::cache::glob::CompiledGlob::sqlite_alternatives)
    }
}

/// The one place a query runs. `run()` is this with `skipped` discarded, so
/// the offset/limit logic (RFC 022 R2) exists once.
pub(crate) fn execute_query<T>(
    q: QueryBuilder<'_, T>,
) -> Result<Vec<CacheEntry<T>>, LocalFileCacheError>
where
    T: Serialize + DeserializeOwned,
{
    execute_report(q).map(|report| report.entries)
}

#[cfg(feature = "json")]
pub(crate) fn execute_report<T>(
    q: QueryBuilder<'_, T>,
) -> Result<QueryReport<T>, LocalFileCacheError>
where
    T: Serialize + DeserializeOwned,
{
    let prepared = q.prepare_path_filters()?;
    let plan = classify_query(&q);
    let plan = match plan {
        QueryPlan::Tier2 { .. }
            if !repository::namespace_all_json(q.core.conn, q.core.namespace)? =>
        {
            QueryPlan::Tier3
        }
        other => other,
    };

    match plan {
        QueryPlan::Tier3 => execute_tier3(q, &prepared),
        QueryPlan::Tier1 => {
            let candidates = repository::query_candidates(
                q.core.conn,
                q.core.namespace,
                q.path_like.as_deref(),
                q.index_hint.as_deref(),
                prepared.path_in_dir(),
                prepared.path_glob(),
            )?;
            let mut order: Vec<usize> = (0..candidates.len()).collect();
            if !q.order_by.is_empty() {
                order.sort_by(|&ia, &ib| {
                    for key in &q.order_by {
                        let c = cmp_candidate_basic(&candidates[ia], &candidates[ib], key);
                        if c != std::cmp::Ordering::Equal {
                            return c;
                        }
                    }
                    std::cmp::Ordering::Equal
                });
            }
            materialize(&q, &candidates, &order)
        }
        QueryPlan::Tier2 {
            select_fields,
            field_index,
            where_fields,
        } => {
            let rows = repository::query_candidates_json_pushdown(
                q.core.conn,
                q.core.namespace,
                q.path_like.as_deref(),
                q.index_hint.as_deref(),
                prepared.path_in_dir(),
                prepared.path_glob(),
                &select_fields,
                &where_fields,
            )?;
            let (candidates, field_values): (Vec<CandidateRow>, Vec<Vec<Option<f64>>>) =
                rows.into_iter().unzip();
            let mut order: Vec<usize> = (0..candidates.len()).collect();
            if !q.order_by.is_empty() {
                order.sort_by(|&ia, &ib| {
                    for key in &q.order_by {
                        let c = cmp_candidate_json(
                            &candidates[ia],
                            &field_values[ia],
                            &candidates[ib],
                            &field_values[ib],
                            &field_index,
                            key,
                        );
                        if c != std::cmp::Ordering::Equal {
                            return c;
                        }
                    }
                    std::cmp::Ordering::Equal
                });
            }
            materialize(&q, &candidates, &order)
        }
    }
}

#[cfg(not(feature = "json"))]
pub(crate) fn execute_report<T>(
    q: QueryBuilder<'_, T>,
) -> Result<QueryReport<T>, LocalFileCacheError>
where
    T: Serialize + DeserializeOwned,
{
    let prepared = q.prepare_path_filters()?;
    let candidates = repository::query_candidates(
        q.core.conn,
        q.core.namespace,
        q.path_like.as_deref(),
        q.index_hint.as_deref(),
        prepared.path_in_dir(),
        prepared.path_glob(),
    )?;
    let mut order: Vec<usize> = (0..candidates.len()).collect();
    if !q.order_by.is_empty() {
        order.sort_by(|&ia, &ib| {
            for key in &q.order_by {
                let c = cmp_candidate_basic(&candidates[ia], &candidates[ib], key);
                if c != std::cmp::Ordering::Equal {
                    return c;
                }
            }
            std::cmp::Ordering::Equal
        });
    }
    materialize(&q, &candidates, &order)
}

/// Tier 3: every candidate's payload is decoded, exactly as before RFC 021 —
/// used when a field predicate/sort cannot be pushed into SQL (a non-numeric
/// predicate, an unsafe field path, or a namespace whose payloads are not
/// uniformly `encoding = 'json'`). Still benefits from R1: one streaming
/// query replaces the old `1 + 2N` per-path fetch loop.
#[cfg(feature = "json")]
fn execute_tier3<T>(
    q: QueryBuilder<'_, T>,
    prepared: &PreparedPathFilters,
) -> Result<QueryReport<T>, LocalFileCacheError>
where
    T: Serialize + DeserializeOwned,
{
    let rows = repository::query_candidates_with_payloads(
        q.core.conn,
        q.core.namespace,
        q.path_like.as_deref(),
        q.index_hint.as_deref(),
        prepared.path_in_dir(),
        prepared.path_glob(),
    )?;

    let mut matched: Vec<(CacheEntry<T>, serde_json::Value, i64)> = Vec::new();
    let mut skipped: Vec<SkippedEntry> = Vec::new();
    for row in rows {
        let (Some(content), Some(encoding)) = (row.content, row.encoding) else {
            continue;
        };
        let payload: T = match decode_with(&q.core, &content, &encoding) {
            Ok(p) => p,
            Err(error) => {
                skipped.push(SkippedEntry {
                    path: PathBuf::from(&row.path),
                    error,
                });
                continue;
            }
        };
        let laa = row.last_accessed_at;
        let entry = CacheEntry {
            path: PathBuf::from(&row.path),
            metadata: FileMetadata {
                mtime: row.mtime,
                file_size: row.file_size,
                hash: row.hash,
            },
            payload,
        };

        let needs_json = !q.predicates.is_empty()
            || q.order_by
                .iter()
                .any(|o| matches!(o, OrderBy::Field { .. }));
        let json_val = if needs_json {
            match serde_json::to_value(&entry.payload) {
                Ok(v) => v,
                Err(e) => {
                    skipped.push(SkippedEntry {
                        path: entry.path,
                        error: LocalFileCacheError::Serialization(e.to_string()),
                    });
                    continue;
                }
            }
        } else {
            serde_json::Value::Null
        };
        if q.predicates.iter().all(|p| p.matches(&json_val)) {
            matched.push((entry, json_val, laa));
        }
    }

    if !q.order_by.is_empty() {
        matched.sort_by(|(ea, va, la_a), (eb, vb, la_b)| {
            for key in &q.order_by {
                let c = cmp_key_json(ea, va, *la_a, eb, vb, *la_b, key);
                if c != std::cmp::Ordering::Equal {
                    return c;
                }
            }
            std::cmp::Ordering::Equal
        });
    }

    let start = q.offset.min(matched.len());
    let end = q
        .limit
        .map(|l| (start + l).min(matched.len()))
        .unwrap_or(matched.len());
    let entries = matched
        .into_iter()
        .skip(start)
        .take(end - start)
        .map(|(e, _, _)| e)
        .collect();
    Ok(QueryReport { entries, skipped })
}

/// RFC 021 pass 2 / RFC 022 R2: materialize `candidates[order[..]]`,
/// decoding payloads only for the rows needed to fill `offset`/`limit`.
/// `offset` counts only entries that decode successfully: a row whose
/// payload is missing or fails to decode is never counted toward `offset`
/// and never appears in the result, and the next candidate in `order` is
/// tried in its place. Before RFC 022 R2, `offset` counted candidates
/// positionally, so a bad row before or within the window could shift or
/// duplicate a page; that defect is what this fixes.
/// `candidates`/`order` were already fully materialized in memory by pass 1
/// to make sorting possible, so scanning past a bad row costs no extra SQL
/// beyond the occasional additional `payloads_for_ids` chunk.
fn materialize<T>(
    q: &QueryBuilder<'_, T>,
    candidates: &[CandidateRow],
    order: &[usize],
) -> Result<QueryReport<T>, LocalFileCacheError>
where
    T: Serialize + DeserializeOwned,
{
    use std::collections::HashMap;

    let mut to_skip = q.offset;
    let target = q.limit;
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    let mut idx = 0;
    while target.is_none_or(|t| out.len() < t) && idx < order.len() {
        let want = match target {
            Some(t) => to_skip.saturating_add(t - out.len()),
            None => order.len() - idx,
        };
        // `repository::ID_LIST_CHUNK`: the same chunk size `payloads_for_ids`
        // and `evict_lru` use, so all three id-list SQL statements in this
        // codebase stay equal by construction (RFC 022 R6).
        let window_end = (idx + want.min(repository::ID_LIST_CHUNK)).min(order.len());
        let window = &order[idx..window_end];
        let ids: Vec<i64> = window.iter().map(|&i| candidates[i].id).collect();
        let payload_rows = repository::payloads_for_ids(q.core.conn, &ids)?;
        let mut payload_map: HashMap<i64, (Vec<u8>, String)> = payload_rows
            .into_iter()
            .map(|(id, content, encoding)| (id, (content, encoding)))
            .collect();
        for &i in window {
            let c = &candidates[i];
            // Missing payload or decode failure: never counted toward
            // `offset`, never in the result — try the next candidate.
            let Some((content, encoding)) = payload_map.remove(&c.id) else {
                continue;
            };
            let payload: T = match decode_with(&q.core, &content, &encoding) {
                Ok(p) => p,
                Err(error) => {
                    skipped.push(SkippedEntry {
                        path: PathBuf::from(&c.path),
                        error,
                    });
                    continue;
                }
            };
            if to_skip > 0 {
                // A successful decode counts toward `offset`, but is not
                // itself part of the page.
                to_skip -= 1;
                continue;
            }
            out.push(CacheEntry {
                path: PathBuf::from(&c.path),
                metadata: FileMetadata {
                    mtime: c.mtime,
                    file_size: c.file_size,
                    hash: c.hash.clone(),
                },
                payload,
            });
            if target.is_some_and(|t| out.len() == t) {
                break;
            }
        }
        idx = window_end;
    }
    Ok(QueryReport {
        entries: out,
        skipped,
    })
}

// ---------------------------------------------------------------------------
// RFC 021 — query plan classification (json feature)
// ---------------------------------------------------------------------------

#[cfg(feature = "json")]
enum QueryPlan {
    /// No field predicate, no field-based sort: payload content is never
    /// needed until the winning ids are known.
    Tier1,
    /// Field predicate(s) are `field_gt`/`field_lt` only (or absent), every
    /// referenced field path is safe to translate to a SQLite JSON path,
    /// and (checked separately, after classification) the namespace's
    /// payloads are uniformly `encoding = 'json'`.
    Tier2 {
        /// Distinct field paths that must come back as a column (the field
        /// keys used by `order_by`/`then_by_field`), in stable order.
        select_fields: Vec<String>,
        /// `select_fields[path]` — position of each path's column.
        field_index: std::collections::HashMap<String, usize>,
        /// Pushed-down `(field_path, ">"|"<", threshold)` predicates.
        where_fields: Vec<(String, &'static str, f64)>,
    },
    /// A field predicate/sort exists that cannot be pushed down. Every
    /// candidate's payload is decoded, as before this RFC.
    Tier3,
}

/// Reject a field path SQLite's JSON path syntax would parse differently
/// than [`get_field`]'s plain dot-split lookup: quotes, brackets, and `$`
/// are meaningful to SQLite's path grammar, not to `get_field`. A rejected
/// path routes the query to tier 3 rather than to an incorrect tier 2
/// extraction — never a hard error, since path shape has never before been
/// a thing a caller had to think about.
#[cfg(feature = "json")]
fn is_safe_json_field_path(path: &str) -> bool {
    !path.is_empty()
        && path
            .split('.')
            .all(|segment| !segment.is_empty() && !segment.contains(['\'', '"', '[', ']', '$']))
}

#[cfg(feature = "json")]
fn classify_query<T>(q: &QueryBuilder<'_, T>) -> QueryPlan {
    let has_field_order = q
        .order_by
        .iter()
        .any(|o| matches!(o, OrderBy::Field { .. }));

    if q.predicates.is_empty() && !has_field_order {
        return QueryPlan::Tier1;
    }

    let all_numeric_predicates = q
        .predicates
        .iter()
        .all(|p| matches!(p, Predicate::FieldGt { .. } | Predicate::FieldLt { .. }));
    if !all_numeric_predicates {
        return QueryPlan::Tier3;
    }

    let mut select_fields: Vec<String> = Vec::new();
    let mut field_index: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for o in &q.order_by {
        if let OrderBy::Field { path, .. } = o {
            if !is_safe_json_field_path(path) {
                return QueryPlan::Tier3;
            }
            field_index.entry(path.clone()).or_insert_with(|| {
                select_fields.push(path.clone());
                select_fields.len() - 1
            });
        }
    }

    let mut where_fields: Vec<(String, &'static str, f64)> = Vec::new();
    for p in &q.predicates {
        let (path, cmp, threshold) = match p {
            Predicate::FieldGt { path, threshold } => (path, ">", *threshold),
            Predicate::FieldLt { path, threshold } => (path, "<", *threshold),
            _ => unreachable!("all_numeric_predicates checked above"),
        };
        if !is_safe_json_field_path(path) {
            return QueryPlan::Tier3;
        }
        where_fields.push((path.clone(), cmp, threshold));
    }

    QueryPlan::Tier2 {
        select_fields,
        field_index,
        where_fields,
    }
}

/// R4: what [`QueryBuilder::dry_run`] reports about execution, in addition
/// to the SQLite plan — which tier `run()` would take and why, so "this
/// query decodes every payload in the namespace" is visible before someone
/// measures it.
#[cfg(feature = "json")]
pub(super) fn describe_query_plan<T>(
    q: &QueryBuilder<'_, T>,
) -> Result<String, LocalFileCacheError> {
    let plan = classify_query(q);
    let plan = match plan {
        QueryPlan::Tier2 { .. }
            if !repository::namespace_all_json(q.core.conn, q.core.namespace)? =>
        {
            QueryPlan::Tier3
        }
        other => other,
    };
    Ok(match plan {
        QueryPlan::Tier1 => "execution: tier 1 — no field predicate or field sort; payload \
             content is decoded only for rows surviving offset/limit"
            .to_owned(),
        QueryPlan::Tier2 { select_fields, .. } => format!(
            "execution: tier 2 — JSON field(s) {select_fields:?} evaluated in SQL via \
             json_extract; payload content is still decoded only for rows surviving \
             offset/limit"
        ),
        QueryPlan::Tier3 => "execution: tier 3 — every candidate payload is decoded before \
             ordering/limiting (a non-`field_gt`/`field_lt` predicate, a field path unsafe to \
             push into SQL, or a payload encoding other than 'json' is present in this \
             namespace)"
            .to_owned(),
    })
}

#[cfg(not(feature = "json"))]
pub(super) fn describe_query_plan<T>(
    _q: &QueryBuilder<'_, T>,
) -> Result<String, LocalFileCacheError> {
    Ok(
        "execution: tier 1 — no field predicate or field sort; payload content is decoded \
        only for rows surviving offset/limit"
            .to_owned(),
    )
}

// ---------------------------------------------------------------------------
// Per-key comparison helpers
// ---------------------------------------------------------------------------

/// Compares two candidate rows (no payload) on every `OrderBy` key except
/// `Field`, which tier 1 never carries by construction (`classify_query`
/// routes any field-sorted query to tier 2 or tier 3).
fn cmp_candidate_basic(a: &CandidateRow, b: &CandidateRow, key: &OrderBy) -> std::cmp::Ordering {
    match key {
        #[cfg(feature = "json")]
        OrderBy::Field { .. } => unreachable!("tier 1 never carries a field order_by key"),
        OrderBy::Mtime(ord) => ord_dir(a.mtime.cmp(&b.mtime), *ord),
        OrderBy::LastAccessed(ord) => ord_dir(a.last_accessed_at.cmp(&b.last_accessed_at), *ord),
        OrderBy::Path(ord) => ord_dir(
            std::path::Path::new(&a.path).cmp(std::path::Path::new(&b.path)),
            *ord,
        ),
    }
}

/// Tier 2's comparator: like [`cmp_candidate_basic`], plus `Field`, read
/// from the `json_extract`-derived column rather than a decoded payload.
#[cfg(feature = "json")]
#[allow(clippy::too_many_arguments)]
fn cmp_candidate_json(
    a: &CandidateRow,
    a_fields: &[Option<f64>],
    b: &CandidateRow,
    b_fields: &[Option<f64>],
    field_index: &std::collections::HashMap<String, usize>,
    key: &OrderBy,
) -> std::cmp::Ordering {
    match key {
        OrderBy::Field { path, order } => {
            let idx = field_index[path];
            let c = a_fields[idx]
                .partial_cmp(&b_fields[idx])
                .unwrap_or(std::cmp::Ordering::Equal);
            ord_dir(c, *order)
        }
        OrderBy::Mtime(ord) => ord_dir(a.mtime.cmp(&b.mtime), *ord),
        OrderBy::LastAccessed(ord) => ord_dir(a.last_accessed_at.cmp(&b.last_accessed_at), *ord),
        OrderBy::Path(ord) => ord_dir(
            std::path::Path::new(&a.path).cmp(std::path::Path::new(&b.path)),
            *ord,
        ),
    }
}

/// Tier 3's comparator: unchanged from before RFC 021, operating on the
/// fully decoded entry.
#[cfg(feature = "json")]
fn cmp_key_json<T>(
    ea: &CacheEntry<T>,
    va: &serde_json::Value,
    la_a: i64,
    eb: &CacheEntry<T>,
    vb: &serde_json::Value,
    la_b: i64,
    key: &OrderBy,
) -> std::cmp::Ordering {
    match key {
        OrderBy::Field { path, order } => {
            let a = get_field(va, path).and_then(json_sort_key);
            let b = get_field(vb, path).and_then(json_sort_key);
            let c = a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal);
            if *order == SortOrder::Desc {
                c.reverse()
            } else {
                c
            }
        }
        OrderBy::Mtime(ord) => ord_dir(ea.metadata.mtime.cmp(&eb.metadata.mtime), *ord),
        OrderBy::LastAccessed(ord) => ord_dir(la_a.cmp(&la_b), *ord),
        OrderBy::Path(ord) => ord_dir(ea.path.cmp(&eb.path), *ord),
    }
}

#[inline]
fn ord_dir(c: std::cmp::Ordering, ord: SortOrder) -> std::cmp::Ordering {
    if ord == SortOrder::Desc {
        c.reverse()
    } else {
        c
    }
}

#[cfg(feature = "json")]
fn json_sort_key(v: &serde_json::Value) -> Option<f64> {
    v.as_f64()
}
