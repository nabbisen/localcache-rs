//! Payload query support.
//!
//! [`QueryBuilder`] provides a fluent interface for filtering cached entries.
//!
//! Payload predicates (`field_gt`, `field_lt`, etc.) and sorting by payload
//! fields require the `json` Cargo feature.  Path-based filtering (`path_like`)
//! and result pagination (`limit`, `offset`) are always available.
//!
//! # Example
//!
//! ```no_run
//! use localcache::{CacheEngine, CacheOptions};
//!
//! let engine = CacheEngine::<Vec<f32>>::builder()
//!     .database(":memory:")
//!     .build()?;
//!
//! // …populate the engine…
//!
//! // Path-based query (always available)
//! let results = engine.query()
//!     .path_like("%/docs/%")
//!     .limit(10)
//!     .run()?;
//! # Ok::<(), localcache::LocalFileCacheError>(())
//! ```

use std::path::PathBuf;

use serde::{Serialize, de::DeserializeOwned};

use crate::cache::entry::CacheEntry;
use crate::db::repository::{self, CandidateRow};
use crate::detection::metadata::FileMetadata;
use crate::error::LocalFileCacheError;

#[cfg(all(test, feature = "json"))]
#[path = "query/tests.rs"]
mod tests;

/// Local wrapper around [`crate::cache::engine::decode_with`], the one
/// choke point every RFC 021 tier routes payload decoding through. Adds a
/// test-only call counter so `cache/query/tests.rs` can assert "decode
/// count is bounded by `limit`, not namespace size" observably rather than
/// by inferring it from timing — reset with `DECODE_CALLS.with(|c|
/// c.set(0))` before a query under test.
fn decode_with<U: DeserializeOwned>(
    core: &crate::cache::engine::EngineCore<'_>,
    bytes: &[u8],
    encoding: &str,
) -> Result<U, LocalFileCacheError> {
    #[cfg(test)]
    DECODE_CALLS.with(|c| c.set(c.get() + 1));
    crate::cache::engine::decode_with(core, bytes, encoding)
}

#[cfg(test)]
thread_local! {
    pub(crate) static DECODE_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

// ---------------------------------------------------------------------------
// SortOrder (always available)
// ---------------------------------------------------------------------------

/// Sort direction for [`QueryBuilder::order_by_updated_at`] and
/// [`QueryBuilder::order_by_path`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    /// Ascending (smallest first).
    Asc,
    /// Descending (largest first).
    Desc,
}

// ---------------------------------------------------------------------------
// OrderBy specification
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) enum OrderBy {
    /// Sort by a JSON payload field (requires `json` feature).
    #[cfg(feature = "json")]
    Field { path: String, order: SortOrder },
    /// Sort by `mtime` timestamp proxy.
    UpdatedAt(SortOrder),
    /// Sort by `last_accessed_at` timestamp.
    LastAccessed(SortOrder),
    /// Sort by stored path string.
    Path(SortOrder),
}

// ---------------------------------------------------------------------------
// Predicate type (json feature only)
// ---------------------------------------------------------------------------

#[cfg(feature = "json")]
pub(crate) enum Predicate {
    FieldEq {
        path: String,
        value: serde_json::Value,
    },
    FieldGt {
        path: String,
        threshold: f64,
    },
    FieldLt {
        path: String,
        threshold: f64,
    },
    FieldContains {
        path: String,
        substring: String,
    },
    PayloadContains {
        needle: String,
    },
}

#[cfg(feature = "json")]
impl Predicate {
    fn matches(&self, value: &serde_json::Value) -> bool {
        match self {
            Predicate::FieldEq {
                path,
                value: expected,
            } => get_field(value, path) == Some(expected),
            Predicate::FieldGt { path, threshold } => get_field(value, path)
                .and_then(|v| v.as_f64())
                .map(|n| n > *threshold)
                .unwrap_or(false),
            Predicate::FieldLt { path, threshold } => get_field(value, path)
                .and_then(|v| v.as_f64())
                .map(|n| n < *threshold)
                .unwrap_or(false),
            Predicate::FieldContains { path, substring } => get_field(value, path)
                .and_then(|v| v.as_str())
                .map(|s| s.contains(substring.as_str()))
                .unwrap_or(false),
            Predicate::PayloadContains { needle } => serde_json::to_string(value)
                .map(|s| s.contains(needle.as_str()))
                .unwrap_or(false),
        }
    }
}

#[cfg(feature = "json")]
fn get_field<'a>(value: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let mut current = value;
    for key in path.split('.') {
        current = current.get(key)?;
    }
    Some(current)
}

// ---------------------------------------------------------------------------
// QueryBuilder
// ---------------------------------------------------------------------------

/// Fluent query builder for filtering and sorting cache entries.
///
/// Obtain one via [`crate::CacheEngine::query`].
pub struct QueryBuilder<'e, T> {
    pub(crate) core: crate::cache::engine::EngineCore<'e>,
    pub(crate) _phantom: std::marker::PhantomData<T>,
    #[cfg(feature = "json")]
    pub(crate) predicates: Vec<Predicate>,
    pub(crate) limit: Option<usize>,
    pub(crate) offset: usize,
    pub(crate) path_like: Option<String>,
    /// Nominates a specific SQLite index for the `files` table scan.
    pub(crate) index_hint: Option<String>,
    /// Raw directory filter; resolution is deferred to fallible terminals.
    pub(crate) path_in_dir: Option<(PathBuf, bool)>,
    /// Raw glob pattern; bounded compilation is deferred to fallible terminals.
    pub(crate) path_glob: Option<String>,
    /// Multiple sort keys applied in order (primary, secondary, …).
    pub(crate) order_by: Vec<OrderBy>,
}

impl<'e, T> QueryBuilder<'e, T>
where
    T: Serialize + DeserializeOwned,
{
    // ------------------------------------------------------------------
    // Path filter (always available)
    // ------------------------------------------------------------------

    /// Restrict to entries whose stored path matches a SQL LIKE pattern.
    pub fn path_like(mut self, pattern: impl Into<String>) -> Self {
        self.path_like = Some(pattern.into());
        self
    }

    /// Restrict to entries whose stored path lives **in `dir`**.
    ///
    /// `recursive = false` matches only **direct children** of `dir` (no
    /// subdirectories).  `recursive = true` matches the entire subtree.
    ///
    /// `dir` is resolved at `run()`/`dry_run()`: it is canonicalized when it
    /// exists, while a missing directory uses its exact path string so stored
    /// entries remain queryable. Other I/O failures propagate, and paths that
    /// cannot be represented as valid UTF-8 return `InvalidPath`.
    ///
    /// Characters that are special in SQL `LIKE` (backslash, `%`, `_`) are
    /// escaped automatically — directory names containing those characters
    /// match **literally**.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use localcache::CacheEngine;
    /// # let engine = CacheEngine::<Vec<f32>>::builder().database(":memory:").build()?;
    /// // Direct children only:
    /// let results = engine.query()
    ///     .path_in_dir("/data/docs", false)
    ///     .run()?;
    ///
    /// // All files in the subtree:
    /// let all = engine.query()
    ///     .path_in_dir("/data/docs", true)
    ///     .run()?;
    /// # Ok::<(), localcache::LocalFileCacheError>(())
    /// ```
    pub fn path_in_dir(mut self, dir: impl AsRef<std::path::Path>, recursive: bool) -> Self {
        self.path_in_dir = Some((dir.as_ref().to_path_buf(), recursive));
        self
    }

    /// Restrict to entries whose stored path matches a glob `pattern`.
    ///
    /// Uses the same dialect as [`crate::ScanOptions::glob_pattern`]:
    /// - `*` — any sequence of Unicode scalar values (including none)
    /// - `?` — exactly one Unicode scalar value
    /// - `{a,b,c}` — nested and multiple brace alternatives
    ///
    /// The match is applied to the **full stored path**, case-sensitively on
    /// every platform, without Unicode normalization.
    /// A literal `[` in a pattern is matched as-is; unlike the SQLite
    /// `GLOB` operator, character classes (`[abc]`) are not supported.
    ///
    /// Pattern validation is deferred to `run()`/`dry_run()`. Unmatched braces,
    /// NUL, and bounded safety-limit violations return `UnsupportedFeature`
    /// before database work.
    ///
    /// > Note: `*` and `?` in the pattern always act as wildcards.  If you
    /// > need a literal `*` or `?` in a path segment, use `path_like` with
    /// > SQL `LIKE` escaping instead.
    ///
    /// **Performance:** start the pattern with a literal, not `*`. A leading
    /// literal produces an indexable range and stays flat as the namespace
    /// grows; a leading `*` cannot, and scan cost grows with it. Prefer
    /// `path_glob("/data/*.json")` over `path_glob("*/*.json")` when the
    /// prefix is known. See `docs/src/performance.md` for measured numbers.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use localcache::CacheEngine;
    /// # let engine = CacheEngine::<Vec<f32>>::builder().database(":memory:").build()?;
    /// // Match all .txt and .md files anywhere in the cache:
    /// let docs = engine.query()
    ///     .path_glob("*.{txt,md}")
    ///     .run()?;
    /// # Ok::<(), localcache::LocalFileCacheError>(())
    /// ```
    pub fn path_glob(mut self, pattern: impl Into<String>) -> Self {
        self.path_glob = Some(pattern.into());
        self
    }
    ///
    /// Requires the full name of an allowed main-schema index. Both terminal
    /// operations validate its complete catalog shape before generating
    /// `INDEXED BY <name>`; missing or unauthorized names return
    /// [`LocalFileCacheError::UnsupportedFeature`] without planner fallback.
    ///
    /// Use [`crate::CacheEngine::list_path_indexes`] to discover available
    /// user-created indexes. The schema-v5 built-ins
    /// `idx_files_namespace_path` and `idx_files_lru` are also accepted while
    /// their complete expected shapes remain intact. SQLite treats
    /// `INDEXED BY` as a requirement, so this API never silently falls back
    /// to automatic planning.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use localcache::CacheEngine;
    /// # let engine = CacheEngine::<Vec<f32>>::builder().database(":memory:").build()?;
    /// let results = engine.query()
    ///     .path_like("%/docs/%")
    ///     .index_hint("lc_user_my_idx")
    ///     .run()?;
    /// # Ok::<(), localcache::LocalFileCacheError>(())
    /// ```
    pub fn index_hint(mut self, index_name: impl Into<String>) -> Self {
        self.index_hint = Some(index_name.into());
        self
    }

    /// Return the SQLite query plan without executing the query.
    ///
    /// Runs `EXPLAIN QUERY PLAN` on the path-listing SQL (with any
    /// configured [`index_hint`](QueryBuilder::index_hint) and
    /// [`path_like`](QueryBuilder::path_like) applied) and returns the
    /// human-readable plan, one line per step.
    ///
    /// No payloads are loaded and no cache entries are read.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use localcache::CacheEngine;
    /// # let engine = CacheEngine::<Vec<f32>>::builder().database(":memory:").build()?;
    /// let plan = engine.query()
    ///     .path_like("%/docs/%")
    ///     .dry_run()?;
    /// println!("{plan}");
    /// # Ok::<(), localcache::LocalFileCacheError>(())
    /// ```
    pub fn dry_run(self) -> Result<String, LocalFileCacheError> {
        let prepared = self.prepare_path_filters()?;
        let sql_plan = repository::explain_query(
            self.core.conn,
            self.core.namespace,
            self.path_like.as_deref(),
            self.index_hint.as_deref(),
            prepared.path_in_dir(),
            prepared.path_glob(),
        )?;
        let execution = describe_query_plan(&self)?;
        Ok(format!("{sql_plan}\n{execution}"))
    }

    // ------------------------------------------------------------------
    // Payload predicates (json feature)
    // ------------------------------------------------------------------

    /// Match entries where the JSON field at `field_path` equals `value`.
    #[cfg(feature = "json")]
    pub fn field_eq(
        mut self,
        field_path: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.predicates.push(Predicate::FieldEq {
            path: field_path.into(),
            value: value.into(),
        });
        self
    }

    /// Match entries where the numeric JSON field is greater than `threshold`.
    #[cfg(feature = "json")]
    pub fn field_gt(mut self, field_path: impl Into<String>, threshold: f64) -> Self {
        self.predicates.push(Predicate::FieldGt {
            path: field_path.into(),
            threshold,
        });
        self
    }

    /// Match entries where the numeric JSON field is less than `threshold`.
    #[cfg(feature = "json")]
    pub fn field_lt(mut self, field_path: impl Into<String>, threshold: f64) -> Self {
        self.predicates.push(Predicate::FieldLt {
            path: field_path.into(),
            threshold,
        });
        self
    }

    /// Match entries where the string JSON field contains `substring`.
    #[cfg(feature = "json")]
    pub fn field_contains(
        mut self,
        field_path: impl Into<String>,
        substring: impl Into<String>,
    ) -> Self {
        self.predicates.push(Predicate::FieldContains {
            path: field_path.into(),
            substring: substring.into(),
        });
        self
    }

    /// Match entries where the entire payload contains `needle`.
    #[cfg(feature = "json")]
    pub fn payload_contains(mut self, needle: impl Into<String>) -> Self {
        self.predicates.push(Predicate::PayloadContains {
            needle: needle.into(),
        });
        self
    }

    // ------------------------------------------------------------------
    // Sorting (always available)
    // ------------------------------------------------------------------

    /// Sort results by a dot-separated JSON payload field (requires `json` feature).
    ///
    /// Clears any previous sort keys and sets this as the primary key.
    /// Chain with `then_by_*` for secondary sorting.
    #[cfg(feature = "json")]
    pub fn order_by_field(mut self, field_path: impl Into<String>, ascending: bool) -> Self {
        self.order_by = vec![OrderBy::Field {
            path: field_path.into(),
            order: if ascending {
                SortOrder::Asc
            } else {
                SortOrder::Desc
            },
        }];
        self
    }

    /// Sort results by `updated_at` timestamp (primary key).
    pub fn order_by_updated_at(mut self, ascending: bool) -> Self {
        self.order_by = vec![OrderBy::UpdatedAt(if ascending {
            SortOrder::Asc
        } else {
            SortOrder::Desc
        })];
        self
    }

    /// Sort results by `last_accessed_at` timestamp (primary key).
    ///
    /// Entries never read since being written have `last_accessed_at == 0`
    /// and sort as oldest under ascending order.
    pub fn order_by_last_accessed(mut self, ascending: bool) -> Self {
        self.order_by = vec![OrderBy::LastAccessed(if ascending {
            SortOrder::Asc
        } else {
            SortOrder::Desc
        })];
        self
    }

    /// Sort results by the stored path string (primary key).
    pub fn order_by_path(mut self, ascending: bool) -> Self {
        self.order_by = vec![OrderBy::Path(if ascending {
            SortOrder::Asc
        } else {
            SortOrder::Desc
        })];
        self
    }

    /// Add a secondary sort by a JSON payload field (requires `json` feature).
    ///
    /// Call after one of the `order_by_*` methods.
    #[cfg(feature = "json")]
    pub fn then_by_field(mut self, field_path: impl Into<String>, ascending: bool) -> Self {
        self.order_by.push(OrderBy::Field {
            path: field_path.into(),
            order: if ascending {
                SortOrder::Asc
            } else {
                SortOrder::Desc
            },
        });
        self
    }

    /// Add a secondary sort by `updated_at`.
    pub fn then_by_updated_at(mut self, ascending: bool) -> Self {
        self.order_by.push(OrderBy::UpdatedAt(if ascending {
            SortOrder::Asc
        } else {
            SortOrder::Desc
        }));
        self
    }

    /// Add a secondary sort by `last_accessed_at`.
    pub fn then_by_last_accessed(mut self, ascending: bool) -> Self {
        self.order_by.push(OrderBy::LastAccessed(if ascending {
            SortOrder::Asc
        } else {
            SortOrder::Desc
        }));
        self
    }

    /// Add a secondary sort by path.
    pub fn then_by_path(mut self, ascending: bool) -> Self {
        self.order_by.push(OrderBy::Path(if ascending {
            SortOrder::Asc
        } else {
            SortOrder::Desc
        }));
        self
    }

    // ------------------------------------------------------------------
    // Pagination (always available)
    // ------------------------------------------------------------------

    /// Return at most `n` matching entries.
    pub fn limit(mut self, n: usize) -> Self {
        self.limit = Some(n);
        self
    }

    /// Skip the first `n` matching entries.
    pub fn offset(mut self, n: usize) -> Self {
        self.offset = n;
        self
    }

    // ------------------------------------------------------------------
    // Terminal
    // ------------------------------------------------------------------

    /// Execute the query.
    pub fn run(self) -> Result<Vec<CacheEntry<T>>, LocalFileCacheError> {
        execute_query(self)
    }

    fn prepare_path_filters(&self) -> Result<PreparedPathFilters, LocalFileCacheError> {
        let path_in_dir = self
            .path_in_dir
            .as_ref()
            .map(|(dir, recursive)| {
                let resolved = match crate::path::normalize_path(dir) {
                    Ok(canonical) => canonical,
                    Err(LocalFileCacheError::FileNotFound { .. }) => dir.clone(),
                    Err(error) => return Err(error),
                };
                let mut prefix = crate::path::path_to_str(&resolved)?.to_owned();
                if !prefix.ends_with(std::path::MAIN_SEPARATOR) {
                    prefix.push(std::path::MAIN_SEPARATOR);
                }
                Ok((prefix, *recursive))
            })
            .transpose()?;

        let path_glob = self
            .path_glob
            .as_deref()
            .map(crate::cache::glob::compile)
            .transpose()?;

        Ok(PreparedPathFilters {
            path_in_dir,
            path_glob,
        })
    }
}

struct PreparedPathFilters {
    path_in_dir: Option<(String, bool)>,
    path_glob: Option<crate::cache::glob::CompiledGlob>,
}

impl PreparedPathFilters {
    fn path_in_dir(&self) -> Option<(&str, bool)> {
        self.path_in_dir
            .as_ref()
            .map(|(prefix, recursive)| (prefix.as_str(), *recursive))
    }

    fn path_glob(&self) -> Option<&[String]> {
        self.path_glob
            .as_ref()
            .map(crate::cache::glob::CompiledGlob::sqlite_alternatives)
    }
}

#[cfg(feature = "json")]
pub(crate) fn execute_query<T>(
    q: QueryBuilder<'_, T>,
) -> Result<Vec<CacheEntry<T>>, LocalFileCacheError>
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
pub(crate) fn execute_query<T>(
    q: QueryBuilder<'_, T>,
) -> Result<Vec<CacheEntry<T>>, LocalFileCacheError>
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
) -> Result<Vec<CacheEntry<T>>, LocalFileCacheError>
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
    for row in rows {
        let (Some(content), Some(encoding)) = (row.content, row.encoding) else {
            continue;
        };
        let payload: T = match decode_with(&q.core, &content, &encoding) {
            Ok(p) => p,
            Err(_) => continue,
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
                Err(_) => continue,
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
    Ok(matched
        .into_iter()
        .skip(start)
        .take(end - start)
        .map(|(e, _, _)| e)
        .collect())
}

/// RFC 021 pass 2: materialize `candidates[order[..]]`, decoding payloads
/// only for the rows that survive `offset`/`limit`. A row whose payload is
/// missing or fails to decode is skipped and backfilled from the next
/// candidate in `order`, matching today's behaviour of returning up to
/// `limit` successfully-decoded rows rather than a short page — the skip
/// just moves after the (cheap, payload-free) sort instead of before it.
/// `candidates`/`order` were already fully materialized in memory by pass 1
/// to make that sort possible, so backfilling costs no extra SQL beyond the
/// occasional additional `payloads_for_ids` chunk.
fn materialize<T>(
    q: &QueryBuilder<'_, T>,
    candidates: &[CandidateRow],
    order: &[usize],
) -> Result<Vec<CacheEntry<T>>, LocalFileCacheError>
where
    T: Serialize + DeserializeOwned,
{
    use std::collections::HashMap;

    let start = q.offset.min(order.len());
    let target = q.limit.unwrap_or(order.len().saturating_sub(start));
    let mut out = Vec::new();
    let mut idx = start;
    while out.len() < target && idx < order.len() {
        let need = target - out.len();
        let window_end = (idx + need).min(order.len());
        let window = &order[idx..window_end];
        let ids: Vec<i64> = window.iter().map(|&i| candidates[i].id).collect();
        let payload_rows = repository::payloads_for_ids(q.core.conn, &ids)?;
        let mut payload_map: HashMap<i64, (Vec<u8>, String)> = payload_rows
            .into_iter()
            .map(|(id, content, encoding)| (id, (content, encoding)))
            .collect();
        for &i in window {
            let c = &candidates[i];
            let Some((content, encoding)) = payload_map.remove(&c.id) else {
                continue;
            };
            let payload: T = match decode_with(&q.core, &content, &encoding) {
                Ok(p) => p,
                Err(_) => continue,
            };
            out.push(CacheEntry {
                path: PathBuf::from(&c.path),
                metadata: FileMetadata {
                    mtime: c.mtime,
                    file_size: c.file_size,
                    hash: c.hash.clone(),
                },
                payload,
            });
        }
        idx = window_end;
    }
    Ok(out)
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
fn describe_query_plan<T>(q: &QueryBuilder<'_, T>) -> Result<String, LocalFileCacheError> {
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
fn describe_query_plan<T>(_q: &QueryBuilder<'_, T>) -> Result<String, LocalFileCacheError> {
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
        OrderBy::UpdatedAt(ord) => ord_dir(a.mtime.cmp(&b.mtime), *ord),
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
        OrderBy::UpdatedAt(ord) => ord_dir(a.mtime.cmp(&b.mtime), *ord),
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
        OrderBy::UpdatedAt(ord) => ord_dir(ea.metadata.mtime.cmp(&eb.metadata.mtime), *ord),
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
