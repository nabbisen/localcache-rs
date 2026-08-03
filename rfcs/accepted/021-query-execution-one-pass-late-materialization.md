# RFC 021 — Query Execution: One Pass, Late Materialization

| Field | Value |
|---|---|
| Status | Accepted (owner, 2026-08-03) |
| Feature | *(no Cargo feature; core query engine)* |
| Touches | `crates/localcache/src/cache/query.rs`, `crates/localcache/src/db/repository.rs` |
| Finding | Phase 23 P2a cost decomposition, 2026-08-03 |
| Milestone | Phase 23 P2a |
| Breaking | **No** — no public API, schema, or wire-format change |
| Authorship | High-capability model; **reviewed by the owner** (owner decision, 2026-08-01) |
| Supersedes framing | ROADMAP's "JSON query design — the field-query ceiling" |

## Summary

The 1M-row field query costs **~4.2 s, and ~80% of that is not JSON work.** It is a per-row
`SELECT` pair and a payload decode performed for every row in the namespace, before `LIMIT` is
applied. Fetch the rows in one streaming query, push filtering and ordering into SQL where the
encoding permits, and **decode only the payloads that survive `LIMIT`**.

Projected: **~4.2 s → well under 1 s** for the headline query, and a proportional improvement to
*every* query, not only field queries.

## Motivation

### The recorded diagnosis was wrong

The ROADMAP records, from N4:

> `dry_run()` shows the plan narrowing on `namespace=?` only, so every row's JSON is decoded and
> sorted before `LIMIT` applies… no index can serve a JSON field extraction or an `ORDER BY` on
> one.

The conclusion — an expression index over `json_extract` — follows from that premise. **The
premise describes SQLite doing work it does not do.** `dry_run()` shows the *candidate-path*
query, which is the cheap part. Reading `execute_query` shows what actually happens:

1. `repository::keys(...)` runs one query and materialises **every matching path** into a `Vec`.
2. For each path: `find_file(path)` — **a second query** — then `load_payload(id)` — **a third**.
3. The payload is decoded into `T`.
4. `serde_json::to_value(&entry.payload)` **re-serialises `T` back into a `serde_json::Value`**.
5. Predicates are evaluated **in Rust**, against that value.
6. The sort runs in Rust.
7. **`limit` is applied at the very end**, at `query.rs:655` — after the loop.

So `.limit(25)` against a 1M-entry namespace performs **1 + 2,000,000 statement executions and
1,000,000 payload decodes to return 25 rows.**

### Measured, not inferred

Layers isolated through the public API by varying only the query shape, at 1M entries, JSON codec,
same session — and repeated with a lighter payload so that row count and query count are held
identical while the bytes deserialised change:

| Layer | heavy payload (1331 B/row) | light payload (713 B/row) |
|---|---|---|
| **1. `limit(25)` only — no JSON work at all** | **3.386 s — 79.8%** | 2.347 s — 90.8% |
| 2. `+ field_gt` (`to_value` + predicate) | 0.646 s — 15.2% | 0.140 s — 5.4% |
| 3. `+ order_by_field` (the Rust sort) | 0.211 s — 5.0% | 0.098 s — 3.8% |
| **total** | **4.243 s** | 2.585 s |

Layer 1 contains no JSON evaluation whatsoever, and it is four fifths of the cost. Splitting it
using the payload-size difference:

| Component | cost at 1M | per row |
|---|---|---|
| **Per-row SQL round-trips** (the `1 + 2N` pattern) | **~2.35 s** | 2.35 µs |
| Payload decode | ~1.04 s | 1.04 µs |
| `to_value` + predicate | 0.65 s | 0.65 µs |
| Sort | 0.21 s | 0.21 µs |

An independent check agrees: a narrow `path_glob` returning 1000 rows costs 3.58 ms — **3.58 µs
per row through the same loop**, which extrapolates to 3.58 s at 1M, against layer 1's measured
3.39 s.

### What this means for the proposed remedy

**A `json_extract` expression index addresses the bottom 20% of the chart.** Even if it made
predicate evaluation and sorting entirely free, the query would go from 4.24 s to ~3.4 s — a 1.25×
improvement, in exchange for solving the hardest problem in the design space (payloads may be
bincode, zstd-compressed, or AES-256-GCM encrypted, and `json_extract` needs literal JSON on disk).

**The fetch loop is the cost, and it is encoding-independent.** This is the second time in Phase 23
that the recorded remedy targeted a minority of the measured cost: RFC 020's inherited candidate
was "batch the `stat` calls," which was 11.7% while the per-row commits were 88.3%.

## Design

### R1 — One streaming query instead of `1 + 2N`

`keys()` already scans `files` with all path predicates and the RFC 002 index hint applied. It
then discards everything but the path, and the loop re-queries `files` **by the path it just
returned**, then queries `payloads` separately.

Replace with a single statement that selects the file columns and left-joins `payloads`, reusing
`build_path_sql`'s existing predicate and index-hint construction unchanged:

```sql
SELECT f.path, f.mtime, f.file_size, f.hash, f.updated_at, f.last_accessed_at,
       p.content, p.encoding
FROM files f
LEFT JOIN payloads p ON p.file_id = f.id
WHERE <existing path predicates>
```

Stream it with `query_map` rather than collecting paths first, so peak memory stops being
proportional to namespace size. `LEFT JOIN` preserves today's behaviour for a file row with no
payload row: the current code `continue`s on `load_payload → None`, and the join must skip those
identically, not yield a null-payload entry.

This is the 55% item and it is encoding-independent.

### R2 — Apply `LIMIT` before materialising payloads

> **Amended 2026-08-03, before implementation.** The first draft of this section pushed *ordering*
> into SQL. That is unsafe: three comparator divergences were found while writing the P2b handoff
> (see "Ordering hazards" below). The corrected design keeps **every comparison in Rust** and uses
> SQL only to avoid *fetching and decoding payloads*. It achieves the same saving without any
> comparator-parity risk.

The engine currently decodes every candidate to return `limit` rows. The fix is not to move sorting
into SQL — it is to **stop fetching payloads until the winners are known**. Two passes:

**Pass 1 — select and order, without payload content.** One query over `files`, with the existing
path predicates and index hint, selecting only what ordering needs: `id`, `path`, `mtime`,
`file_size`, `hash`, `last_accessed_at`. **No `content` blob, no decode.** Sort in Rust with the
*existing* comparator, then apply `offset`/`limit`.

**Pass 2 — materialise the survivors.** One query fetching `content` and `encoding` for the
surviving ids, then decode those. `query().limit(25)` becomes **25 decodes instead of a million**,
for every encoding, with ordering semantics bit-identical to today's because the comparator did not
move.

**Field predicates and field sorts need the payload for every candidate** — unless the field can be
extracted without shipping the blob. Where every payload in the namespace is `encoding = 'json'`,
pass 1 may add the extracted value as a column:

```sql
SELECT f.id, f.path, …, json_extract(p.content, '$.score') AS sort_key
FROM files f JOIN payloads p ON p.file_id = f.id
WHERE <existing path predicates>
```

`sort_key` comes back as a scalar; **the comparison still happens in Rust**, mapping it to
`Option<f64>` exactly as `json_sort_key` does today (non-numeric and missing both become `None`).
The predicate may be pushed into the `WHERE` clause, since a predicate is a filter rather than an
ordering and `field_gt`'s `as_f64().map(|n| n > t).unwrap_or(false)` semantics translate exactly
when non-numeric rows are excluded.

**Check the encoding precondition, do not assume it:** a namespace can hold mixed encodings if a
user changes codec or enables compression mid-life. Gate this on a cheap
`SELECT COUNT(*) FROM payloads … WHERE encoding <> 'json'` being zero. If it is not zero, fall
through — **never** push down for some rows and silently drop the rest.

**Any other encoding with a field predicate or field sort** keeps today's decode-everything
behaviour; `zstd`, `raw`, and every `-aes256gcm` variant are opaque to SQL by construction. Pass 1's
single query still applies, so R1's saving is kept.

#### Ordering hazards — why the comparator does not move

All three were found in the current code and all three would silently change results:

| # | Hazard | Why SQL differs |
|---|---|---|
| 1 | `OrderBy::UpdatedAt` compares **`metadata.mtime`**, not `updated_at` (`query.rs:704`) | The obvious SQL translation `ORDER BY updated_at` sorts by a different column than the method name implies |
| 2 | `OrderBy::Path` compares `PathBuf`, which orders **component-wise**, not byte-wise | `/a/b` vs `/a-b`: Rust gives `/a/b` first, SQL `BINARY` collation gives `/a-b` first — **opposite** |
| 3 | `json_sort_key` is `as_f64()`, so a string or missing field becomes `None` and sorts **first** ascending | SQLite orders `NULL < REAL < TEXT`, so a string field sorts **after** numbers |

Hazard 2 is the sharpest: it needs only a namespace containing both `/a/b` and `/a-b` to produce a
different result set under `limit`, with no error anywhere.

**Tier 3 — field predicate or field sort over any other encoding.** `zstd`, `raw`, and every
`-aes256gcm` variant are opaque to SQL by construction; an expression index over an encrypted BLOB
is meaningless. The fields are reachable only through `T`, so a full decode is unavoidable and
this tier keeps today's behaviour. R1's single-pass fetch still applies, so it improves too.

Tier 3 is a real, permanent limit rather than a gap to close later. It should be **documented and
observable**, not silently slow — see R4.

### R3 — Stop re-serialising `T` to reach its own fields

For `encoding = 'json'`, step 3–4 above decode JSON bytes into `T` and then serialise `T` back into
a `serde_json::Value` — two full serde passes to recover a structure the stored bytes already had.
Where a `serde_json::Value` is what the predicate needs, parse the bytes once.

Under tier 2 this largely disappears, since SQL evaluates the field. It still applies to tier 3
whenever the payload is JSON-but-compressed or JSON-but-encrypted: after decompressing or
decrypting, parse to `Value` directly rather than routing through `T`.

### R4 — Let `dry_run()` tell the truth about which tier ran

`dry_run()` currently returns the SQLite plan for the candidate query, which — as this RFC's
motivation shows — is exactly the part that misleads. It should also report **which tier the query
will use, and why**, so that "this query decodes every payload in the namespace" is visible before
someone measures it.

This is the RFC 018 principle applied to performance: the engine should not appear to do less work
than it does. It is the honest answer to tier 3, and it is cheaper and more useful than a JSON
index that cannot serve most payload configurations.

## What is deliberately not proposed

**A user-declared `json_extract` expression index.** Tier 2 already pushes the predicate and sort
into SQL for exactly the payload configuration an index could serve, without new public API, new
schema, or new invalidation rules. An index would add: what happens when a user enables compression
after building one, whether it is declared or inferred, how it interacts with RFC 011's ownership
validation, and what the API says when it cannot help. **Revisit only if tier 2 measures
insufficient** — and it should be measured before being designed, which is the whole lesson here.

**Parallel decode.** Same reasoning as RFC 020's deferred parallel `stat`: fix the structural cost
first, re-measure, then decide whether concurrency is worth its failure modes.

## Semantics that must not change

- **Result ordering must be identical**, including tie-breaking. Guaranteed structurally by the
  amended R2: the comparator never moves to SQL. See "Ordering hazards" for the three concrete
  divergences this avoids.
- **`offset` + `limit` interaction** is unchanged (`query.rs:655-658`).
- **Rows whose payload fails to decode are skipped**, not surfaced as errors — today's `Err(_) =>
  continue`. Late materialisation must not turn a previously-skipped row into a failed query.
- Predicates that inspect the whole payload (`payload_contains`) still require the full value.
- **Queries perform no writes** — verified; they do not touch `last_accessed_at`. Nothing here may
  introduce one.
- `read_only` mode, RFC 002 index hints, and RFC 011's identifier boundary are untouched.

## Testing requirements

Existing query tests must pass unmodified — 57 in `tests/query.rs` alone. Add:

1. **Tier equivalence.** The same query over the same data returns byte-identical results under
   each tier. Force tier 3 by storing one entry with a non-`json` encoding in the namespace, and
   assert the result set matches the all-JSON tier-2 case.
2. **Mixed-encoding safety.** A namespace with one `zstd` row among `json` rows must return the
   same rows as before — the specific failure this design could introduce is silently dropping
   rows SQL cannot evaluate.
3. **Ordering parity**, including ties on the sort field, `NULL`/missing fields, and non-numeric
   values where a numeric comparison is requested.
4. **Undecodable payload** is still skipped rather than failing the query.
5. **`LIMIT`/`OFFSET` boundaries** at 0, 1, exactly-`limit`, and beyond the result count.
6. **A file row with no payload row** is skipped identically under the `LEFT JOIN`.
7. **Decode count.** The count of payload decodes for a tier-1 `limit(25)` query is bounded by the
   limit, not the namespace size — the property this RFC exists to create.

## Risks

| Risk | Assessment |
|---|---|
| ~~SQL ordering differs from Rust ordering~~ | **Removed by the R2 amendment** — the comparator stays in Rust, so there is no parity to establish. Three concrete divergences that would otherwise have shipped are tabulated under "Ordering hazards". |
| Mixed-encoding namespace loses rows under tier 2 | Prevented by the precondition check; test 2 exists for it. |
| `LEFT JOIN` changes no-payload behaviour | Explicit above; test 6. |
| Streaming holds a read statement open longer | Queries take no write locks and WAL readers do not block writers. |
| Measurements from one host and one filesystem | The dominant term is statement executions and CPU deserialisation, neither strongly filesystem-bound; the *shape* should hold, the multiple may differ. |

## Expected result, for P2c to check

At 1M entries, JSON codec, headline query (`field_gt` + `order_by_field` + `limit 25`):

| | now | projected |
|---|---|---|
| Per-row SQL round-trips | ~2.35 s | ~0 (one streaming query) |
| Payload decode | ~1.04 s | ~25 decodes |
| `to_value` + predicate | 0.65 s | pushed into SQL (tier 2) |
| Sort | 0.21 s | pushed into SQL (tier 2) |
| **Total** | **4.24 s** | **well under 1 s** |

No single figure is projected, because the residual is dominated by SQLite's own scan of a 1M-row
covering index — which P1a measured at ~60 ms for `path_in_dir` — plus `json_extract` over 1M
payloads, which nothing has measured yet. **If the result is not at least 4× better, the model in
this RFC is wrong and should be re-derived rather than patched.**

Tier 1 (`limit(25)` with no field predicate) has a much sharper prediction: **3.39 s → milliseconds**,
since it becomes a covering-index scan plus 25 decodes.

Per P1a's limitation 5, the before/after pair must be taken **in one session at a matched `TMPDIR`
path length**.
