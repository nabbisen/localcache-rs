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

## `QueryBuilder`

`QueryBuilder` filters and sorts entries. Path filters, pagination, and
non-field sorting (by path, `updated_at`, or `last_accessed_at`) are always
available. Only payload predicates and sorting by a payload field
(`SortKey::Field`) require the `json` feature — see the tables below.
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
    .order_by(SortKey::Field("score".into()), SortOrder::Desc)  // descending
    .limit(10)
    .offset(0)
    .run()?;
```

### Path filters *(always available)*

| Method | Description |
|---|---|
| `.path_like(pattern)` | SQL `LIKE` pattern on stored path (`%` = any sequence, `_` = one char) |
| `.path_in_dir(dir, recursive)` | Exact directory scoping — no over-fetch, metacharacter-safe |
| `.path_glob(pattern)` | Case-sensitive Unicode-scalar glob on stored path: `*`, `?`, nested/multiple `{a,b}` alternatives |
| `.dry_run()` | Return `EXPLAIN QUERY PLAN` output, plus which execution path `run()` would take, without loading payloads |

`path_like`'s pattern uses `\` as its `LIKE` escape character. A literal `%`, `_`, or `\` must be
written as `\%`, `\_`, or `\\`. This matters for Windows paths, whose separator is `\`.

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
engine.query().order_by(SortKey::Field("score".into()), SortOrder::Desc).run()?;
engine.query().order_by(SortKey::Path, SortOrder::Asc).run()?;
engine.query().order_by(SortKey::Mtime, SortOrder::Desc).run()?;
engine.query().order_by(SortKey::LastAccessed, SortOrder::Desc).run()?;

// Multi-column sort (primary + secondary).
engine.query()
    .order_by(SortKey::Field("category".into()), SortOrder::Asc)
    .then_by(SortKey::Field("score".into()), SortOrder::Desc)
    .then_by(SortKey::Path, SortOrder::Asc)
    .run()?;
```

### Pagination

`offset` and `limit` count only entries that decode successfully; entries that cannot be decoded
are skipped, never counted.

```rust
let page_size = 20;
let page = 3;

let results = engine.query()
    .order_by(SortKey::Path, SortOrder::Asc)
    .offset(page * page_size)
    .limit(page_size)
    .run()?;
```

### Entries a query cannot decode: `run_report`

`run()` leaves out an entry whose payload cannot be decoded, and says nothing.
Under the wrong encryption key, every payload fails to decode, so a query
returns an empty `Vec` with no error. `run_report()` returns the same entries and
also lists what it left out and why:

```rust
let report = engine.query().path_like("%/docs/%").run_report()?;

// Exactly what run() would have returned.
for entry in &report.entries {
    println!("{}", entry.path.display());
}

// What run() would have dropped silently, with the error for each.
for skipped in &report.skipped {
    eprintln!("cannot decode {}: {}", skipped.path.display(), skipped.error);
}
```

`offset` and `limit` count returned entries only, as for `run()`. `skipped` lists
every undecodable entry the scan passed while producing the page, in scan
order, including those passed while skipping `offset` entries. Entries beyond the
page are never examined, so they are not listed. A query with a payload
predicate or a payload-field sort must decode every candidate, so there an
undecodable entry is listed whether or not it would have matched. An entry
whose payload row is missing produces no decode error and is not listed; `get`
treats it as a miss as well.

`SyncCacheEngine`, `ReadPool` and `AsyncCacheEngine` have `query_run_report`,
taking the same closure as their `query_run`.

**Change in v0.22.0.** `run()` will return the first undecodable entry's error,
as `get` does, instead of leaving the entry out. A query under the wrong key will
then fail with an error instead of returning nothing. To keep tolerating
undecodable entries, use `run_report()`: it is available now and will not change.
The decision is recorded in
[RFC 024](https://github.com/nabbisen/localcache-rs/blob/main/rfcs/accepted/024-api-consistency.md)
(B1).

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

// Copy all entries from one namespace (or database) into another.
let dst_engine = CacheEngine::<T>::builder()
    .database("cache.sqlite3")
    .namespace("v2")
    .build()?;
let copied = dst_engine.import_from(&src_engine)?;
```

## Path indexes

Earlier releases could create extra `lc_user_…` indexes on `(namespace, path)`.
They duplicate the built-in unique index on that pair, so they cannot make any
query faster, and creating them is deprecated. Nothing in a normal workflow
needs one. An index created by an earlier release keeps working and can be
removed; see the migration table in [API Overview](./api.md#migrating-from-deprecated-names).

## Explain plan / dry_run (v0.17.0)

Inspect the SQLite query plan before running a query — useful for
performance diagnostics and test assertions:

```rust
let plan = engine.query()
    .path_like("%/docs/%")
    .dry_run()?;
// → the EXPLAIN QUERY PLAN rows for the path-listing scan
println!("{plan}");
```

`dry_run()` runs `EXPLAIN QUERY PLAN` on the path-listing SQL, then appends which execution path
`run()` would actually take — no payloads are loaded, and the cache is not modified.

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

Supported: `*` (any sequence of Unicode scalar values), `?` (one scalar), and
nested or multiple `{a,b}` alternatives. Matching is case-sensitive on every
platform and performs no Unicode normalization. A literal `[` matches `[`
(character classes are unsupported).

Unmatched braces, NUL, and bounded pattern-expansion violations return a
stable `UnsupportedFeature` error from `run()` or `dry_run()` before database
work. The fluent `path_glob` setter remains infallible.

**Performance:** start the pattern with a literal, not `*`. A leading literal
narrows to an indexable range and stays flat as the namespace grows; a
leading `*` cannot, and cost grows with it:

```rust
engine.query().path_glob("/data/*.json").run()?; // flat
engine.query().path_glob("*/*.json").run()?;      // grows with namespace size
```

See [Performance](./performance.md) for measured numbers.

### Combining predicates

```rust
// Only .txt files directly in /data/docs (not in subdirectories):
let results = engine.query()
    .path_in_dir("/data/docs", false)
    .path_glob("*.txt")
    .run()?;
```

Both predicates compose with each other and with `dry_run`.
