# RFC 026 Step 0 — Verify the Design Before Any Code

RFC: `rfcs/accepted/026-use-order-and-one-path-index.md` (accepted by the owner, 2026-11-01). Scope: `.git-exclude/reviewed/071-architect-v0.23.0-scope-reviewed-against-the-philosophy-2026-11-01.md`.
Start after the v0.22.2 preparation is pushed. This is **verification and measurement only**: no tracked file changes, everything under `.git-exclude/tmp/`.
Format of the review request: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6.
Authorship: high-capability model. Authorized: the owner.

## 0. Why, and the rule

RFC 026's item A (the use order, R1 to R3) is not proven. The owner's rule: **if the verification shows the design cannot stay simple, A leaves v0.23.0 and B (the index drop)
ships alone.** You do not decide that: you report the evidence and the architect decides. Be adversarial; try to break R1 and R3. Use `rusqlite 0.40.2` with `bundled` (the same
SQLite, 3.53.2, as the crate), WAL, `synchronous=NORMAL`, the crate's busy timeout, on a copy of the real schema (create a database with the crate, then copy it).

## 1. What to establish, each with a command, its output and an exit code

1. **R1 under contention.** The use statement for an existing row: `UPDATE files SET last_used_us = max(?1, (SELECT COALESCE(MAX(last_used_us),0)+1 FROM files WHERE namespace=?2)) WHERE id=?3`,
   and the insert form for a new or overwritten row (`INSERT … VALUES (… max(?now,(SELECT COALESCE(MAX(last_used_us),0)+1 …)) …) ON CONFLICT(namespace,path) DO UPDATE SET … last_used_us = excluded.last_used_us`).
   Run at least 8 connections, in **separate processes** and in separate threads, doing random uses for 60 s against 10,000 rows and two namespaces, with a deliberately small and a
   deliberately wrong clock (a fixed `now`, and a `now` stepping backwards). Report: duplicates per namespace (must be 0), every `SQLITE_BUSY`/`BUSY_SNAPSHOT` seen and whether a retry
   or the busy timeout absorbed it, throughput against the current single-column `UPDATE … SET last_accessed_at = now`.
2. **Overflow.** What SQLite does with `MAX+1` at `9223372036854775807` and at `2^62`: integer wrap, float conversion, or an error; and a statement form that **fails** at or above 2^62
   instead of storing a float. Show it.
3. **R3 backfill.** The rule: per namespace, in order of `(s, updated_at, id)`, `v_1 = s_1·10^6`, `v_i = max(s_i·10^6, v_{i-1}+1)`, where `s = last_accessed_at` if above 0 else `updated_at`.
   Implement it (in SQL, or in Rust inside the transaction: say which), on a namespace of **1,000,000** and of **2,000,000 rows that share one second**, and on a mixed realistic one. Report time,
   peak memory, and a check that the result is strictly increasing, preserves the order of `s`, and that the row count and every payload byte are unchanged.
4. **The planner.** Collect every SQL statement in `crates/localcache/src/db/` that touches `files` by `namespace`, `path` or `last_accessed_at` (eviction, count, listing, exact lookup, `path_in_dir`
   range, glob/like, JSON pushdown candidates). `EXPLAIN QUERY PLAN` each on the **v5 shape** and on the **v6 shape** (duplicate index and `idx_files_lru` gone, `idx_files_use` on
   `(namespace, last_used_us)` added), at 100k rows with `ANALYZE` and without. Report any plan that changes or regresses; the Q7 `path_in_dir` plan is the one to watch.
5. **Readers of `last_accessed_at`.** A table: every occurrence in `crates/` (source, tests, benches, CLI) and `docs/src/`, with what R2 requires of it (`/ 1_000_000`, no change, rewrite the doc, etc.).
   Include `ExportRecord`/`EntryInfo`/`SortKey::LastAccessed` and the query sort in Rust (`cache/query/execution.rs`).
6. **Export and import.** From the RFC's R2: show with the real structs how an exported record maps back (the rule `s × 10^6`, `s = last_accessed_at` if above 0 else `updated_at`), and
   whether a round trip through the current CLI keeps the *relative* recency.
7. **The old behaviour reproduced** (so the new tests are failing-before): the `a`/`g`/`h` sequence of RFC 026's Motivation on the current tree, and the same sequence on a scratch copy of
   the new statement logic (a model is enough) showing `a` is evicted.

## 2. The report

One review request: `.git-exclude/review-request/NNN-dev-rfc026-step0-<date>.md`, with each of 1 to 7 answered with evidence, then:
- **Simplicity:** an honest estimate of the lines and files A adds beyond B, and the migration steps in order, with anything that surprised you;
- **Findings that contradict the RFC**, each stated plainly (the architect corrects the RFC);
- a one-paragraph verdict on whether A can stay simple. You report; you do not decide whether A stays.

## 3. Not yours

Any change to a tracked file; the implementation; the version; the decision to drop A.
