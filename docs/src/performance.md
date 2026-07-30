# Performance and Capacity

Measured guidance for sizing a cache and avoiding the one query pattern that does
not scale.

> **How to read these numbers.** They come from a single host — Linux x86_64,
> release profile, database on a RAM-backed tmpfs — with every entry in **one
> namespace** and the JSON codec. Treat the **shape** of each curve as the finding
> and the absolute figures as indicative. Your hardware, storage, and namespace
> layout will shift the constants.
>
> Reproduce them yourself:
>
> ```bash
> LOCALCACHE_SCALE=100000 cargo bench --features json --bench scale_profile
> ```

## The short version

- **Point lookups do not slow down as the cache grows.** `get` and `get_if_fresh`
  stay around 7.5 µs from ten thousand entries to a million.
- **Budget roughly 950 bytes per entry.** A million entries is about 950 MB.
- **In `path_glob`, start the pattern with a literal.** A leading literal stays flat
  at any size; a leading `*` grows with the namespace.
- **Avoid whole-namespace JSON field queries on large caches.** Sorting on a JSON
  field costs about 4 seconds per million entries, even with a small `limit`.

## Measured scaling

Time per operation at three namespace sizes:

| Operation | 10k | 100k | 1M | Growth |
|---|---|---|---|---|
| `get` (warm hit) | 6.98 µs | 7.31 µs | 7.51 µs | **flat** |
| `get_if_fresh` (metadata mode) | 6.94 µs | 7.32 µs | 7.55 µs | **flat** |
| `path_glob`, leading literal | 2.87 ms | 3.01 ms | 3.05 ms | **flat** |
| `batch_set`, per entry | 9.89 µs | 10.26 µs | 10.71 µs | flat per entry |
| `cleanup_missing_files` | 14.4 ms | 147 ms | 1.40 s | linear |
| LRU eviction, per evicted entry | 1.53 µs | 1.99 µs | 2.07 µs | flat per entry |
| `path_in_dir`, non-recursive | 3.48 ms | 7.60 ms | 46.2 ms | 13× |
| `path_glob`, **leading wildcard** | 3.31 ms | 8.34 ms | 56.3 ms | 17× |
| `field_gt` + `order_by_field` + `limit 25` | 38 ms | 402 ms | **4.38 s** | **115×** |

Storage grows linearly at roughly 950 bytes per entry, including the payload, path,
and index overhead.

## Why the last row is so slow

`ORDER BY` on a JSON field cannot use an index. `EXPLAIN QUERY PLAN` shows the query
narrowing only by namespace:

```text
SEARCH main.files USING COVERING INDEX idx_files_namespace_path (namespace=?)
```

So the engine decodes the JSON field from **every** entry in the namespace, sorts
them all, and only then applies `limit`. A small `limit` does not reduce the work —
the sort has to see every row first.

If you need ranked access over a large namespace, the practical options today are to
narrow the candidate set first with a path predicate, keep the ranked subset in its
own smaller namespace, or sort in your own code over a bounded result set.

You can check any query's plan before running it:

```rust
let plan = engine.query()
    .field_gt("score", 0.5)
    .order_by_field("score", false)
    .dry_run()?;
println!("{plan}");
```

## Why glob prefixes matter

The same index explains the glob difference. A leading literal produces a range
scan:

```text
SEARCH main.files USING COVERING INDEX idx_files_namespace_path (namespace=? AND path>? AND path<?)
```

A leading wildcard cannot, so the pattern is tested against every entry in the
namespace.

```rust
// Flat regardless of cache size — the literal prefix bounds the scan.
engine.query().path_glob("/data/embeddings/*.json").run()?;

// Grows with the namespace — nothing bounds the scan.
engine.query().path_glob("*/embeddings/*.json").run()?;
```

Both returned the same 1000 rows in the measurements above; the only difference is
the prefix. At ten thousand entries the two are within 15% of each other, so this is
easy to miss in a small test and only becomes visible at scale.

## Namespaces are the main scaling lever

Every operation whose plan narrows by `namespace=?` scans the whole namespace. The
measurements above are the pessimistic case because they place all entries in one.
Splitting unrelated data across namespaces makes that predicate do real work and is
the cheapest structural improvement available.

## What was not measured

`preload`, concurrent access through `ReadPool` or the async backends, the bincode
codec at scale, watcher behaviour on large trees, and first-open cost after a
process restart.

One caveat worth repeating: the measurement host kept the database on **tmpfs**, so
filesystem checks ran at memory speed. `cleanup_missing_files` and metadata-mode
freshness checks call `stat` per entry, so on real storage they will be **slower**
than the table shows. Treat 1.40 s per million as a floor.
