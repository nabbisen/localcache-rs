//! Payload query support.
//!
//! [`QueryBuilder`] provides a fluent interface for filtering cached entries
//! by inspecting their decoded JSON payloads.
//!
//! Predicates are evaluated against `serde_json::Value` representations of
//! the decoded payload, so they work with any codec.
//!
//! # Example
//!
//! ```no_run
//! use localcache::{CacheEngine, CacheOptions, Codec};
//! use serde::{Serialize, Deserialize};
//!
//! #[derive(Serialize, Deserialize)]
//! struct Doc { title: String, score: f64 }
//!
//! let engine = CacheEngine::<Doc>::builder()
//!     .database(":memory:")
//!     .codec(Codec::Json)
//!     .build()?;
//!
//! // …populate the engine…
//!
//! let results = engine.query()
//!     .field_gt("score", 0.9)
//!     .order_by_field("score", false)   // descending
//!     .limit(10)
//!     .run()?;
//!
//! for entry in results {
//!     println!("{}: {}", entry.path.display(), entry.payload.score);
//! }
//! # Ok::<(), localcache::LocalFileCacheError>(())
//! ```

use std::path::PathBuf;

use serde::{Serialize, de::DeserializeOwned};

use crate::cache::entry::CacheEntry;
use crate::detection::metadata::FileMetadata;
use crate::error::LocalFileCacheError;

// ---------------------------------------------------------------------------
// Order direction
// ---------------------------------------------------------------------------

/// Sort direction for [`QueryBuilder::order_by_field`].
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
    /// Sort by a dot-separated payload field (requires JSON-serialisable value).
    Field { path: String, order: SortOrder },
    /// Sort by `updated_at` metadata timestamp.
    UpdatedAt(SortOrder),
    /// Sort by stored `path` string.
    Path(SortOrder),
}

// ---------------------------------------------------------------------------
// Predicate type
// ---------------------------------------------------------------------------

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
    // Path filter
    // ------------------------------------------------------------------

    /// Restrict the search to entries whose stored path matches a SQLite
    /// `LIKE` pattern.
    pub fn path_like(mut self, pattern: impl Into<String>) -> Self {
        self.path_like = Some(pattern.into());
        self
    }

    // ------------------------------------------------------------------
    // Payload field predicates
    // ------------------------------------------------------------------

    /// Match entries where the JSON field at `field_path` equals `value`.
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
    pub fn field_gt(mut self, field_path: impl Into<String>, threshold: f64) -> Self {
        self.predicates.push(Predicate::FieldGt {
            path: field_path.into(),
            threshold,
        });
        self
    }

    /// Match entries where the numeric JSON field is less than `threshold`.
    pub fn field_lt(mut self, field_path: impl Into<String>, threshold: f64) -> Self {
        self.predicates.push(Predicate::FieldLt {
            path: field_path.into(),
            threshold,
        });
        self
    }

    /// Match entries where the string JSON field contains `substring`.
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

    /// Match entries where the entire payload (as JSON string) contains `needle`.
    pub fn payload_contains(mut self, needle: impl Into<String>) -> Self {
        self.predicates.push(Predicate::PayloadContains {
            needle: needle.into(),
        });
        self
    }

    // ------------------------------------------------------------------
    // Sorting
    // ------------------------------------------------------------------

    /// Sort results by a dot-separated payload field.
    ///
    /// `ascending = true` → smallest value first.
    ///
    /// Non-sortable entries (field absent or wrong type) are placed at the
    /// end of the result list.
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
    ///
    /// `ascending = true` → oldest written first.
    pub fn order_by_updated_at(mut self, ascending: bool) -> Self {
        self.order_by = Some(OrderBy::UpdatedAt(if ascending {
            SortOrder::Asc
        } else {
            SortOrder::Desc
        }));
        self
    }

    /// Sort results by the stored path string lexicographically.
    ///
    /// `ascending = true` → alphabetical order.
    pub fn order_by_path(mut self, ascending: bool) -> Self {
        self.order_by = Some(OrderBy::Path(if ascending {
            SortOrder::Asc
        } else {
            SortOrder::Desc
        }));
        self
    }

    // ------------------------------------------------------------------
    // Pagination
    // ------------------------------------------------------------------

    /// Return at most `n` matching entries after applying `offset`.
    pub fn limit(mut self, n: usize) -> Self {
        self.limit = Some(n);
        self
    }

    /// Skip the first `n` matching entries before applying `limit`.
    ///
    /// Useful for paginating through large result sets.
    pub fn offset(mut self, n: usize) -> Self {
        self.offset = n;
        self
    }

    // ------------------------------------------------------------------
    // Terminal
    // ------------------------------------------------------------------

    /// Execute the query and return matching [`CacheEntry`] values.
    ///
    /// All matching entries (after predicates, offset, and limit) are
    /// returned.  The `order_by` clause — if set — sorts the *full* matching
    /// set before slicing with `offset` and `limit`.
    pub fn run(self) -> Result<Vec<CacheEntry<T>>, LocalFileCacheError> {
        execute_query(self)
    }
}

// ---------------------------------------------------------------------------
// Shared execution (used by both sync run() and async wrapper)
// ---------------------------------------------------------------------------

pub(crate) fn execute_query<T>(
    q: QueryBuilder<'_, T>,
) -> Result<Vec<CacheEntry<T>>, LocalFileCacheError>
where
    T: Serialize + DeserializeOwned,
{
    use crate::db::repository;

    let paths = repository::keys(&q.engine.conn, &q.engine.namespace, q.path_like.as_deref())?;

    let mut matched: Vec<(CacheEntry<T>, serde_json::Value)> = Vec::new();

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

        // Evaluate predicates.
        let json_val = if !q.predicates.is_empty() || q.order_by.is_some() {
            match serde_json::to_value(&payload) {
                Ok(v) => v,
                Err(_) => continue,
            }
        } else {
            serde_json::Value::Null
        };

        if q.predicates.iter().all(|p| p.matches(&json_val)) {
            let entry = CacheEntry {
                path: PathBuf::from(&row.path),
                metadata: FileMetadata {
                    mtime: row.metadata.mtime,
                    file_size: row.metadata.file_size,
                    hash: row.metadata.hash.clone(),
                },
                payload,
            };
            matched.push((entry, json_val));
        }
    }

    // Apply ordering.
    if let Some(order) = &q.order_by {
        apply_order(&mut matched, order);
    }

    // Apply offset + limit.
    let start = q.offset.min(matched.len());
    let end = q
        .limit
        .map(|l| (start + l).min(matched.len()))
        .unwrap_or(matched.len());

    Ok(matched
        .into_iter()
        .skip(start)
        .take(end - start)
        .map(|(e, _)| e)
        .collect())
}

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
            let time_cmp = ea.metadata.mtime.cmp(&eb.metadata.mtime);
            let c = if time_cmp == std::cmp::Ordering::Equal {
                ea.path.cmp(&eb.path)
            } else {
                time_cmp
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

/// Convert a JSON value to an `f64`-based sort key so numeric and string
/// values can be ordered together (strings mapped to `f64::MAX`).
fn json_sort_key(v: &serde_json::Value) -> Option<f64> {
    v.as_f64()
}
