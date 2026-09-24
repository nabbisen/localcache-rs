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
use crate::db::repository;
use crate::error::LocalFileCacheError;

mod execution;
use execution::{PreparedPathFilters, describe_query_plan};
pub(crate) use execution::{execute_query, execute_report};

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
// QueryReport (always available)
// ---------------------------------------------------------------------------

/// The result of [`QueryBuilder::run_report`]: the entries a query returns,
/// and the entries it passed over because they could not be decoded.
///
/// `run()` returns only [`entries`](Self::entries); it discards
/// [`skipped`](Self::skipped). A query under the wrong encryption key or with
/// a corrupted payload therefore returns fewer entries, or none, without
/// saying why. `run_report()` says why.
#[derive(Debug)]
#[non_exhaustive]
pub struct QueryReport<T> {
    /// The entries the query returns: exactly what `run()` returns for the
    /// same query.
    pub entries: Vec<CacheEntry<T>>,
    /// Entries the query passed over because they could not be decoded, with
    /// the error each produced, in scan order.
    ///
    /// * **Without a payload predicate or a payload-field sort**, this lists
    ///   every undecodable entry the scan passed while producing the page,
    ///   including those passed while skipping `offset` entries. Entries
    ///   after the page are never examined, so they are not listed.
    /// * **With a payload predicate or a payload-field sort**, every
    ///   candidate's payload must be decoded to be tested or ordered, so an
    ///   undecodable entry is listed whether or not it would have matched.
    /// * An entry whose payload row is missing produces no decode error, so
    ///   it is not listed. `get` also treats it as a miss.
    pub skipped: Vec<SkippedEntry>,
}

/// One entry a query could not decode. See [`QueryReport::skipped`].
#[derive(Debug)]
#[non_exhaustive]
pub struct SkippedEntry {
    /// The stored path of the entry.
    pub path: PathBuf,
    /// The error decoding its payload produced.
    pub error: LocalFileCacheError,
}

// ---------------------------------------------------------------------------
// SortOrder (always available)
// ---------------------------------------------------------------------------

/// Sort direction for [`QueryBuilder::order_by`] and [`QueryBuilder::then_by`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    /// Ascending (smallest first).
    Asc,
    /// Descending (largest first).
    Desc,
}

/// What [`QueryBuilder::order_by`] and [`QueryBuilder::then_by`] sort by.
///
/// There is deliberately no variant for the `updated_at` column: nothing has
/// asked to sort by it, and the old `order_by_updated_at` sorted by the source
/// file's modification time, not by `updated_at`. `#[non_exhaustive]` leaves
/// room for a distinctly named key later.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SortKey {
    /// A dot-separated field of the JSON payload (requires the `json` feature).
    #[cfg(feature = "json")]
    Field(String),
    /// The source file's modification time (`mtime`) as recorded when the
    /// entry was stored — not when the entry was written to the cache.
    Mtime,
    /// The `last_accessed_at` timestamp: the last **read** (`get`,
    /// `get_if_fresh`, or `touch`). Entries never read since being written
    /// have `last_accessed_at == 0` and sort as oldest under
    /// [`SortOrder::Asc`]. This is the same ordering `max_entries` eviction
    /// uses.
    LastAccessed,
    /// The stored path string.
    Path,
}

// ---------------------------------------------------------------------------
// OrderBy specification
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) enum OrderBy {
    /// Sort by a JSON payload field (requires `json` feature).
    #[cfg(feature = "json")]
    Field { path: String, order: SortOrder },
    /// Sort by the source file's `mtime`, as recorded when the entry was stored.
    Mtime(SortOrder),
    /// Sort by `last_accessed_at` timestamp.
    LastAccessed(SortOrder),
    /// Sort by stored path string.
    Path(SortOrder),
}

impl OrderBy {
    fn from_key(key: SortKey, order: SortOrder) -> Self {
        match key {
            #[cfg(feature = "json")]
            SortKey::Field(path) => OrderBy::Field { path, order },
            SortKey::Mtime => OrderBy::Mtime(order),
            SortKey::LastAccessed => OrderBy::LastAccessed(order),
            SortKey::Path => OrderBy::Path(order),
        }
    }
}

impl SortOrder {
    /// Maps the deprecated `ascending: bool` spelling onto a direction.
    fn from_ascending(ascending: bool) -> Self {
        if ascending {
            SortOrder::Asc
        } else {
            SortOrder::Desc
        }
    }
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
    ///
    /// `\` is the pattern's escape character: a literal `%`, `_`, or `\`
    /// must be written as `\%`, `\_`, or `\\`. This matters for Windows
    /// paths, whose separator is `\`.
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
    /// Force the query to use a specific index via SQLite's `INDEXED BY`.
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
    #[deprecated(
        since = "0.21.5",
        note = "the index duplicates the built-in unique index on (namespace, path) and cannot speed up a query"
    )]
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

    /// Sort results by `key`, in `order`, as the primary key.
    ///
    /// Clears any previous sort keys. Chain with [`then_by`](Self::then_by)
    /// for secondary sorting.
    ///
    /// ```no_run
    /// # use localcache::{CacheEngine, SortKey, SortOrder};
    /// # let engine = CacheEngine::<Vec<f32>>::builder().database(":memory:").build()?;
    /// let newest_first = engine
    ///     .query()
    ///     .order_by(SortKey::Mtime, SortOrder::Desc)
    ///     .then_by(SortKey::Path, SortOrder::Asc)
    ///     .run()?;
    /// # Ok::<(), localcache::LocalFileCacheError>(())
    /// ```
    pub fn order_by(mut self, key: SortKey, order: SortOrder) -> Self {
        self.order_by = vec![OrderBy::from_key(key, order)];
        self
    }

    /// Add a secondary sort by `key`, in `order`.
    ///
    /// Call after [`order_by`](Self::order_by).
    pub fn then_by(mut self, key: SortKey, order: SortOrder) -> Self {
        self.order_by.push(OrderBy::from_key(key, order));
        self
    }

    /// Sort results by a dot-separated JSON payload field (requires `json` feature).
    #[cfg(feature = "json")]
    #[deprecated(
        since = "0.21.5",
        note = "use order_by(SortKey::Field(path), SortOrder::Asc) or SortOrder::Desc"
    )]
    pub fn order_by_field(self, field_path: impl Into<String>, ascending: bool) -> Self {
        self.order_by(
            SortKey::Field(field_path.into()),
            SortOrder::from_ascending(ascending),
        )
    }

    /// Sorts by the source file's `mtime`, **not** by the `updated_at` column
    /// its name suggests.
    #[deprecated(
        since = "0.21.5",
        note = "sorts by the source file's mtime, not updated_at; use order_by(SortKey::Mtime, SortOrder::Asc) or SortOrder::Desc"
    )]
    pub fn order_by_updated_at(self, ascending: bool) -> Self {
        self.order_by(SortKey::Mtime, SortOrder::from_ascending(ascending))
    }

    /// Sort results by the last **read** timestamp (primary key).
    #[deprecated(
        since = "0.21.5",
        note = "use order_by(SortKey::LastAccessed, SortOrder::Asc) or SortOrder::Desc"
    )]
    pub fn order_by_last_accessed(self, ascending: bool) -> Self {
        self.order_by(SortKey::LastAccessed, SortOrder::from_ascending(ascending))
    }

    /// Sort results by the stored path string (primary key).
    #[deprecated(
        since = "0.21.5",
        note = "use order_by(SortKey::Path, SortOrder::Asc) or SortOrder::Desc"
    )]
    pub fn order_by_path(self, ascending: bool) -> Self {
        self.order_by(SortKey::Path, SortOrder::from_ascending(ascending))
    }

    /// Add a secondary sort by a JSON payload field (requires `json` feature).
    #[cfg(feature = "json")]
    #[deprecated(
        since = "0.21.5",
        note = "use then_by(SortKey::Field(path), SortOrder::Asc) or SortOrder::Desc"
    )]
    pub fn then_by_field(self, field_path: impl Into<String>, ascending: bool) -> Self {
        self.then_by(
            SortKey::Field(field_path.into()),
            SortOrder::from_ascending(ascending),
        )
    }

    /// Adds a secondary sort by the source file's `mtime`, **not** by the
    /// `updated_at` column its name suggests.
    #[deprecated(
        since = "0.21.5",
        note = "sorts by the source file's mtime, not updated_at; use then_by(SortKey::Mtime, SortOrder::Asc) or SortOrder::Desc"
    )]
    pub fn then_by_updated_at(self, ascending: bool) -> Self {
        self.then_by(SortKey::Mtime, SortOrder::from_ascending(ascending))
    }

    /// Add a secondary sort by the last **read** timestamp.
    #[deprecated(
        since = "0.21.5",
        note = "use then_by(SortKey::LastAccessed, SortOrder::Asc) or SortOrder::Desc"
    )]
    pub fn then_by_last_accessed(self, ascending: bool) -> Self {
        self.then_by(SortKey::LastAccessed, SortOrder::from_ascending(ascending))
    }

    /// Add a secondary sort by path.
    #[deprecated(
        since = "0.21.5",
        note = "use then_by(SortKey::Path, SortOrder::Asc) or SortOrder::Desc"
    )]
    pub fn then_by_path(self, ascending: bool) -> Self {
        self.then_by(SortKey::Path, SortOrder::from_ascending(ascending))
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
    ///
    /// An entry whose payload cannot be decoded is left out of the result
    /// without an error: under the wrong encryption key, a query returns an
    /// empty `Vec`. Use [`run_report`](Self::run_report) to see what was
    /// left out and why.
    ///
    /// **From v0.22.0**, `run()` will return the first undecodable entry's
    /// error instead, as `get` does; `run_report()` will remain the way to
    /// tolerate them. See RFC 024 B1.
    pub fn run(self) -> Result<Vec<CacheEntry<T>>, LocalFileCacheError> {
        execute_query(self)
    }

    /// Execute the query and report the entries it could not decode.
    ///
    /// `entries` is exactly what [`run`](Self::run) returns; `skipped` lists
    /// each undecodable entry with its error. `offset` and `limit` count
    /// returned entries only, as for `run()`.
    ///
    /// ```no_run
    /// use localcache::CacheEngine;
    ///
    /// let engine = CacheEngine::<Vec<f32>>::builder()
    ///     .database("cache.sqlite3")
    ///     .build()?;
    ///
    /// let report = engine.query().path_like("%/docs/%").run_report()?;
    /// for skipped in &report.skipped {
    ///     eprintln!("{}: {}", skipped.path.display(), skipped.error);
    /// }
    /// println!("{} entries", report.entries.len());
    /// # Ok::<(), localcache::LocalFileCacheError>(())
    /// ```
    pub fn run_report(self) -> Result<QueryReport<T>, LocalFileCacheError> {
        execute_report(self)
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
