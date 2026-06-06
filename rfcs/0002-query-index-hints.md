# RFC 0002 — Query Index Hints and Explain Plan

| Field    | Value |
|----------|-------|
| Status   | Proposed |
| Feature  | *(core, no feature flag)* |
| Touches  | `src/cache/query.rs`, `src/db/repository.rs`, `src/cache/engine.rs` |

## Summary

Add two capabilities to `QueryBuilder`:

1. **Index hint** — let the caller nominate which SQLite index to use for a
   query, avoiding full table scans on large namespaces.
2. **Explain plan** — a `dry_run()` method that returns the SQLite query
   plan string (`EXPLAIN QUERY PLAN`) without executing the query.

## Motivation

`QueryBuilder` currently generates `SELECT` statements that SQLite's
planner optimises automatically.  For large caches (100k+ entries) with
payload-field predicates, the planner may choose a suboptimal plan.
Users who have created a path index via `create_path_index` have no way
to tell the query to prefer that index.

`EXPLAIN QUERY PLAN` is available in SQLite and surfaces planning
decisions — useful for diagnosing performance and for test assertions.

## Requirements

- `QueryBuilder::index_hint(name)` — optional; omitting it retains current
  auto-planner behaviour.
- `QueryBuilder::dry_run()` → `Result<String, …>` — returns the
  `EXPLAIN QUERY PLAN` output without touching the payload table.
- No feature flag required (both features are SQLite intrinsics, zero extra
  dependencies).
- The public `explain()` method on `CacheEngine` (already exists for
  staleness diagnosis) must not be confused with this query-level explain.
  Name the new method `dry_run` to differentiate.

## Design

### New `QueryBuilder` methods

```rust
impl<'e, T> QueryBuilder<'e, T> {
    /// Suggest a specific index for the `files` table scan.
    ///
    /// Generates `INDEXED BY <name>` in the SQL.  If the named index does
    /// not exist, SQLite returns an error at query time.
    ///
    /// # Example
    /// ```no_run
    /// # use localcache::CacheEngine;
    /// # let engine = CacheEngine::<Vec<f32>>::builder().database(":memory:").build()?;
    /// let results = engine.query()
    ///     .path_like("%/docs/%")
    ///     .index_hint("idx_files_path")
    ///     .run()?;
    /// # Ok::<(), localcache::LocalFileCacheError>(())
    /// ```
    pub fn index_hint(mut self, index_name: impl Into<String>) -> Self;

    /// Return the SQLite query plan without executing the query.
    ///
    /// Runs `EXPLAIN QUERY PLAN <generated SQL>` and returns the
    /// human-readable plan string, one line per step.
    ///
    /// # Example
    /// ```no_run
    /// # use localcache::CacheEngine;
    /// # let engine = CacheEngine::<Vec<f32>>::builder().database(":memory:").build()?;
    /// let plan = engine.query()
    ///     .path_like("%/docs/%")
    ///     .dry_run()?;
    /// println!("{plan}");
    /// # Ok::<(), localcache::LocalFileCacheError>(())
    /// ```
    pub fn dry_run(self) -> Result<String, LocalFileCacheError>;
}
```

### SQL generation changes — `query.rs`

**`index_hint`**

Add a field `index_hint: Option<String>` to `QueryBuilder`.  In `build_sql()`,
inject `INDEXED BY <name>` immediately after the table name:

```sql
-- without hint (current)
SELECT f.id, f.namespace, f.path, f.mtime, ...
FROM files f
JOIN payloads p ON p.file_id = f.id
WHERE f.namespace = ?
  AND f.path LIKE ?

-- with hint
SELECT f.id, f.namespace, f.path, f.mtime, ...
FROM files f INDEXED BY idx_files_path
JOIN payloads p ON p.file_id = f.id
WHERE f.namespace = ?
  AND f.path LIKE ?
```

**`dry_run`**

Reuse the existing `build_sql() -> (String, Vec<SqlParam>)` helper and
prepend `EXPLAIN QUERY PLAN `:

```rust
pub fn dry_run(self) -> Result<String, LocalFileCacheError> {
    let (sql, params) = self.build_sql();
    let explain_sql = format!("EXPLAIN QUERY PLAN {sql}");
    repository::explain_query(&self.engine.conn, &explain_sql, &params)
}
```

### New repository function — `repository.rs`

```rust
/// Run `EXPLAIN QUERY PLAN <sql>` and return the plan as a newline-joined string.
pub(crate) fn explain_query(
    conn: &Connection,
    explain_sql: &str,
    params: &[SqlParam],
) -> Result<String, LocalFileCacheError> {
    let mut stmt = conn.prepare_cached(explain_sql)?;
    let rows: Vec<String> = stmt
        .query_map(rusqlite::params_from_iter(params.iter().map(|p| p.as_sql())), |row| {
            // EXPLAIN QUERY PLAN columns: id, parent, notused, detail
            row.get::<_, String>(3)
        })?
        .collect::<Result<_, _>>()?;
    Ok(rows.join("\n"))
}
```

### `SqlParam` — unified parameter type

`QueryBuilder` currently builds `Vec<Box<dyn ToSql>>` inline.  To support
`dry_run` (which re-runs the query through a different path), extract a
small `enum SqlParam { Text(String), Real(f64), Int(i64) }` that
implements `ToSql`.  This is an internal change only; no public API
surface is affected.

### `AsyncCacheEngine` and `ConnectionPool`

```rust
// AsyncCacheEngine
pub async fn query_dry_run<F>(&self, f: F) -> Result<String, LocalFileCacheError>
where
    F: FnOnce(QueryBuilder<T>) -> QueryBuilder<T> + Send + 'static;

// ConnectionPool: delegate directly — QueryBuilder borrows &CacheEngine,
// so callers use the pool's inner lock directly (no change needed).
```

## Test plan

- `index_hint` with a valid index: query returns correct results.
- `index_hint` with an invalid index name: `run()` returns `Err(Database(_))`.
- `dry_run` on a simple `path_like` query: plan string contains `"SCAN"` or
  `"SEARCH"`.
- `dry_run` with `index_hint`: plan string mentions the hint index.
- `dry_run` does not load any payloads (entry count in DB unchanged).
- Async `query_dry_run` wrapper: plan returned correctly from spawned task.

## Security considerations

`INDEXED BY <name>` is parameterised through the query-builder field, not
interpolated directly from user input in a request path.  The index name
originates from calling code, not untrusted external data.  No SQL
injection risk beyond what already exists in `QueryBuilder::field_eq` etc.
