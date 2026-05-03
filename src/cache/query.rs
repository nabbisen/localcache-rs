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
    pub(crate) order_by: Option<OrderBy>,
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
    #[cfg(feature = "json")]
    pub fn order_by_field(mut self, field_path: impl Into<String>, ascending: bool) -> Self {
        self.order_by = Some(OrderBy::Field {
            path: field_path.into(),
            order: if ascending {
                SortOrder::Asc
            } else {
                SortOrder::Desc
            },
        });
        self
    }

    /// Sort results by `updated_at` timestamp.
    pub fn order_by_updated_at(mut self, ascending: bool) -> Self {
        self.order_by = Some(OrderBy::UpdatedAt(if ascending {
            SortOrder::Asc
        } else {
            SortOrder::Desc
        }));
        self
    }

    /// Sort results by the stored path string.
    pub fn order_by_path(mut self, ascending: bool) -> Self {
        self.order_by = Some(OrderBy::Path(if ascending {
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

// ---------------------------------------------------------------------------
// Execution
// ---------------------------------------------------------------------------

pub(crate) fn execute_query<T>(
    q: QueryBuilder<'_, T>,
) -> Result<Vec<CacheEntry<T>>, LocalFileCacheError>
where
    T: Serialize + DeserializeOwned,
{
    use crate::db::repository;

    let paths = repository::keys(&q.engine.conn, &q.engine.namespace, q.path_like.as_deref())?;

    // When json feature is on, we collect (entry, json_value) for predicate eval.
    // When off, we collect (entry, ()) as a dummy.
    #[cfg(feature = "json")]
    let mut matched: Vec<(CacheEntry<T>, serde_json::Value)> = Vec::new();
    #[cfg(not(feature = "json"))]
    let mut matched: Vec<CacheEntry<T>> = Vec::new();

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
            let needs_json = !q.predicates.is_empty() || q.order_by.is_some();
            let json_val = if needs_json {
                match serde_json::to_value(&entry.payload) {
                    Ok(v) => v,
                    Err(_) => continue,
                }
            } else {
                serde_json::Value::Null
            };

            if q.predicates.iter().all(|p| p.matches(&json_val)) {
                matched.push((entry, json_val));
            }
        }

        #[cfg(not(feature = "json"))]
        {
            matched.push(entry);
        }
    }

    // Apply ordering.
    #[cfg(feature = "json")]
    if let Some(ref order) = q.order_by {
        apply_order(&mut matched, order);
    }
    #[cfg(not(feature = "json"))]
    if let Some(ref order) = q.order_by {
        apply_order_simple(&mut matched, order);
    }

    // Apply offset + limit.
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
        .map(|(e, _)| e)
        .collect());

    #[cfg(not(feature = "json"))]
    return Ok(matched.into_iter().skip(start).take(end - start).collect());
}

// ---------------------------------------------------------------------------
// Sorting helpers
// ---------------------------------------------------------------------------

#[cfg(feature = "json")]
fn apply_order<T>(matched: &mut [(CacheEntry<T>, serde_json::Value)], order: &OrderBy) {
    matched.sort_by(|(ea, va), (eb, vb)| match order {
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
        OrderBy::UpdatedAt(ord) => {
            let c = ea.metadata.mtime.cmp(&eb.metadata.mtime);
            let c = if c == std::cmp::Ordering::Equal {
                ea.path.cmp(&eb.path)
            } else {
                c
            };
            if *ord == SortOrder::Desc {
                c.reverse()
            } else {
                c
            }
        }
        OrderBy::Path(ord) => {
            let c = ea.path.cmp(&eb.path);
            if *ord == SortOrder::Desc {
                c.reverse()
            } else {
                c
            }
        }
    });
}

#[allow(dead_code)]
fn apply_order_simple<T>(matched: &mut [CacheEntry<T>], order: &OrderBy) {
    matched.sort_by(|ea, eb| match order {
        #[cfg(feature = "json")]
        OrderBy::Field { .. } => std::cmp::Ordering::Equal,
        OrderBy::UpdatedAt(ord) => {
            let c = ea.metadata.mtime.cmp(&eb.metadata.mtime);
            if *ord == SortOrder::Desc {
                c.reverse()
            } else {
                c
            }
        }
        OrderBy::Path(ord) => {
            let c = ea.path.cmp(&eb.path);
            if *ord == SortOrder::Desc {
                c.reverse()
            } else {
                c
            }
        }
    });
}

#[cfg(feature = "json")]
fn json_sort_key(v: &serde_json::Value) -> Option<f64> {
    v.as_f64()
}
