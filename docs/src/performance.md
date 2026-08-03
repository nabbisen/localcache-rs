# Performance and Capacity

Measured guidance for sizing a cache and avoiding the one query pattern that does
not scale.

> **How to read these numbers.** They come from a single host — Linux x86_64,
> release profile, database on **real storage (btrfs on a LUKS-encrypted volume)**,
> entries stored at **71-character absolute paths**, with every entry in **one
> namespace** and the JSON codec. Treat the **shape** of each curve as the finding
> and the absolute figures as indicative. Your hardware, storage, path depth, and
> namespace layout will shift the constants — see "Limits of these numbers" below.
>
> Reproduce them yourself:
>
> ```bash
> LOCALCACHE_SCALE=100000 cargo bench --features json --bench scale_profile
> ```

## The short version

- **Point lookups do not slow down as the cache grows.** `get` and `get_if_fresh`
  stay around 8.5–9 µs from ten thousand entries to a million.
- **Budget roughly 1000 bytes per entry** at this path depth. A million entries is
  about 1 GB. Shorter or longer stored paths shift this — see below.
- **In `path_glob`, start the pattern with a literal.** A leading literal stays flat
  at any size; a leading `*` grows with the namespace.
- **Avoid whole-namespace JSON field queries on large caches.** Sorting on a JSON
  field costs about 4 seconds per million entries, even with a small `limit`.
- **`cleanup_missing_files` and `cleanup_expired` scale linearly** and are now fast
  enough at 1M to no longer dominate a maintenance pass — see below.

## Measured scaling

Time per operation at three namespace sizes:

| Operation | 10k | 100k | 1M | Growth |
|---|---|---|---|---|
| `get` (warm hit) | 8.45 µs | 8.92 µs | 8.40 µs | **flat** |
| `get_if_fresh` (metadata mode) | 8.56 µs | 9.07 µs | 8.75 µs | **flat** |
| `path_glob`, leading literal | 3.07 ms | 2.97 ms | 3.17 ms | **flat** |
| `batch_set`, per entry | 12.89 µs | 12.71 µs | 13.36 µs | flat per entry |
| `cleanup_missing_files` (10% absent) | 9.63 ms | 108.8 ms | 1.08 s | linear |
| `cleanup_expired` (whole namespace) | 43.2 ms | 448 ms | 6.31 s | linear |
| LRU eviction, per evicted entry | 4.68 µs | 3.50 µs | 3.29 µs | flat per entry |
| `path_in_dir`, non-recursive | 3.66 ms | 8.94 ms | 60.8 ms | 17× |
| `path_glob`, **leading wildcard** | 3.62 ms | 9.49 ms | 68.9 ms | 19× |
| `field_gt` + `order_by_field` + `limit 25` | 37.0 ms | 386 ms | **4.00 s** | **108×** |

Storage grows linearly at roughly **980 / 1018 / 1031 bytes per entry** at the three
sizes above, including the payload, path, and index overhead.

**The storage figure is not universal — it depends on path length.** Every entry
stores its absolute path in the table *and* again in the covering index, so
per-entry storage, and every index-scan timing in the table above, depend on how
deep the cached files live. A 27-character stored path measured 979 bytes/entry
against 1251 bytes/entry at 106 characters — a 1.28× difference from path length
alone, no code change involved. A reader planning capacity should measure at their
own path depth rather than assume the figures above transfer directly.

## `cleanup_missing_files` and `cleanup_expired`

Both scan the namespace in pages and delete each page's removals inside one
transaction, rather than committing once per deleted row. `cleanup_expired` costs
more than `cleanup_missing_files` at the same scale because it can delete the whole
namespace (every expired entry) rather than a fraction — the table above measures
`cleanup_missing_files` with 10% of entries absent and `cleanup_expired` with the
entire namespace expired, which is the more representative case for each ("some
files went away" vs. "everything past its TTL").

Both are still linear in namespace size and both are destructive — a full sweep of
a million-entry namespace, even at the faster figures above, is measured in
seconds, not milliseconds. Schedule them off the hot path.

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
the prefix. At ten thousand entries the two are within 20% of each other, so this is
easy to miss in a small test and only becomes visible at scale.

## Namespaces are the main scaling lever

Every operation whose plan narrows by `namespace=?` scans the whole namespace. The
measurements above are the pessimistic case because they place all entries in one.
Splitting unrelated data across namespaces makes that predicate do real work and is
the cheapest structural improvement available.

## What was not measured

- **Watcher behaviour on large trees.** Needs sustained observation with induced
  filesystem events, not one-shot timing — a fundamentally different harness shape
  from everything else on this page.
- **Async-runtime backends under concurrent load.** `ReadPool` under 8 threads was
  measured (aggregate ~5.4 µs/call at 1M); tokio, async-std, and smol under
  concurrent access were not.

Two more operations have been measured, as single-run figures — treat them as
indicative, not as precise as the replicated table above:

- `preload` costs roughly **71.5 µs per entry** at 1M (a directory scan plus a
  `stat` and a factory call per file, so it costs more than `batch_set`, which
  accepts pre-built payloads).
- A cold `CacheEngine::open` against an existing 1M-entry database — the cost a
  process pays on start, with the page cache empty — measured **roughly 0.6–0.8
  seconds** across two runs.

## Limits of these numbers

- **These are real-storage figures on one filesystem (btrfs on LUKS)**, not a
  universal constant. Copy-on-write, checksumming, and encryption all add cost;
  ext4 or an unencrypted SSD will differ. This is *a* real-storage number, not
  *the* one.
- **One namespace holds every entry.** Operations narrowing only on `namespace = ?`
  therefore scan nearly the whole table — the pessimistic case, and the right
  default for finding hotspots, but a deployment spreading entries across many
  namespaces will see different numbers.
- **`cleanup_missing_files` and `cleanup_expired` are destructive and cannot be
  repeated within a run**, so their figures above are single-sample (at 1M, the
  mean of two same-session runs) and carry roughly ±15% session-to-session
  variance. A **ratio** measured as a same-session paired comparison — the "before
  RFC 020 / after RFC 020" figures reported in the project's own history — is
  reliable; a single **absolute** figure quoted across sessions, or across hosts,
  is not.
