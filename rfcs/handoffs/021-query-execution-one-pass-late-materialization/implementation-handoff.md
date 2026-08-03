# RFC 021 Implementation Handoff — One Pass, Late Materialization

RFC: `rfcs/accepted/021-query-execution-one-pass-late-materialization.md` (accepted 2026-08-03)
Milestone: Phase 23 **P2b**
Files: `crates/localcache/src/cache/query.rs`, `crates/localcache/src/db/repository.rs`,
plus a sibling test module (see §8)

## 0. The change in one sentence

`execute_query` fetches every candidate's payload and decodes it **before** applying `limit`.
Fetch the payloads **after** ordering and limiting, in one query instead of two per row.

**No public API change. No schema change. No new dependency.** This is a rewrite of one function's
internals plus new repository helpers.

## 1. What is actually slow — do not optimise the wrong layer

Measured at 1M entries (RFC 021 § Motivation):

| Component | cost | share |
|---|---|---|
| **Per-row SQL round-trips** (`1 + 2N` statements) | **~2.35 s** | **55%** |
| Payload decode | ~1.04 s | 25% |
| `to_value` + predicate | 0.65 s | 15% |
| Sort | 0.21 s | 5% |

The ROADMAP said SQLite decodes and sorts JSON. It does not — `dry_run()` shows only the
candidate-path query, which is the cheap part. **A `json_extract` index addresses the bottom 20%**
and is explicitly out of scope here.

## 2. Read this before you move any sorting into SQL

**Do not.** The RFC was amended before reaching you for exactly this reason. Three divergences
exist in the current comparator, and each would silently change results with no error:

| # | Current behaviour | The obvious SQL translation |
|---|---|---|
| 1 | `OrderBy::UpdatedAt` compares **`metadata.mtime`** (`query.rs:704`) | `ORDER BY updated_at` — **a different column** than the name suggests |
| 2 | `OrderBy::Path` compares `PathBuf` — **component-wise** | `ORDER BY path` uses `BINARY` bytes. For `/a/b` vs `/a-b` the two orders are **opposite** |
| 3 | `json_sort_key` is `as_f64()`; string/missing → `None`, sorts **first** ascending | SQLite orders `NULL < REAL < TEXT` — a string field sorts **after** numbers |

**The comparator stays in Rust, unchanged.** SQL is used only to avoid *fetching and decoding
payloads*. That is where the 80% is, and it costs no parity risk.

## 3. R1 — one query instead of `1 + 2N`

Today: `repository::keys()` scans `files` and returns `Vec<PathBuf>`; the loop then calls
`find_file(path)` — **re-querying the row `keys()` just read** — and `load_payload(id)`.

Add to `repository.rs` a function that returns the ordering columns in one pass, reusing
`build_path_sql`'s existing predicate and index-hint construction unchanged:

```rust
/// RFC 021 pass 1: candidate rows with everything ordering needs and
/// **no payload content**. Same predicates and index-hint handling as `keys`.
pub(crate) fn query_candidates(
    conn: &Connection,
    namespace: &str,
    pattern: Option<&str>,
    index_hint: Option<&str>,
    path_in_dir: Option<(&str, bool)>,
    path_glob: Option<&[String]>,
) -> Result<Vec<CandidateRow>, LocalFileCacheError>
```

`CandidateRow` carries `id`, `path`, `mtime`, `file_size`, `hash`, `last_accessed_at` — everything
`cmp_key_json`/`cmp_key_simple` read, and nothing else.

**Keep `keys()`** unless you confirm no other caller; check before deleting, as was done for RFC 020.

## 4. R2 — the two passes

**Pass 1.** `query_candidates(...)` → sort in Rust with the **existing** comparator → apply
`q.offset` / `q.limit` exactly as `query.rs:655-658` does today.

**Pass 2.** For the surviving ids only:

```rust
/// RFC 021 pass 2: payloads for an explicit id set, one statement.
pub(crate) fn payloads_for_ids(
    conn: &Connection,
    ids: &[i64],
) -> Result<Vec<(i64, Vec<u8>, String)>, LocalFileCacheError>
```

Then decode those and build `CacheEntry` values in the order pass 1 established. **Chunk the `IN`
list** — SQLite's default `SQLITE_MAX_VARIABLE_NUMBER` is 999 on older builds; a `limit` larger
than that must not produce a malformed statement. Chunk at 500 and reassemble.

### When the payload is needed for *every* candidate

If the query has a field predicate or a field sort, pass 1 cannot rank without the field.

- **All payloads in the namespace are `encoding = 'json'`** — add
  `json_extract(p.content, '$.<path>')` as a scalar column in pass 1 and read it as
  `Option<f64>`, mapping non-numeric and NULL to `None` so it matches `json_sort_key` exactly.
  The predicate may go in the `WHERE` clause. **The comparison still happens in Rust.**
  Gate on `SELECT COUNT(*) FROM payloads p JOIN files f ON … WHERE f.namespace = ?1 AND p.encoding <> 'json'`
  being zero. **If it is not zero, fall through — never push down for some rows and drop the rest.**
- **Any other encoding** — keep today's decode-everything path. `zstd`, `raw`, and the
  `-aes256gcm` variants are opaque to SQL by construction. R1's single query still applies.

`field_path` reaches SQL as a JSON path. **It must not be interpolated into SQL text** — bind it as
a parameter (`json_extract(p.content, ?)`), and reject or escape paths containing `'` or `"`. RFC 011
established the identifier boundary; this is the same class of hazard on a different axis.

## 5. R3 — stop the double serde

For `encoding = 'json'`, the code decodes bytes → `T`, then `serde_json::to_value(&T)` back to a
`Value` (`query.rs:~608`). Where a `Value` is what the predicate needs, parse the bytes once.

This mostly disappears under the pushdown path. It still applies where the payload is
JSON-but-compressed or JSON-but-encrypted: after decompressing or decrypting, parse to `Value`
directly rather than routing through `T`.

## 6. R4 — `dry_run()` reports the path taken

Add to `dry_run()`'s output which path the query will take and why — pass-1-only, pushdown, or
decode-everything — so "this query decodes every payload in the namespace" is visible before
someone measures it. RFC 018's truthfulness applied to performance.

Keep the existing SQLite plan output. Add, do not replace.

## 7. Behaviour that must not change

- **Result ordering, including ties** — guaranteed structurally by keeping the comparator in Rust.
- **`offset` + `limit`** semantics (`query.rs:655-658`).
- **A row whose payload fails to decode is skipped**, not an error (`Err(_) => continue`). Late
  materialisation must not turn a skipped row into a failed query — and note the skip now happens
  *after* limiting, so a query could return fewer than `limit` rows where it previously back-filled.
  **Decide this deliberately and say which you chose**; matching today's back-fill may require
  over-fetching in pass 2.
- **A file row with no payload row is skipped** — today via `load_payload → None`.
- **Queries perform no writes.** Verified: they do not touch `last_accessed_at`. Do not add one.
- `read_only` mode, RFC 002 index hints, RFC 011's identifier boundary: untouched.
- `payload_contains` inspects the whole payload and still needs full materialisation.

## 8. Tests

Existing query tests must pass **unmodified** — 57 in `tests/query.rs`. If one needs changing, stop
and say so in the review request: it means behaviour moved.

Put new unit tests in a sibling module, `cache/query/tests.rs`, declared
`#[cfg(test)] #[path = "query/tests.rs"] mod tests;` — the crate convention, and it keeps
`query.rs` out of the module-size register's way.

1. **Ordering parity, hazard 2 specifically** — a namespace containing both `/a/b` and `/a-b`,
   ordered by path with a `limit` that cuts between them. This is the sharpest regression this
   change could introduce.
2. **Ordering parity, hazard 3** — a sort field that is numeric in some entries, a string in
   others, and missing in a third.
3. **Ordering parity, hazard 1** — `order_by_updated_at` against entries whose `mtime` and
   `updated_at` orderings disagree.
4. **Mixed-encoding safety** — one `zstd` row among `json` rows, with a field predicate; the
   result set must equal the all-decode path's.
5. **Decode count is bounded by `limit`** — the property this RFC exists to create. Assert it
   observably (a counter behind `#[cfg(test)]`, or infer from timing at a scale where the
   difference is unambiguous).
6. **`IN`-list chunking** — a `limit` above 999.
7. **`limit`/`offset` boundaries** — 0, 1, exactly-`limit`, beyond the result count.
8. **Undecodable payload** and **file row with no payload row** are still skipped.

## 9. Gates

- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` clean
- `cargo fmt --all --check` clean
- `python3 scripts/source_integrity.py --require-tracked` OK
- Full suite green; report the count you observe (**417** as of `f56fa66` — report yours, do not
  restate this one)
- No public API change; no schema change; `git diff --stat` limited to the files in the header

## 10. Not yours

**Do not measure this milestone.** P2c re-measures, paired in one session at a matched `TMPDIR`
path length, per P1a's limitation 5. A smoke check that it is faster and correct is fine; do not
tune against it.

**Do not add a `json_extract` expression index.** Explicitly out of scope — it addresses 20% and
carries the invalidation problems the RFC declines.

**Do not parallelise decode.** Same reasoning as RFC 020's deferred parallel `stat`.

## 11. Expected result

Tier-1 `query().limit(25)` with no field predicate: **3.39 s → milliseconds**, since it becomes a
covering-index scan plus 25 decodes. The headline field query: **4.24 s → well under 1 s**.

No single figure is projected for the headline query — the residual depends on `json_extract` over
1M payloads, which nothing has measured. **If it is not at least 4× better, the RFC's model is
wrong; report that rather than tuning.**
