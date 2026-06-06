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
use crate::detection::metadata::FileMetadata;
use crate::error::LocalFileCacheError;

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
    pub(crate) engine: &'e crate::cache::engine::CacheEngine<T>,
    #[cfg(feature = "json")]
    pub(crate) predicates: Vec<Predicate>,
    pub(crate) limit: Option<usize>,
    pub(crate) offset: usize,
    pub(crate) path_like: Option<String>,
    /// Nominates a specific SQLite index for the `files` table scan.
    pub(crate) index_hint: Option<String>,
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

    /// Suggest a specific SQLite index for the `files` table scan.
    ///
    /// Generates `INDEXED BY <name>` in the path-listing SQL.  If the
    /// named index does not exist, [`QueryBuilder::run`] returns
    /// `Err(`[`LocalFileCacheError::Database`]`)`.
    ///
    /// Use [`CacheEngine::list_path_indexes`] to discover available
    /// user-created indexes.
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
        use crate::db::repository;
        repository::explain_query(
            &self.engine.conn,
            &self.engine.namespace,
            self.path_like.as_deref(),
            self.index_hint.as_deref(),
        )
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
}

pub(crate) fn execute_query<T>(
    q: QueryBuilder<'_, T>,
) -> Result<Vec<CacheEntry<T>>, LocalFileCacheError>
where
    T: Serialize + DeserializeOwned,
{
    use crate::db::repository;

    let paths = repository::keys(
        &q.engine.conn,
        &q.engine.namespace,
        q.path_like.as_deref(),
        q.index_hint.as_deref(),
    )?;

    // Tuple: (entry, json_value_or_null, last_accessed_at)
    #[cfg(feature = "json")]
    let mut matched: Vec<(CacheEntry<T>, serde_json::Value, i64)> = Vec::new();
    #[cfg(not(feature = "json"))]
    let mut matched: Vec<(CacheEntry<T>, i64)> = Vec::new();

    for path in &paths {
        let path_str = match path.to_str() {
            Some(s) => s,
            None => continue,
        };

        let row = match repository::find_file(&q.engine.conn, &q.engine.namespace, path_str)? {
            Some(r) => r,
            None => continue,
        };

        let payload_row = match repository::load_payload(&q.engine.conn, row.id)? {
            Some(p) => p,
            None => continue,
        };

        let payload: T = match q
            .engine
            .decode_pub(&payload_row.content, &payload_row.encoding)
        {
            Ok(p) => p,
            Err(_) => continue,
        };

        let laa = row.last_accessed_at;

        let entry = CacheEntry {
            path: PathBuf::from(&row.path),
            metadata: FileMetadata {
                mtime: row.metadata.mtime,
                file_size: row.metadata.file_size,
                hash: row.metadata.hash.clone(),
            },
            payload,
        };

        #[cfg(feature = "json")]
        {
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

        #[cfg(not(feature = "json"))]
        matched.push((entry, laa));
    }

    // Multi-column ordering.
    #[cfg(feature = "json")]
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

    #[cfg(not(feature = "json"))]
    if !q.order_by.is_empty() {
        matched.sort_by(|(ea, la_a), (eb, la_b)| {
            for key in &q.order_by {
                let c = cmp_key_simple(ea, *la_a, eb, *la_b, key);
                if c != std::cmp::Ordering::Equal {
                    return c;
                }
            }
            std::cmp::Ordering::Equal
        });
    }

    // Offset + limit.
    let start = q.offset.min(matched.len());
    let end = q
        .limit
        .map(|l| (start + l).min(matched.len()))
        .unwrap_or(matched.len());

    #[cfg(feature = "json")]
    return Ok(matched
        .into_iter()
        .skip(start)
        .take(end - start)
        .map(|(e, _, _)| e)
        .collect());

    #[cfg(not(feature = "json"))]
    return Ok(matched
        .into_iter()
        .skip(start)
        .take(end - start)
        .map(|(e, _)| e)
        .collect());
}

// ---------------------------------------------------------------------------
// Per-key comparison helpers
// ---------------------------------------------------------------------------

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

#[allow(dead_code)]
fn cmp_key_simple<T>(
    ea: &CacheEntry<T>,
    la_a: i64,
    eb: &CacheEntry<T>,
    la_b: i64,
    key: &OrderBy,
) -> std::cmp::Ordering {
    match key {
        #[cfg(feature = "json")]
        OrderBy::Field { .. } => std::cmp::Ordering::Equal,
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
