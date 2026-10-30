# Q2f Handoff — Three Behaviour Changes (RFC 024 B1, B2, B3; RFC 025 R6 row 4)

RFC: `rfcs/accepted/024-api-consistency.md`, Part B, **B1, B2, B3**, and `rfcs/accepted/025-error-taxonomy-and-input-validation.md`
R6's fourth row (the import bound, specified once, here).
Scope: authorized by the owner on 2026-10-29; each change was announced in v0.21.5. Order: … Q3c → **Q2f** → Q3e →
Q10 → release preparation. Start when Q3c is committed locally. **Local commits; do not push.**
Format of the review request: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6.
Authorship: high-capability model. Authorized: the owner.

## 0. What this is

Three behaviours change in v0.22.0. **One commit per change (B1, B2, B3), in this order, in one review request**,
each with its own failing-before test, so each can be reverted alone. If something is not in this file, **stop
and file a design request**.

## 1. B1 — `QueryBuilder::run` returns an undecodable entry's error

`run()` returns `Err` with the **first `SkippedEntry`'s error** exactly when `run_report().skipped` would be
non-empty for the same query; otherwise it returns what it returns today. The two share one execution path, so
the rule cannot drift (RFC 024 B1, clarified by review 021). The inherited scope of `skipped` is unchanged: rows
passed while producing the page; every candidate on the decode-everything tier; never a row with no payload row
(which `get` also treats as a miss).

- Implement it by making `run()` call the same execution as `run_report()` and convert a non-empty `skipped`
  to its first error. Report the diff size and that no second code path exists.
- "First" is in result order of the query (the order the entries would have been returned).
- Every wrapper's `run`/`dry_run` equivalent (sync, async on each runtime) follows. `dry_run()` does not decode
  payloads and is unchanged: say so after checking.
- **Tests (failing before):** under a wrong encryption key, `run()` returns `Err(EncryptionError(_))` (it
  returned `Ok(vec![])`), and `run_report()` still lists every row in `skipped`; an entry whose encoding tag is
  unknown makes `run()` return `Err(UnknownEncoding(_))`; a query where nothing is skipped returns exactly the
  same entries as before (guards: ordering, `offset`/`limit`, every tier); `batch_get` and `get` unchanged.
- **Docs:** `docs/src/querying.md` and the rustdoc of `run`, `run_report` and `SkippedEntry` say: an error unless
  you ask to skip. Re-read the pages for any sentence that says `run()` skips undecodable entries.

## 2. B2 — An engine that does not choose a journal mode leaves the database's alone

- `JournalMode` gains **`Preserve`**, which becomes the `#[default]` (and `CacheOptions::default().journal_mode`
  and the builder's default). On a **new** database it applies WAL, as today. On an **existing** database it
  leaves the journal mode unchanged. `Wal` and `Delete` stay explicit choices, applied as today.
- `JournalMode` becomes `#[non_exhaustive]` in the same commit.
- **"New" means** the database has no localcache schema when the engine opens it (the open classifies it as
  fresh: the file did not exist, or it is an empty file). Report exactly how the code decides this, in the
  request, and that an existing database with the current or an older schema counts as existing.
- The migration precondition (a rollback-capable or WAL journal) is unchanged. A `Preserve` open of a database
  in `OFF` or `MEMORY` mode is refused by the existing `DatabaseState` check, as a `Delete` open would be.
  In-memory databases behave as today.
- Read-only opens and `ReadPool` never change a journal mode (as today); show it.
- The CLI's writable commands inherit `Preserve`, so they stop converting databases. Find where the CLI sets or
  inherits the mode and report.
- **Tests (failing before):** a database created in `DELETE` mode stays `DELETE` after a default open, a write
  and a close; one in `WAL` stays `WAL`; a new database becomes `WAL`; an explicit `Wal` converts a `DELETE`
  database and an explicit `Delete` converts a `WAL` one (guards); a `Preserve` open of an `OFF`/`MEMORY`-mode
  file database is refused with `DatabaseState` and the file is unchanged; the CLI's `set`/`import` on a
  `DELETE` database leaves it `DELETE`.
- **Docs:** `docs/src/` pages that say the default is WAL (grep `journal`, `WAL`), the rustdoc of `JournalMode`,
  `CacheOptions::journal_mode`, and the CLI page. State which behaviours are unchanged.

## 3. B3 — Imports honour `max_entries` (and R6's fourth row)

`import_entries` and `import_from` enforce `max_entries` **under the same rule as `batch_set`** (RFC 022 R6, Q3c):
- the eviction runs in the import's own `IMMEDIATE` transaction;
- it never evicts a row the call wrote;
- an import of more **distinct** entries than `max_entries` is rejected **before writing**, with
  `InvalidArgument { argument: "records", .. }` (for `import_from` too; add nothing to the name set). Distinct
  means the stored key; the same path twice in one import counts once.

One bound, enforced by every write path. Reuse `enforce_max_entries` as `batch_set` does; do not copy it.

- `namespace_copy` is removed in Q3e; if it shares the import code path it gets the rule automatically, and you
  say so. Do not write new code for it.
- **Tests (failing before):** an import of 6 distinct records with `max_entries(3)` is rejected before any row is
  written (entry count and database unchanged); an import of exactly 3 into an empty database is accepted; an
  import of 2 records into a database already holding 3 (max 3) evicts the least-recently-used older rows, keeps
  both imported rows, and ends at 3, all in one transaction (a failure injected after the write and before
  commit leaves the database as it was, using the existing test hook); duplicates count once; `import_from`
  behaves the same; with no `max_entries` nothing changes (guard).
- **Docs:** `docs/src/` (import, portability, `max_entries` pages), the rustdoc of `import_entries`,
  `import_from` and `max_entries`. The ROADMAP deferred-register row about imports bypassing `max_entries` is
  closed by this commit (the architect updates it).

## 4. CHANGELOG, gates, request

- `CHANGELOG.md`, `## [Unreleased]` / `### Changed`, one **Breaking** entry per change (B1, B2, B3), each with
  who is affected and what to do (`run_report()` to tolerate undecodable entries; `JournalMode::Wal` to keep the
  old default; split or size imports to `max_entries`). `JournalMode` being `#[non_exhaustive]` is part of B2's
  entry.
- Gates as in Q3b's handoff § 6. One review request `.git-exclude/review-request/NNN-dev-q2f-…-<date>.md`
  describing the **uncommitted** tree, with the three changes separable (a patch per change under
  `.git-exclude/tmp/`, as in Q9b); commit locally as three commits after approval; no push, tag, version bump or
  `ROADMAP.md` edit. List every required step and every judgement call.

## 5. Not yours

The removals (B4, Q3e); the `ConnectionPool` strings; new error variants beyond Q3b/Q3c; the bounded-decode
error (Q10); anything about schema v6 (v0.23.0).
