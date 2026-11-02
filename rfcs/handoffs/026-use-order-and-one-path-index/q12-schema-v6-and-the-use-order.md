# Q12 Handoff — Schema v6 and the Use Order (RFC 026 R1 to R3, the code in `crates/localcache/src`)

RFC: `rfcs/accepted/026-use-order-and-one-path-index.md` (accepted 2026-11-01; findings of step 0 folded into R1 to R3 on 2026-11-03).
Evidence you are building on: your request `.git-exclude/review-request/052-dev-rfc026-step0-2026-11-01.md` (the statements, the backfill SQL, the plans, the readers table).
Order: v0.22.2 preparation (done) → step 0 (done) → **Q12** → Q13 → v0.23.0 release preparation.
**Local commits; do not push.** The architect publishes v0.22.2 from a worktree and rebases your commits.
Format of the review request: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6.
Authorship: high-capability model. Authorized: the owner.

## 0. What this is

All the library code of the change, in one slice, because the schema cannot change without the repository code that reads it. Your step 0 already chose the
mechanisms; this file fixes the decisions your findings needed. Anything not covered here: stop and file a design request. Q13 owns the CLI, the book, the README,
the CHANGELOG and the numbers on the performance page; do not touch them (except rustdoc on items you change, which `RUSTDOCFLAGS="-D warnings"` requires).

## 1. The decisions (the RFC states them; this is the checklist)

1. **Column and check (F1).** `last_used_us INTEGER NOT NULL DEFAULT 0 CHECK (typeof(last_used_us) = 'integer' AND last_used_us < 4611686018427387904)`. A use that breaks it fails
   with `SQLITE_CONSTRAINT_CHECK` and changes nothing; map exactly that error (extended code 275 on that column, not any constraint) to `LocalFileCacheError::DatabaseState("use order exhausted …")`.
   Test it with a database whose maximum is 2^62 − 1 (the next use fails; the row is unchanged) and with a hostile v5 file (`last_accessed_at` = 2^63 − 1): the migration must fail
   and leave the database as it was (v5, unchanged bytes).
2. **Fresh equals migrated.** `create_fresh` defines the v6 `files` table with the same columns in the same order as a migrated table (the new column last, after `payload_version`),
   same default and check. The classifier compares the **structure** (`PRAGMA table_xinfo`: name, type, notnull, default, pk, hidden) and the index set and columns, **never the SQL text**.
   A test opens a fresh v6 and a migrated v6 and asserts identical `table_xinfo` and identical index sets.
3. **The use statements** are the RFC's R1 forms, with `now_us` taken once per statement. The same expression in `touch`, the `get`/`get_if_fresh` hit, both upserts (insert **and** the
   `ON CONFLICT … DO UPDATE` branch: an overwrite is a use), `set` and `batch_set`. A `get`/`get_if_fresh` hit ignores a failed use as today (`let _ =`), documented on those methods; `touch`, `set`
   and `batch_set` return it.
4. **Eviction** `ORDER BY last_used_us ASC, id ASC`; update the comments and the rustdoc that say "least recently read" or "never-read first".
5. **Public seconds.** Every read of the old column becomes `last_used_us / 1_000_000` (integer division) into the existing `i64` fields. Types, names and the Rust-side sort stay.
6. **Import (F2).** Per R2: `s = last_accessed_at` if above 0 else `updated_at`; reject the **whole import before writing** with `InvalidArgument { argument: "records" }` (reason: the recency of a record is out of range)
   when any `s` is negative or `s × 1_000_000` is not below 2^62 (checked arithmetic; no float ever). Value stored: `s × 1_000_000`.
7. **Export (F3).** Order by use, oldest first: `ORDER BY last_used_us ASC, id ASC`. Document the order on `export_entries`. Test: export then import into a fresh database keeps the relative recency of entries written
   within one second.
8. **The migration (R3).** `migrate_v5_to_v6`, in the order your step 0 found SQLite forces: add column (with the check) → the one backfill statement → drop both indexes → drop `last_accessed_at` → create `idx_files_use`
   → postconditions → `user_version = 6` last. The backfill is the window statement of your request § 3 (never a row loop). Postconditions as R3 step 3: classifier accepts exact v6; row and payload counts and digests unchanged;
   per namespace strictly increasing in the rule's order; every value an integer below 2^62. Crash-injection points in the existing style (`MigrationPoint`), one per step. `CURRENT_VERSION` is the single constant (Q8b) and the test that pins it changes to 6.
   Also: a read-only open of v5 reports `SchemaNotCurrent`; a v6 database opened by this code is current; the v1→…→v5 migrations are untouched and chain on to v6.
9. **Out of scope here (Q13):** the CLI, `docs/`, `README.md`, `CHANGELOG.md`, the numbers on `performance.md`.

## 2. Tests (failing before, where a behaviour changes)

- The pathology: `max_entries(3)`, set `a b c`, get `a`, set `d … h` → `a` is evicted (it survived before; run it on the base first and say so).
- `touch` protects inside one microsecond (needs a clock you can fix in a test: a `#[cfg(test)]` override of `now_us`, in the existing style of `test_hook`); a write is a use; an overwrite is a use; a read-only engine and a `ReadPool` slot never write.
- Migration from **every** historical fixture (v1 to v5; `tests/fixtures/` and the in-crate builders) reaches v6 with every entry and payload intact; a second open is a no-op; every failure-injection point leaves the database byte-identical to v5;
  the backfill equals an **independent** implementation of the recurrence (write it in the test; 100,000 rows, including 200,000 rows sharing one second); hostile values (F1).
- Concurrency: four engines on one file, eight threads, a mixed workload for a few seconds; afterwards `SELECT namespace, last_used_us, count(*) … HAVING count(*) > 1` is empty.
- Plans: the `EXPLAIN` tests (`db/repository/tests.rs`) now expect `sqlite_autoindex_files_1`; the eviction plan uses `idx_files_use`; the Q7 range plan is a covering search on the autoindex.
- Import/export: F2 (a range failure writes nothing), F3 (order; relative recency survives a round trip).
- Every other test passes; the compat fixtures pass **unmodified** (they are v1 to v5 databases: they test the migration now).

## 3. Measurements to report (no claim without both arms, paired, same host, alternating)

1. **The warm hit, through the engine** (the open item of step 0, F5): `cache_bench` `warm_json_hit` and the `scale_profile` `get` rows, base tree against this tree, at least 5 alternating rounds, with host load. State the median ratio.
   **If a warm hit is more than 10% slower, report it and stop for the architect's decision; do not tune it silently.**
2. The migration of a 1,000,000-entry database through the crate (not your scratch tool): time, peak RSS, WAL size, free space needed; and the file size and insert-time effect of dropping the duplicate index measured through `set`/`batch_set`.
   Report the numbers with their limits; the architect decides what the book says (Q13).

## 4. Gates and the request

`cargo fmt --all --check`; `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`; `cargo make gate-features` (the matrix, not only `--all-features`); `cargo test --workspace --all-features --locked --no-fail-fast`
(626 now; report the total); `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked`; `cargo make gate-versions`; `rustup run 1.85.0 cargo make gate-msrv`; `cargo make gate-deny`; `cargo make bench-compile`; `mdbook build docs`.
File `.git-exclude/review-request/NNN-dev-q12-<date>.md` describing the **uncommitted** tree, with a table of every changed file and why; commit locally after approval (suggested split: the migration and classifier; the repository and engine; import and export); no push, tag or version bump. List every required step and every judgement call.

## 5. Not yours

Q13's files; the version; the decision to drop A (the architect decided: A stays); RFC 029.
