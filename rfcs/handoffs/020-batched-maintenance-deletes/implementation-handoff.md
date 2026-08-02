# RFC 020 Implementation Handoff — Batched Maintenance Deletes

RFC: `rfcs/accepted/020-batched-maintenance-deletes.md` (accepted by the owner, 2026-08-03)
Milestone: Phase 23 **P1c**
Files: `crates/localcache/src/cache/engine/maintenance.rs`,
`crates/localcache/src/db/repository.rs`

## 0. What you are changing, in one sentence

`cleanup_missing_files` and `cleanup_expired` currently commit **one transaction per deleted
row**. Page the scan by key, and delete each page inside one transaction.

**No public API change. No schema change. No new dependency.** This ships as a patch release.

## 1. Read this before you "improve" on the design

The intuitive fix is `prepare_cached` — `delete_path` calls `conn.execute(...)`, which re-prepares
the statement every time. **It was measured and it buys nothing:**

| Strategy | 100k deletes | per delete |
|---|---|---|
| autocommit + `execute` — current | 5.070 s | 50.7 µs |
| autocommit + `prepare_cached` | 5.219 s | 52.2 µs |
| **one transaction + `prepare_cached`** | 0.501 s | 5.0 µs |
| **transactions of 10 000** | **0.415 s** | **4.1 µs** |

The cost is the per-row commit, not statement preparation. Use a cached statement anyway — it is
free and correct — but **the transaction boundary is the change that matters.** If you find
yourself optimising the scan instead, re-read §2 of the RFC: the scan is 11.7% of the cost and
already within ~16% of the raw syscall floor.

## 2. The constraint that decides the code shape

`CacheEngine` holds `pub(crate) conn: Connection` and these are `&self` methods, so
`Connection::transaction()` (which needs `&mut`) is unavailable.

**Use `conn.unchecked_transaction()`** — already the established idiom here, in
`repository.rs:111`, `repository.rs:543`, `engine.rs:437`, and `engine.rs:658`. Follow the
existing `fn x(conn) → unchecked_transaction → x_in_tx(&tx) → commit` split that `upsert` /
`upsert_in_tx` uses, so the new helpers read like the ones already there.

## 3. R2 — the paged scan

Two new helpers in `repository.rs`:

```rust
/// One page of paths in `namespace`, ordered by `path`, starting strictly
/// after `after`. Pass `""` for the first page: every stored path is a
/// non-empty absolute path, so `path > ''` selects all of them.
/// Served by the existing `idx_files_namespace_path` — no new index.
pub(crate) fn paths_page_in_namespace(
    conn: &Connection,
    namespace: &str,
    after: &str,
    limit: usize,
) -> Result<Vec<String>, LocalFileCacheError>

/// Same page shape, carrying `updated_at` for `cleanup_expired`.
pub(crate) fn path_rows_page_in_namespace(
    conn: &Connection,
    namespace: &str,
    after: &str,
    limit: usize,
) -> Result<Vec<(String, i64)>, LocalFileCacheError>
```

```sql
SELECT path FROM files
WHERE namespace = ?1 AND path > ?2
ORDER BY path
LIMIT ?3
```

Use `prepare_cached` for both — they run once per page.

**Do not delete `all_paths_in_namespace` or `all_file_rows_in_namespace`** without checking their
other callers first. This RFC only changes the two cleanup methods.

## 4. R1/R3 — the batched delete

```rust
pub(crate) fn delete_paths_in_tx(
    tx: &Transaction,
    namespace: &str,
    paths: &[String],
) -> Result<usize, LocalFileCacheError>

pub(crate) fn delete_paths(
    conn: &Connection,
    namespace: &str,
    paths: &[String],
) -> Result<usize, LocalFileCacheError>
```

`delete_paths_in_tx` prepares the existing `DELETE FROM files WHERE namespace = ?1 AND path = ?2`
once via `prepare_cached` and executes it per path, summing the affected-row counts.
`delete_paths` wraps it in `unchecked_transaction` and commits.

The `ON DELETE CASCADE` to `payloads` is unchanged — `payloads.file_id` is `INTEGER PRIMARY KEY`,
so the cascade is an indexed lookup. Nothing to do there.

## 5. The method bodies

Make the page size a parameter of a private helper so tests can exercise the boundaries cheaply
(§7), with the public method supplying the constant:

```rust
/// Rows per scan page and per delete transaction. Measured: 10 000 gives
/// 4.1 µs/delete against 50.7 µs for the per-row commits it replaces.
/// Internal — retunable without a version bump.
pub(crate) const MAINTENANCE_CHUNK: usize = 10_000;

pub fn cleanup_missing_files(&self) -> Result<usize, LocalFileCacheError> {
    self.cleanup_missing_files_paged(MAINTENANCE_CHUNK)
}

fn cleanup_missing_files_paged(&self, page_size: usize) -> Result<usize, LocalFileCacheError> {
    self.guard_write()?;
    let mut removed = 0usize;
    let mut cursor = String::new();
    loop {
        let page = repository::paths_page_in_namespace(
            &self.conn, &self.namespace, &cursor, page_size,
        )?;
        if page.is_empty() {
            break;
        }
        let last = page.len() < page_size;
        // Advance the cursor from the last path *scanned*, before filtering.
        cursor = page[page.len() - 1].clone();

        let absent: Vec<String> = page
            .into_iter()
            .filter(|p| !Path::new(p).exists())
            .collect();
        if !absent.is_empty() {
            removed += repository::delete_paths(&self.conn, &self.namespace, &absent)?;
        }
        if last {
            break;
        }
    }
    Ok(removed)
}
```

**The one trap, and it is a real one.** The cursor must come from the last path **scanned**, not
the last path **surviving**. If you take it from the survivors and an entire page turns out to be
absent, the cursor never advances and the loop spins forever. Test 7.2 exists to catch exactly
this — write it even if you are confident.

`cleanup_expired` takes the same shape: keep the `let Some(ttl) = self.ttl else { return Ok(0); }`
early return, page with `path_rows_page_in_namespace`, filter with
`is_expired(updated_at, Some(ttl))`, delete the batch.

## 6. Behaviour that changes, and behaviour that must not

**Changes — bounded partial progress on error.** Completed pages stay committed; the failing page
rolls back. Today the same failure leaves an arbitrary number of individually-committed deletes.
Both are partial. Not observable through the public API, since both return `Err` with no count.

**Changes — visibility of concurrent inserts.** The old code snapshotted every path up front. The
paged version sees rows inserted during the sweep if they sort after the cursor. Neither is a
consistent snapshot; the new behaviour is simply more current. Harmless, but do not be surprised
by it, and do not add locking to "fix" it.

**Must not change:** the return value on every success path; the path-semantics contract in
`cleanup_missing_files`'s rustdoc (exact stored strings, **no re-canonicalization**,
case-insensitive-filesystem entries preserved); `guard_write()` first, before any query;
`cleanup_expired`'s `ttl == None → Ok(0)` short-circuit.

**Do not touch** `delete_lru_n` or `delete_by_other_version` — both are already single statements.

## 7. Tests

Existing tests for both methods must pass **unmodified**. If you need to change one, stop and say
so in the review request — it means behaviour moved.

Use `cleanup_missing_files_paged(3)` (and the `cleanup_expired` equivalent) so boundary cases cost
three files rather than ten thousand.

1. **Chunk boundaries** — entry counts at `page_size - 1`, `page_size`, `page_size + 1`, asserting
   the exact returned count.
2. **A fully-absent page** — every path in one page missing, so the cursor must still advance.
   **This is the infinite-loop regression test.** Give it a timeout-shaped shape if the harness
   supports one.
3. **Mixed present/absent spanning pages**, so deletion is interleaved with cursor advancement
   rather than confined to one page.
4. **Partial progress on error** — force a failure mid-sweep and assert completed pages are
   committed and the failing page is not.
5. **`cleanup_expired` parity** — cases 1–3 again, plus `ttl == None → Ok(0)`.
6. **Empty namespace** and **single-entry namespace**.
7. **One test through the public method** with the real `MAINTENANCE_CHUNK`, so the wiring is
   covered and not only the parameterised helper.
8. **Namespace isolation** — entries in another namespace are untouched by either method.

## 8. Gates

- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` clean
- `cargo fmt --all --check` clean
- `python3 scripts/source_integrity.py --require-tracked` OK
- Full suite green; report the count against the current baseline
- **No public API change** — the new helper is private, the constant `pub(crate)`
- **No schema change** — no migration, no new index
- `git diff --stat` should touch exactly the two files named above

## 9. Not yours

**Do not measure this milestone.** P1d re-measures, and it must be a paired before/after taken in
**one session at a matched `TMPDIR` path length** — P1a's limitation 5 and the reason the last
three review rounds happened. An ad-hoc timing here would invite tuning against a number that
cannot be compared.

A quick smoke check that it is faster and correct is fine. **Do not tune `MAINTENANCE_CHUNK` on
it.**

**Do not add parallel `stat` calls.** Measured at 7.1× on the scan phase and deliberately deferred
in the RFC — the point is to fix the 88%, re-measure, and then decide. Doing both at once destroys
the attribution.

**Do not add a progress or resumability API.** That would make this a minor release instead of a
patch. Revisit after P1d.

## 10. Expected result

At 1M entries with 10% missing: **5.761 s → ~1.09 s**. If your smoke check is nowhere near that,
say so in the review request rather than adjusting the constant — the RFC's cost model would be
wrong, and that is worth knowing.
