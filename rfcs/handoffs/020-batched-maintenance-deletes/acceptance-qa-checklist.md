# RFC 020 Acceptance & QA Checklist — P1c

Companion to `rfcs/handoffs/020-batched-maintenance-deletes/implementation-handoff.md`.
This is what the review will check.

## A. Scope discipline

- [ ] Three files changed: `cache/engine/maintenance.rs`, `db/repository.rs`, and the test module
      `cache/engine/maintenance/tests.rs` — declared `#[cfg(test)] #[path = "maintenance/tests.rs"] mod tests;`
      per the crate convention, **not** embedded in `maintenance.rs` *(amended 2026-08-03; the
      original said "exactly two files" and left the tests nowhere to live)*
- [ ] **No public API change** — new paged helper is private, `MAINTENANCE_CHUNK` is `pub(crate)`
- [ ] No schema change, no migration, no new index, no new dependency
- [ ] `delete_lru_n` and `delete_by_other_version` untouched
- [ ] No parallel `stat` (deferred by the RFC)
- [ ] No progress/resumability API (would force a minor release)
- [ ] `MAINTENANCE_CHUNK` left at 10 000 — not tuned against an ad-hoc timing

## B. Correctness of the paging

- [ ] Cursor advances from the last path **scanned**, not the last **surviving**
- [ ] A fully-absent page still advances the cursor — **no infinite loop**
- [ ] Termination on a short page (`page.len() < page_size`) or an empty page
- [ ] First page uses `""` as the cursor and reaches every row
- [ ] `ORDER BY path` present — the cursor is meaningless without it
- [ ] Paged queries use `prepare_cached`

## C. Correctness of the batching

- [ ] Deletes run inside `unchecked_transaction`, following the `upsert`/`upsert_in_tx` split
- [ ] The DELETE statement is `prepare_cached` **inside** the transaction
- [ ] Returned count is the sum of rows actually affected, not the number attempted
- [ ] Commit happens per page, not once for the whole sweep

## D. Behaviour preserved

- [ ] Existing tests for both methods pass **unmodified** (if any needed changing — flagged, not silently edited)
- [ ] `guard_write()` still runs first, before any query
- [ ] `cleanup_expired` still returns `Ok(0)` when `ttl` is `None`
- [ ] Path semantics unchanged: exact stored strings, **no re-canonicalization**, case-only renames preserved
- [ ] Namespace isolation holds for both methods

## E. Tests added

- [ ] Chunk boundaries: `page_size - 1`, `page_size`, `page_size + 1`
- [ ] Fully-absent page (the infinite-loop regression test)
- [ ] Mixed present/absent spanning pages
- [ ] Partial progress on error: completed pages committed, failing page rolled back
- [ ] `cleanup_expired` parity for the above, plus `ttl == None`
- [ ] Empty namespace, single-entry namespace
- [ ] At least one test through the **public** method with the real constant
- [ ] Boundary tests use a small page size, not 10 000 real files

## F. Gates

- [ ] `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` clean
- [ ] `cargo fmt --all --check` clean
- [ ] `python3 scripts/source_integrity.py --require-tracked` OK
- [ ] Full suite green, count reported against the baseline
- [ ] `git status --porcelain` clean of scratch residue

## G. Reporting

- [ ] Any behaviour difference found during implementation is **reported, not absorbed**
- [ ] If the smoke check is nowhere near ~1.09 s at 1M, that is stated plainly — the RFC's cost
      model would be wrong, which is worth more than a quietly adjusted constant
- [ ] Judgement calls reported rather than resolved silently

## What will not count against you

- Reporting that the cost model is wrong. It was derived from four measured strategies, but on
  your run it may not hold — say so.
- Finding that an existing test genuinely must change, and explaining why.
- Declining to guess a mechanism for anything surprising. That discipline has caught three
  artifacts across this phase already.
