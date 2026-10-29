# Performance and Capacity

Measured guidance for sizing a cache and avoiding the one query pattern that does
not scale.

> **How to read these numbers.** They come from a single host — Linux x86_64,
> release profile, database on **real storage (btrfs on a LUKS-encrypted volume)**,
> entries stored at **147-character absolute paths**, with every entry in **one
> namespace** and the JSON codec. All figures below come from one profile session at
> one path length — see "Limits of these numbers" for why that matters. Treat the
> **shape** of each curve as the finding and the absolute figures as indicative.
> Your hardware, storage, path depth, and namespace layout will shift the
> constants.
>
> Reproduce them yourself:
>
> ```bash
> LOCALCACHE_SCALE=100000 cargo bench --features json --bench scale_profile
> ```

## The short version

- **Point lookups do not slow down as the cache grows.** `get` and `get_if_fresh`
  stay around 10 µs from ten thousand entries to a million.
- **A `limit`-only query's *payload decode* is bounded by the limit, not the
  namespace** — 25 rows decoded, not the whole table. Its SQL-side scan still
  touches every candidate row's metadata, so it is not flat, but it is roughly
  10× cheaper per row than a query that also evaluates a JSON field.
- **Budget roughly 1250–1300 bytes per entry** at this path depth. A million
  entries is about 1.2 GB. Shorter or longer stored paths shift this — see below.
- **In `path_glob`, start the pattern with a literal.** A leading literal stays flat
  at any size; a leading `*` grows with the namespace.
- **`path_in_dir` is flat too.** It is answered by a range on the stored path, so its
  cost follows the size of the directory's subtree, not of the namespace.
- **A JSON field query sorted by `SortKey::Field` is the most expensive query shape** —
  roughly 2.1 seconds per million entries, even with a small `limit`, because
  every candidate's field must still be extracted.
- **`cleanup_missing_files` and `cleanup_expired` scale linearly** and are now fast
  enough at 1M to no longer dominate a maintenance pass — see below.

## Measured scaling

Time per operation at three namespace sizes, from one profile session at a
**147-character** stored path:

| Operation | 10k | 100k | 1M | Growth |
|---|---|---|---|---|
| `get` (warm hit) | 9.19 µs | 9.47 µs | 9.58 µs | **flat** |
| `get_if_fresh` (metadata mode) | 9.57 µs | 9.88 µs | 9.91 µs | **flat** |
| `path_glob`, leading literal | 1.32 ms | 1.35 ms | 1.54 ms | **flat** |
| `batch_set`, per entry | 19.6 µs | 18.4 µs | 25.2 µs | roughly flat per entry |
| **`limit(25)`, no field predicate** | 1.71 ms | 19.2 ms | **198 ms** | ~116× |
| `cleanup_missing_files` (10% absent) | 9.07 ms | 135 ms | 1.40 s | linear |
| `cleanup_expired` (whole namespace) | 82.6 ms | 1.08 s | 6.99 s | linear |
| LRU eviction, per evicted entry | 8.17 µs | 4.98 µs | 6.29 µs | flat per entry |
| `path_in_dir`, non-recursive | 1.46 ms | 1.58 ms | 1.78 ms | **flat** |
| `path_glob`, **leading wildcard** | 2.29 ms | 11.9 ms | 105 ms | ~46× |
| `field_gt` + sort by `SortKey::Field` + `limit 25` | 20.4 ms | 208 ms | **2.10 s** | **~103×** |

`path_in_dir` was ~46× (2.63 ms to 120 ms) in releases up to v0.21.5, which matched case-insensitively
and scanned the whole namespace; v0.21.6 answers it with a range on the stored path, so its time
follows the directory's subtree (1000 rows at every size here). See "Why glob prefixes matter".

The five query rows are the median of five repeats of the whole-namespace query block (spread at
1M: `limit(25)` 1.10×, `field_gt` 1.01×, `path_in_dir` 1.04×). Every other figure above is a single
run, and the `batch_set`, `cleanup_*` and eviction rows in particular move by tens of percent between
sessions (see "Limits of these numbers").

Storage grows at roughly **1247 / 1281 / 1292 bytes per entry** at the three sizes
above, at this run's 147-character path length, including the payload, path, and
index overhead.

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

## Why a `limit`-only query is not flat

Payload content is decoded only for the rows that survive `limit` — 25, not the
whole table. But finding and ordering those 25 candidates still means reading
every entry's metadata through the covering index:

```text
SEARCH main.files USING COVERING INDEX idx_files_namespace_path (namespace=?)
```

That scan is real work — path, mtime, size, and hash for every entry in the
namespace — even though only 25 rows are ever returned. "Bounded by the limit"
describes the *decode* step; the *scan* step was always going to be there, which
is why this row grows with the namespace instead of staying flat. It is still
roughly 10× cheaper per row than the query below, because it never has to
extract a JSON field from anything.

## Why the field query is still the most expensive row

`ORDER BY` on a JSON field cannot use an index, so a field query's pass 1 also
extracts the sort field from every candidate — not by decoding into a typed value
and re-serializing (that cost is gone), but via SQLite's own `json_extract`. That
function is not a streaming parser: it parses the whole stored JSON document on
every call, regardless of where the target field sits in it, so its cost scales
with the number of candidates the same way the plain scan above does. That is why
the `limit`-only row and this row grow at almost the same rate (~116× and ~103×)
despite a very different cost per row — and why a small `limit` does not reduce
the work here either: every candidate's field has to be extracted and compared
before the winners are known.

If you need ranked access over a large namespace, the practical options today are to
narrow the candidate set first with a path predicate, keep the ranked subset in its
own smaller namespace, or sort in your own code over a bounded result set.

You can check any query's plan before running it — the plan also states which
execution path was taken and why:

```rust
let plan = engine.query()
    .field_gt("score", 0.5)
    .order_by(SortKey::Field("score".into()), SortOrder::Desc)
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
namespace. `path_in_dir` uses the same kind of range: its plan is
`(namespace=? AND path>? AND path<?)` for both the recursive and the non-recursive form.

```rust
// Flat regardless of cache size — the literal prefix bounds the scan.
engine.query().path_glob("/data/embeddings/*.json").run()?;

// Grows with the namespace — nothing bounds the scan.
engine.query().path_glob("*/embeddings/*.json").run()?;
```

Both returned the same 1000 rows in the measurements above; the only difference is
the prefix. At ten thousand entries the wildcard is already 1.7× the literal's
time, growing to 68× at a million — easy to miss in a small test, and severe at
scale.

## Namespaces are the main scaling lever

Every operation whose plan narrows only by `namespace=?` scans the whole namespace (a
`limit`-only query, a leading-wildcard glob, a field query). The measurements above are
the pessimistic case because they place all entries in one.
Splitting unrelated data across namespaces makes that predicate do real work and is
the cheapest structural improvement available.

## What was not measured

- **Watcher behaviour on large trees.** Needs sustained observation with induced
  filesystem events, not one-shot timing — a fundamentally different harness shape
  from everything else on this page.
- **Async-runtime backends under concurrent load.** `ReadPool` under 8 threads was
  measured (aggregate ~2.6 µs/call at 1M); tokio, async-std, and smol under
  concurrent access were not.

Two more operations have been measured, as single-run figures — treat them as
indicative, not as precise as the replicated table above:

- `preload` costs roughly **235 µs per entry** at 1M, at this session's 147-character
  path length (a directory scan plus a `stat` and a factory call per file, so it
  costs more than `batch_set`, which accepts pre-built payloads). An earlier session
  measured about 84 µs at a 152-character path, and the difference is not explained.
- A cold `CacheEngine::open` against an existing 1M-entry database — the cost a
  process pays on start, with the page cache empty — measured **roughly 0.6–0.8
  seconds** across two runs. Not re-measured this pass; from an earlier run at a
  shorter (71-character) path length, kept here as indicative rather than
  re-stated at this page's current path length.

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
  repeated within a run**, so their figures above are single-sample and carry
  roughly ±15% session-to-session variance, observed previously as a 1.24×
  spread across five otherwise-comparable runs. A **ratio** measured as a
  same-session paired comparison — the "before RFC 020 / after RFC 020" figures
  reported in the project's own history — is reliable; a single **absolute**
  figure quoted across sessions, or across hosts, is not.
