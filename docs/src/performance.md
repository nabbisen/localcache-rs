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
- **For payloads of 1 MiB and up, your release profile matters.** `codegen-units = 1` halves a large
  `Vec<u8>` `get` and does nothing measurable for an embedding — see "Large payloads and your release
  profile".
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
SEARCH main.files USING COVERING INDEX sqlite_autoindex_files_1 (namespace=?)
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
SEARCH main.files USING COVERING INDEX sqlite_autoindex_files_1 (namespace=? AND path>? AND path<?)
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

## What a read hit costs (v0.23.0)

A read hit records a use: it reads the namespace's largest use value and moves one index entry
(RFC 026); before v0.23.0 it rewrote a whole-second timestamp. On a 100,000-entry database a hot hit took
**about 7 to 11 µs more** than before. Two measurements are the two ends: a first micro-measurement gave
6.6 µs, and seven alternating rounds of paired runs gave a median of 11 µs, for a 64-byte payload and for
the `scale_profile` payload alike (single measurements ranged from about −4 to +33 µs). The cost did not
grow with the payload size. A hit that lands on a cold page costs far more than that in either version; the
difference above is only what the use adds.

These are bounds, not promises: they were measured on one shared host, for one database size and two payload
shapes. Measure on your own hardware if a few microseconds per hit matter.
Writing gets cheaper in the same change: `batch_set` took about three quarters of its former time per entry, and
a new database file is about a quarter smaller (one fewer index to maintain). A file migrated from v5 does not
get smaller by itself; see [Migration](./migration.md).

## What binding an encrypted entry costs (v0.24.0)

With an encryption key, each payload is sealed bound to its path: the path and the encoding tag are
authenticated along with the ciphertext (associated data of about 165 bytes for a 124-byte path). On a
host shared with other work, with seven alternating rounds against v0.23.1, a 64-byte entry took about
**0.12 µs longer to `set`** (+1.2%) and **0.17 µs longer to `get`** (+3%); a 1 MiB entry was not
distinguishable from the spread of the runs (about 0.3% to 0.5%). These are bounds measured for one
path length and two payload sizes, not promises.

`rotate_encryption_key` is the one operation whose cost changed in kind: it now seals every entry of
the namespace, in pages of 1,000 rows or 8 MiB. At 1,000,000 entries of 64 bytes it took about 2 s and
8 MiB of memory (the earlier method held the whole namespace, 336 MiB in the same measurement, and 8 GiB
at 4 KiB entries); the write-ahead log still grows by about the size of the namespace's payloads. Its
pages are read by `(namespace, path)`, which SQLite serves from the unique index in order; paging by
row id instead scans the whole namespace for every page, and was about 300 times slower for pages of 100 rows in the same test.

## Large payloads and your release profile (v0.25.1)

This section is about payloads of about 1 MiB and up, such as a large `Vec<u8>` blob or a `Vec<f32>` of
a quarter of a million elements. It does not apply to an embedding of 768 to 4,096 `f32` (3 to 16 KiB), or to a
small struct; those are discussed at the end.

**Reading.** Decoding is the cost of a large `get`: **80 to 93%** of a 1 MiB `Vec<u8>` `get`, which takes
about 650 µs with the profile Cargo gives a dependency by default (16 code-generation units, no LTO). The rest is
SQLite reading the blob (about 90 µs) and one copy (about 20 µs). How fast the decoder's element loop runs
depends on how your compiler splits the program into code-generation units, so **your release profile
matters**. Paired runs on one host, a downstream crate built with the profile named in the first row, against
the default (a value below 1 is faster):

| `get` of | `codegen-units = 1` | `lto = "fat"` | `lto = "thin"` |
|---|---|---|---|
| `Vec<u8>`, 1 to 4 MiB | **0.50×** (654 → 324 µs at 1 MiB) | 0.88 to 0.90× | no gain (1.0 to 1.3× of the default) |
| `Vec<f32>`, 1 to 4 MiB | **0.80×** | 0.73 to 0.74× | no gain (1.0 to 1.3× of the default) |
| `Vec<f32>`, 768 to 4,096 elements | 0.94 to 0.97× (not worth a change) | 0.91 to 0.97× | no gain |
| a `String` of 1 MiB, a small struct | 1.0× | 1.0× | 1.0× |

```toml
# in the downstream application's Cargo.toml: only if you read blobs of 1 MiB and up
[profile.release]
codegen-units = 1
```

`codegen-units = 1` costs build time (each crate is compiled as one unit, with less parallelism); the
build time was not measured here. `lto = "fat"` costs more. **Do not change a profile for small payloads**: for
768 to 4,096 `f32` the difference is a few percent, inside the spread of the runs.

**Writing (v0.25.1).** Before v0.25.1 a `set` of a large `Vec<u8>` took a time that depended on where the
input buffer sat in memory: the same 1 MiB took 1.1, 1.4 or 1.6 ms in different processes (the input and the
output buffer were at addresses that differ by a multiple of 4 KiB, which the CPU's memory-ordering check mistakes for an overlap).
The cause was that the encoder's primitive writers were called as functions, once per element, instead of
compiled into the caller; they are now `#[inline]`. A 1 MiB `Vec<u8>` `set` takes **about 0.36 to 0.5 ms**
(0.24 to 0.28× of the former median at the default profile, 0.41× with `codegen-units = 1`), and a `Vec<f32>` of 1 MiB
0.5 to 0.7× of its former time; a 4,096-float `set` is about 10% faster, a 768-float or small-struct `set` is unchanged
(0.97 to 1.02×, inside the spread of the runs). The bytes written are identical.

**Embeddings (768 to 4,096 `f32`).** A `get` takes 7 to 10 µs on a quiet host, and decoding the vector is **6 to
22% of it** (0.4 to 2.2 µs). The rest is fixed per call; in a sampling profile of a 4,096-float `get` (a few
hundred samples, so roughly) resolving the path took about a fifth, SQLite's file locks about an eighth, and the write
that records the use about a sixth. A profile change cannot
reach those, which is why it does nothing here.

**Limits.** These are bounds from one host (AMD Ryzen 9 9950X, Linux, `rustc` 1.99.1, the database on a local
filesystem), not promises. The split of *your* crate into code-generation units moves these results by itself: one
`Vec<f32>` 1 MiB `get` measured 185 µs and 263 µs (a 42% difference) in two builds of the same source that differed
only in unrelated code of the benchmark program.
Measure with your own payloads and your own profile before relying on a figure here.

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
  measured (aggregate ~2.6 µs/call at 1M); tokio and smol under
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
