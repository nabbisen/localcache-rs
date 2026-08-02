# RFC 020 — Batched Maintenance Deletes

| Field | Value |
|---|---|
| Status | Proposed |
| Feature | *(no Cargo feature; core engine)* |
| Touches | `crates/localcache/src/cache/engine/maintenance.rs`, `crates/localcache/src/db/repository.rs` |
| Finding | Phase 23 P1a real-storage profile, plus a cost decomposition taken for this RFC |
| Milestone | Phase 23 P1b |
| Breaking | **No** — no public API, schema, or wire-format change |
| Authorship | High-capability model; **reviewed by the owner** (owner decision, 2026-08-01) |

## Summary

`cleanup_missing_files` spends **88% of its time committing one transaction per deleted
row.** Wrap the deletes in chunked transactions and page the scan by key. Measured at 1M
entries with 10% missing: **5.761 s → ~1.09 s**, a **5.3× improvement**, with no public API
change and no schema change.

## Motivation

P1a established that `cleanup_missing_files` is the most expensive operation in the engine on
real storage — **5.7 s warm and 8.7 s cold at 1M entries**, against a flat ~3.9 s for the JSON
field query. N4 had ranked it third; on real storage it is first.

N4 proposed a remedy: *"batch the `stat` calls or make the sweep incremental/resumable."*

**That remedy targets the wrong 12%.** Decomposing the cost using only the public API — one
cleanup with nothing missing (scan only), one with 10% missing (scan plus deletes) — gives:

| Phase | Cost @1M | Share |
|---|---|---|
| Scan (1M `exists()` calls) | 0.672 s | **11.7%** |
| Delete (100k rows) | 5.089 s | **88.3%** |

That is **50.7 µs per deleted row**, against a raw `stat` floor of 579 ns per path. The scan is
already close to the syscall floor. The deletes are not close to anything.

### Where the 50.7 µs goes

`cleanup_missing_files` calls `repository::delete_path` per missing entry, and that calls
`conn.execute(...)` — no cached statement, and no enclosing transaction, so **every delete is
its own implicit transaction.**

Four strategies, measured against identical copies of a real 1M-row database with the real
schema and the real pragmas (WAL, `synchronous = NORMAL`, `foreign_keys = ON`):

| Strategy | 100k deletes | per delete | vs current |
|---|---|---|---|
| **a. autocommit + `execute`** — current | 5.070 s | 50.7 µs | — |
| b. autocommit + `prepare_cached` | 5.219 s | 52.2 µs | **none** |
| c. one transaction + `prepare_cached` | 0.501 s | 5.0 µs | **10.1×** |
| d. transactions of 10 000 + `prepare_cached` | **0.415 s** | **4.1 µs** | **12.2×** |

**(b) is the decisive control.** Statement preparation is the intuitive culprit and it buys
*nothing* — the cost is entirely the per-row commit. Had this RFC been written from inspection
rather than measurement, `prepare_cached` is exactly the fix it would have proposed.

Chunked transactions (d) beat one large transaction (c), which is why the design below chunks
rather than wrapping the whole sweep.

## Design

### R1 — Delete in chunked transactions

`cleanup_missing_files` and `cleanup_expired` accumulate the paths to delete and remove them in
transactions of **10 000 rows**, using a cached statement.

10 000 is the measured value from strategy (d). It is an internal constant with no API surface,
so it can be retuned later without a version bump. Finer tuning was not attempted and is not
needed to bank a 12× improvement.

### R2 — Page the scan by key

`all_paths_in_namespace` loads every path in the namespace into a `Vec<String>` — at 1M entries
with 73-character paths that is roughly **100 MB**, and it grows linearly. A 10M-entry cache
would allocate about a gigabyte before doing any work.

Replace it, **for these two methods only**, with keyset pagination:

```sql
SELECT path FROM files
WHERE namespace = ?1 AND path > ?2
ORDER BY path
LIMIT ?3
```

This is served by the existing `idx_files_namespace_path` covering index, so it costs no new
index and no schema change. The cursor is stable under concurrent deletion because deletes only
remove rows the cursor has already passed.

**The page boundary is also the transaction boundary**, so one structure delivers both bounded
memory and the batching of R1. Page size 10 000, matching R1.

### R3 — Apply the same fix to `cleanup_expired`

`cleanup_expired` has the identical defect — `all_file_rows_in_namespace` into a `Vec`, then
`delete_path` per row in autocommit — and it is **potentially worse**, because a namespace whose
entries have all expired deletes every row one transaction at a time. It has never been measured.
Fix it in the same change; measure it in P1d.

`delete_lru_n` and `delete_by_other_version` are already single statements and need no change.
This RFC does not touch them.

## Semantics

**Partial progress on error becomes bounded rather than arbitrary.** Today a failure part-way
through leaves an unpredictable number of rows deleted, because each delete committed
independently. After this change, failure leaves a whole number of completed chunks committed and
the failing chunk rolled back.

Both are partial; neither is all-or-nothing. The new behaviour is strictly better defined, and it
is not observable through the public API — the methods return `Result<usize>`, and on the error
path they return `Err` with no count in both the old and new designs.

**No all-or-nothing guarantee is proposed.** A single transaction over a 1M-row sweep would grow
the WAL unboundedly and hold a write lock for the entire operation, and (c) measured *slower*
than chunking. Cache maintenance does not need atomicity across the whole sweep.

**Lock behaviour.** Each chunk holds a write transaction for roughly 40 ms at the measured rate.
Under WAL, readers are never blocked; other writers wait per chunk rather than per row. This is
strictly better than the current behaviour, which takes and releases the write lock 100 000 times.

## Deliberately out of scope

**Parallel `stat` calls.** Measured: 8 threads take the scan from 0.579 s to 0.081 s, a **7.1×**
speedup. That is real, and after R1–R2 land the scan becomes the dominant remaining term (~62% of
the new total). It is still deferred, for two reasons:

1. **Fix the 88% first, re-measure, then decide.** P1d re-measures. If ~1.1 s at 1M is acceptable,
   introducing threads to a previously single-threaded maintenance path buys little and costs new
   failure modes.
2. Doing both at once destroys the before/after attribution — the same error this phase has
   already corrected three times.

**A progress or resumability API.** Worth considering only if the operation is still slow enough
after this change to need it. At a projected ~1.1 s per million entries it very likely is not, and
adding public API would make this a minor release rather than a patch. **Revisit after P1d, not
before.**

## Compatibility and release

No public API change, no schema change, no wire-format change, no new dependency. Purely an
internal rewrite of two method bodies and their repository helpers.

**This is non-breaking work and should ship as a patch release**, ahead of any breaking work,
per Phase 23's exit criterion 6 and the cadence rule adopted after v0.21.0.

## Testing requirements

Behaviour must be unchanged, so the existing tests for both methods must pass untouched. Add:

1. **Chunk-boundary correctness** — a namespace with entry counts either side of a chunk
   boundary (9 999 / 10 000 / 10 001 missing), asserting the exact returned count.
2. **Mixed present/absent across pages**, so the keyset cursor is exercised with deletions
   interleaved rather than all rows falling in one page.
3. **Partial-progress on error** — inject a failure mid-sweep and assert that completed chunks
   are committed and the failing chunk is not.
4. **`cleanup_expired` parity** — the same three cases, since R3 changes it identically.
5. **Empty and single-entry namespaces**, to pin the paging edge cases.

The path-semantics contract in `cleanup_missing_files`'s existing rustdoc — exact stored strings,
no re-canonicalization, case-insensitive-filesystem behaviour — is unchanged and must stay
covered.

## Risks

| Risk | Assessment |
|---|---|
| Keyset paging changes iteration order | Order was never specified or relied upon; the SQL orders by `path` explicitly. Low. |
| A chunk-sized transaction fails where per-row ones succeeded | Same statement, same constraints; only the commit boundary moves. Low. |
| Bounded partial progress differs from current behaviour on error | Better defined than today, not observable through the public API. Documented above. Low. |
| Measurements taken on one filesystem | btrfs/LUKS, per P1a's standing caveat. The *ratio* (per-row commit vs batched commit) is a SQLite property, not a filesystem one, so the direction holds; the multiple may differ. |

## Expected result, for P1d to check

At 1M entries with 10% missing, on the same host and a matched path length:

| | now | projected |
|---|---|---|
| Scan | 0.672 s | 0.672 s (unchanged) |
| Delete | 5.089 s | ~0.415 s |
| **Total** | **5.761 s** | **~1.09 s** |

If P1d does not see approximately a 5× improvement, the model in this RFC is wrong and should be
re-derived rather than patched. Per P1a's limitation 5, `cleanup_missing_files` is destructive and
cannot be replicated within a run — so the before/after pair must be taken **in one session, at a
matched path length**, and the absolute figures carry roughly ±15% session variance.
