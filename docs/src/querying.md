# Querying the Cache

## Basic lookups

```rust
// Get by exact path (no freshness check).
let entry: Option<CacheEntry<T>> = engine.get("file.txt")?;

// Get only if file hasn't changed since caching.
let entry = engine.get_if_fresh("file.txt")?;

// Check freshness without loading the payload.
use localcache::CacheStatus;
let status = engine.check_status("file.txt")?; // Fresh | Stale | Missing

// Fast existence check (no payload load).
let exists: bool = engine.contains("file.txt")?;
```

## Bulk status checks

```rust
let paths = vec!["a.txt", "b.txt", "c.txt"];
let statuses = engine.check_status_batch(&paths);
// Vec<Result<CacheStatus, _>> in the same order as paths
```

## Listing entries

```rust
// All stored paths, sorted alphabetically.
let paths: Vec<PathBuf> = engine.keys(None)?;

// Filter with a SQL LIKE pattern.
let docs = engine.keys(Some("/data/docs/%"))?;

// Full metadata for all entries (no payload loaded).
let entries: Vec<EntryInfo> = engine.list_entries()?;
// EntryInfo has: path, metadata, encoding, payload_version,
//                updated_at, last_accessed_at

// Aggregate statistics.
let stats = engine.cache_stats()?;
println!("entries: {}", stats.total_entries);
println!("bytes:   {}", stats.total_payload_bytes);
```

## `QueryBuilder` (requires `json` feature)

`QueryBuilder` scans entries and filters by payload content.
Payloads are evaluated as `serde_json::Value`, so any codec works.

```rust
use localcache::{CacheEngine, Codec};

let engine = CacheEngine::<Article>::builder()
    .database("articles.sqlite3")
    .codec(Codec::Json)
    .build()?;

// Find high-scoring articles about Rust.
let results = engine.query()
    .field_gt("score", 0.8)
    .field_contains("title", "Rust")
    .order_by_field("score", false)  // descending
    .limit(10)
    .offset(0)
    .run()?;
```

### Path filters *(always available)*

| Method | Description |
|---|---|
| `.path_like(pattern)` | SQL `LIKE` pattern on stored path (`%` = any sequence, `_` = one char) |
| `.path_in_dir(dir, recursive)` | Exact directory scoping — no over-fetch, metacharacter-safe |
| `.path_glob(pattern)` | Glob on stored path: `*`, `?`, `{a,b}` alternation |
| `.index_hint(name)` | Nominate a SQLite index for the path-listing scan |
| `.dry_run()` | Return `EXPLAIN QUERY PLAN` output without loading payloads |

### Payload predicates *(require `json` feature)*

| Method | Description |
|---|---|
| `.field_eq(path, value)` | Field equals a JSON value |
| `.field_gt(path, n)` | Numeric field > threshold |
| `.field_lt(path, n)` | Numeric field < threshold |
| `.field_contains(path, s)` | String field contains substring |
| `.payload_contains(s)` | Full payload (as JSON string) contains substring |

```rust
// Single sort key.
engine.query().order_by_field("score", false).run()?;
engine.query().order_by_path(true).run()?;
engine.query().order_by_updated_at(false).run()?;
engine.query().order_by_last_accessed(false).run()?;

// Multi-column sort (primary + secondary).
engine.query()
    .order_by_field("category", true)
    .then_by_field("score", false)
    .then_by_path(true)
    .run()?;
```

### Pagination

```rust
let page_size = 20;
let page = 3;

let results = engine.query()
    .order_by_path(true)
    .offset(page * page_size)
    .limit(page_size)
    .run()?;
```

## Diagnosing stale entries

`explain()` returns a structured report of *why* an entry is fresh, stale,
or missing — useful for debugging and CLI tooling:

```rust
let diag = engine.explain("document.txt")?;
println!("status:  {:?}", diag.status);
println!("summary: {}", diag.summary);

if let Some(diff) = diag.metadata_diff {
    println!("mtime changed:     {}", diff.mtime_changed);
    println!("file_size changed: {}", diff.size_changed);
}
if let Some(ttl_rem) = diag.ttl_remaining_secs {
    println!("TTL remaining: {} s", ttl_rem);
}
```

## Namespace management

```rust
// List all namespaces in this database.
let namespaces: Vec<String> = engine.namespace_list()?;

// Copy all entries from one namespace into another.
let dst_engine = CacheEngine::<T>::builder()
    .database("cache.sqlite3")
    .namespace("v2")
    .build()?;
let copied = dst_engine.namespace_copy(&src_engine)?;
```

## Path indexes and index requirements

For large namespaces, create an additional index on `(namespace, path)` and
require a query to use it:

```rust
// Create a user index once:
let idx = engine.create_path_index("docs_idx")?;  // → "lc_user_docs_idx"

// Use it in a query:
let results = engine.query()
    .path_like("%/docs/%")
    .index_hint(&idx)     // SQLite: INDEXED BY "lc_user_docs_idx"
    .run()?;
```

`create_path_index` takes only a suffix. New suffixes must contain 1–64 ASCII
letters, digits, or underscores; the method adds `lc_user_` and returns the
full catalog name. Treat this input as untrusted. `drop_path_index` also takes
a suffix, while `index_hint` takes the full name. Use
`list_path_indexes()` to discover valid public names.

Despite the historical “hint” API name, SQLite `INDEXED BY` is a requirement:
there is no silent planner fallback. Both `run()` and `dry_run()` validate the
complete main-schema index shape immediately before preparing SQL. Missing,
malformed, non-localcache, TEMP, and attached-database indexes return
`LocalFileCacheError::UnsupportedFeature` with a non-echoing safety message.
The built-in names `idx_files_namespace_path` and `idx_files_lru` may also be
used when their expected schema-v5 shapes are intact. They are stable for the
v0.20.1 schema but are implementation indexes, so prefer names returned by
`list_path_indexes()` for application-controlled planning.

Databases created by earlier releases may contain structurally valid
`lc_user_` names outside the new creation grammar. Such legacy indexes remain
listable, usable as full-name requirements, idempotently discoverable through
`create_path_index`, and removable through `drop_path_index`. Once removed,
an out-of-grammar legacy spelling cannot be recreated through the public API.
Dropping a name absent from `main` returns `false`; same-named TEMP or attached
objects are never dropped.

## Explain plan / dry_run (v0.17.0)

Inspect the SQLite query plan before running a query — useful for
performance diagnostics and test assertions:

```rust
let plan = engine.query()
    .path_like("%/docs/%")
    .index_hint("lc_user_docs_idx")
    .dry_run()?;
// → "SEARCH files USING INDEX lc_user_docs_idx ..."
println!("{plan}");
```

`dry_run()` runs `EXPLAIN QUERY PLAN` on the path-listing SQL only — no
payloads are loaded, and the cache is not modified.

With `AsyncCacheEngine`:

```rust
let plan = engine.query_dry_run(|q| q.path_like("%/docs/%")).await?;
```

## Directory-scoped queries (v0.18.0)

Two new predicates push directory filtering into SQL, avoiding over-fetch:

### `path_in_dir`

```rust
// Direct children only (no subdirectories):
let images = engine.query()
    .path_in_dir("/media/photos/2025", false)
    .run()?;

// Full subtree:
let all = engine.query()
    .path_in_dir("/media/photos", true)
    .run()?;
```

Directory names containing `%`, `_`, or `\` are escaped automatically.
A directory that no longer exists on disk is matched against stored entries
using its path string verbatim — deleted-directory queries still work.

### `path_glob`

```rust
// Match .txt and .md anywhere in the cache:
let docs = engine.query()
    .path_glob("*.{txt,md}")
    .run()?;

// Match files exactly one directory under /data:
let top = engine.query()
    .path_glob("/data/?/*.bin")
    .run()?;
```

Supported: `*` (any sequence), `?` (one character), `{a,b}` alternation.
A literal `[` in a pattern matches `[` (character classes are unsupported).

### Combining predicates

```rust
// Only .txt files directly in /data/docs (not in subdirectories):
let results = engine.query()
    .path_in_dir("/data/docs", false)
    .path_glob("*.txt")
    .run()?;
```

Both predicates compose with `index_hint` and `dry_run`.
