//! Payload query support.
//!
//! [`QueryBuilder`] provides a fluent interface for filtering cached entries
//! by inspecting their decoded JSON payloads.  It is available when **both**
//! the `json` feature is enabled and the payload type `T` implements
//! [`serde::Serialize`] (so it can be round-tripped through `serde_json::Value`
//! for field access).
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
//! // Find entries whose JSON payload contains "score" > 0.9
//! let results = engine.query()
//!     .field_gt("score", 0.9)
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
// Predicate type
// ---------------------------------------------------------------------------

/// A predicate applied to a `serde_json::Value` payload.
pub(crate) enum Predicate {
    /// The JSON field at `path` (dot-separated, e.g. `"meta.score"`) must
    /// equal `value`.
    FieldEq {
        path: String,
        value: serde_json::Value,
    },
    /// The field value must be a number greater than `threshold`.
    FieldGt { path: String, threshold: f64 },
    /// The field value must be a number less than `threshold`.
    FieldLt { path: String, threshold: f64 },
    /// The field (as a string) must contain `substring`.
    FieldContains { path: String, substring: String },
    /// The JSON payload (serialised to a string) must contain `needle`.
    PayloadContains { needle: String },
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

/// Traverse a dot-separated field path in a JSON value.
///
/// `"meta.score"` looks up `value["meta"]["score"]`.
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

/// Fluent query builder for filtering cache entries by payload content.
///
/// Obtain one via [`crate::CacheEngine::query`].
pub struct QueryBuilder<'e, T> {
    pub(crate) engine: &'e crate::cache::engine::CacheEngine<T>,
    pub(crate) predicates: Vec<Predicate>,
    pub(crate) limit: Option<usize>,
    pub(crate) path_like: Option<String>,
}

impl<'e, T> QueryBuilder<'e, T>
where
    T: Serialize + DeserializeOwned,
{
    // ------------------------------------------------------------------
    // Path filter
    // ------------------------------------------------------------------

    /// Restrict the search to entries whose stored path matches a SQLite
    /// `LIKE` pattern (`%` = any sequence, `_` = one character).
    ///
    /// Example: `.path_like("%/2024/%")` to match paths containing `/2024/`.
    pub fn path_like(mut self, pattern: impl Into<String>) -> Self {
        self.path_like = Some(pattern.into());
        self
    }

    // ------------------------------------------------------------------
    // Payload field predicates
    // ------------------------------------------------------------------

    /// Match entries where the JSON field at `field_path` equals `value`.
    ///
    /// `field_path` is dot-separated (e.g. `"meta.author"`).
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

    /// Match entries where the numeric JSON field at `field_path` is greater
    /// than `threshold`.
    pub fn field_gt(mut self, field_path: impl Into<String>, threshold: f64) -> Self {
        self.predicates.push(Predicate::FieldGt {
            path: field_path.into(),
            threshold,
        });
        self
    }

    /// Match entries where the numeric JSON field at `field_path` is less
    /// than `threshold`.
    pub fn field_lt(mut self, field_path: impl Into<String>, threshold: f64) -> Self {
        self.predicates.push(Predicate::FieldLt {
            path: field_path.into(),
            threshold,
        });
        self
    }

    /// Match entries where the string JSON field at `field_path` contains
    /// `substring`.
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

    /// Match entries where the entire payload (serialised to JSON string)
    /// contains `needle`.
    ///
    /// This is a coarse but convenient full-payload text search.
    pub fn payload_contains(mut self, needle: impl Into<String>) -> Self {
        self.predicates.push(Predicate::PayloadContains {
            needle: needle.into(),
        });
        self
    }

    // ------------------------------------------------------------------
    // Limit
    // ------------------------------------------------------------------

    /// Return at most `n` matching entries.
    pub fn limit(mut self, n: usize) -> Self {
        self.limit = Some(n);
        self
    }

    // ------------------------------------------------------------------
    // Terminal
    // ------------------------------------------------------------------

    /// Execute the query and return all matching [`CacheEntry`] values.
    ///
    /// Every entry in the namespace (subject to `path_like`) is loaded and
    /// its payload deserialised.  Entries whose decoding fails are silently
    /// skipped.  Payload predicates are evaluated against a
    /// `serde_json::Value` representation of the decoded payload.
    ///
    /// This is a linear scan — suitable for small-to-medium caches.
    pub fn run(self) -> Result<Vec<CacheEntry<T>>, LocalFileCacheError> {
        use crate::db::repository;

        let paths = repository::keys(
            &self.engine.conn,
            &self.engine.namespace,
            self.path_like.as_deref(),
        )?;

        let mut results = Vec::new();

        for path in &paths {
            let path_str = match path.to_str() {
                Some(s) => s,
                None => continue,
            };

            let row =
                match repository::find_file(&self.engine.conn, &self.engine.namespace, path_str)? {
                    Some(r) => r,
                    None => continue,
                };

            let payload_row = match repository::load_payload(&self.engine.conn, row.id)? {
                Some(p) => p,
                None => continue,
            };

            // Decode payload; skip entries that fail decoding.
            let payload: T = match self
                .engine
                .decode_pub(&payload_row.content, &payload_row.encoding)
            {
                Ok(p) => p,
                Err(_) => continue,
            };

            // Apply predicates via serde_json::Value.
            if !self.predicates.is_empty() {
                let json_val = match serde_json::to_value(&payload) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if !self.predicates.iter().all(|p| p.matches(&json_val)) {
                    continue;
                }
            }

            results.push(CacheEntry {
                path: PathBuf::from(&row.path),
                metadata: FileMetadata {
                    mtime: row.metadata.mtime,
                    file_size: row.metadata.file_size,
                    hash: row.metadata.hash.clone(),
                },
                payload,
            });

            if self.limit.is_some_and(|limit| results.len() >= limit) {
                break;
            }
        }

        Ok(results)
    }
}
