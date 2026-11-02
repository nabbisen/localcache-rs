# RFC 026 — A True Use Order for Eviction, and One Path Index (Schema v6)

| Field | Value |
|-------|-------|
| Status | Accepted (owner, 2026-11-01), both decisions as recommended |
| Feature | *(core)* |
| Touches | `db/schema*.rs` (migration `v5 → v6`, classifier), `db/repository.rs` (use assignment, eviction), `cache/engine*.rs`, `cache/engine/portable.rs` (export, import), `docs/src/` (builder, architecture, performance, api, migration), the compat fixtures |
| Release | **v0.23.0** (a minor release: a documented policy changes, and the schema changes) |
| Scope document | `.git-exclude/reviewed/071-architect-v0.23.0-scope-reviewed-against-the-philosophy-2026-11-01.md` |

## Summary

Eviction orders entries by their **use**, reads and writes alike, in a strict total order that does not depend on a clock's resolution. The same
schema change removes an index that duplicates another. One migration, once.

## Motivation (observed on the v0.22.1 tree, 2026-11-01)

1. **An entry read once, ever, is immortal.** `max_entries = 3`; write `a`, `b`, `c`; read `a`; then write `d` to `h` and read nothing: the survivors
   are `a`, `g`, `h`. A new row starts at `last_accessed_at = 0` ("never read") and only a read raises it, so eviction (`ORDER BY last_accessed_at,
   updated_at, id`) always prefers unread entries over any entry that was ever read. RFC 022 R6 recorded true least-recently-used as the principled end
   state and Amendment 2 recorded why the cheap form (a write sets `last_accessed_at = now`) does not work: at one-second resolution a write and a
   later `touch` or `get` tie, the tie-break evicts the entry just touched, and `touch` stops protecting from eviction.
2. **A duplicate index.** `idx_files_namespace_path` on `(namespace, path)` duplicates the index SQLite already builds for `UNIQUE(namespace, path)`.
   Measured on the real schema, 300,000 synthetic rows: **file size −22.3%** (157.5 → 122.4 MB, identical in three rounds), insert time −11% to −15%.
   With it dropped the planner still serves path lookups and the `path_in_dir` range query from the covering `sqlite_autoindex_files_1`
   (checked on `EXPLAIN QUERY PLAN`).
3. A schema change is a one-way door for every user's database. Making both changes in one migration costs users one door, not two.

## Goals

1. Eviction removes the entry whose last **use** is oldest; a use is a read hit, a write, an overwrite or a `touch`. No two uses ever tie.
2. One path index. No redundant stored state.
3. Public API unchanged in shape; the meaning of one field is stated exactly (R2).
4. A migration that preserves every entry and payload and is as safe as the existing ones (RFC 010).

## Non-goals

- A new eviction policy (LFU, ARC, TTL-aware). One policy, made true.
- A change to payload encoding, encryption, or the wire format (RFC 029, if the owner takes it up, is a separate release).
- Any change to `max_entries` rules (RFC 022 R6, RFC 025, RFC 024 B3 stand: a call never evicts what it just wrote).

## Requirements

### R1 — One column, one order

The column `last_accessed_at` (Unix seconds) is replaced by **`last_used_us`**, an `INTEGER NOT NULL`: a *use value*, in microseconds, that is a hybrid of
the clock and a counter. A use of an entry assigns it

    last_used_us = max(now_us, (SELECT COALESCE(MAX(last_used_us), 0) + 1 FROM files WHERE namespace = ?))

in the **same statement** as the write or the touch, so it is strictly greater than every other value in the namespace and the order is total and
independent of the clock's resolution. It is monotone through a clock that steps backward. SQLite serializes writers, so the statement is atomic
across connections and processes. Eviction becomes `ORDER BY last_used_us ASC, id ASC` (the `id` is defensive; it decides only the ties R2 names, between imported rows).

Uses: `get` and `get_if_fresh` hits (as today), `touch`, `set`, `batch_set`, and an overwrite (all new). Read-only engines and `ReadPool` slots never
write, so never assign (as today). `check_status`, `explain` and queries are not uses (as today).

**Overflow.** A value must be an integer below 2^62. SQLite does not wrap or fail at the limit of `i64`: `MAX + 1` at 2^63−1 becomes a **float** and is stored as
one (verified, step 0). So the column carries the rule itself:
`last_used_us INTEGER NOT NULL DEFAULT 0 CHECK (typeof(last_used_us) = 'integer' AND last_used_us < 4611686018427387904)`. A use that would break it fails with
`SQLITE_CONSTRAINT_CHECK` and changes nothing; the engine maps that error to `DatabaseState` ("use order exhausted"). The fresh v6 table is defined with the
same column (same position, default and check) as the migrated one, and the classifier compares the table's structure (`PRAGMA table_xinfo`), never its SQL text.

**Where a failed use is reported.** `set`, `batch_set` and `touch` return the error. A `get` or `get_if_fresh` **hit** keeps today's behaviour: the value is
returned and a failure to record the use is ignored, because a read must not fail for want of bookkeeping (documented on those methods). Only a hostile file can
make a use fail, so this costs nothing in practice.

### R2 — What the public fields mean

`EntryInfo::last_accessed_at` and `ExportRecord::last_accessed_at` stay `i64` seconds: `last_used_us / 1_000_000`. Their meaning becomes **"when the entry
was last used (read or written)"**, replacing "last read; 0 = never read". A stored entry therefore never reports `0`. `SortKey::LastAccessed` sorts by
it. The rustdoc, `README.md`, `builder.md` and `architecture.md` say so, and say that the eviction order is finer than the public seconds.

**Import** keeps the exported recency: an imported row gets `s × 1_000_000`, where `s` is the record's `last_accessed_at` if it is above 0, else its `updated_at`
(the same rule as R3 step 1; an import is a restore, not a use). `s` must be non-negative and `s × 1_000_000` must be below 2^62, computed with checked
arithmetic; a record outside that range makes the whole import fail before anything is written, with `InvalidArgument { argument: "records" }`. Two rows may
share a value; `id` (the order in which the import inserts them) breaks that tie. **Export is ordered by use, oldest first, ties by `id`** (it was ordered by
`updated_at` descending, with no tie-break, and the order was not documented), so an export made by v0.23.0 imports with its relative recency intact even within
one second; an export made by an older version has no finer information to keep. The strict rule is about uses, and every later use exceeds every stored value.

### R3 — The migration `v5 → v6` (one transaction, RFC 010)

1. Compute each row's last-use seconds `s = last_accessed_at` if it is above 0, else `updated_at` (a never-read entry is ordered by when it was written,
   among the others: this is the intended change). Per namespace, in order of `(s, updated_at, id)`, assign `v_1 = s_1 × 10^6` and
   `v_i = max(s_i × 10^6, v_{i-1} + 1)`. No two rows share a value, the order of `s` is preserved, and more than 10^6 rows in one second cannot collide.
2. The order SQLite forces, all in the one transaction: add the column (with the check of R1); run the backfill as **one SQL statement** (`UPDATE … FROM` over window
   functions: `v_i = i + max_{j≤i}(s_j × 10^6 − j)` is the same recurrence in closed form, verified against an independent implementation, step 0); drop `idx_files_lru` and
   **`idx_files_namespace_path`** (a column cannot be dropped while an index uses it); drop `last_accessed_at`; create `idx_files_use` on `(namespace, last_used_us)`.
   The backfill must not be written as a row-by-row loop (step 0: 345 s against 4 s at 1M rows).
3. Postconditions, checked before `user_version = 6` is set last: the classifier accepts exact v6; the row count and every payload are unchanged; per
   namespace the values are strictly increasing in the order of step 1; no value is at or above 2^62.
4. The classifier expects, for v6, the objects of R3 step 2 and nothing else, with the structure of R1. Crash-injection hooks as in the existing migrations; a failed migration
   leaves the database as it was.

Resources, stated: the transaction writes about as much write-ahead log as a fifth to four fifths of the database (measured: 0.2 to 0.8 GB for 1 to 2 million
entries) and `DROP COLUMN` rewrites the table, so the migration needs free disk space of up to about the size of the database and takes seconds to tens of
seconds at a million entries on a good disk; on a full disk it fails and leaves the database as it was. The existing migrations share the property; the
announcement and the migration page say it.

Compatibility consequences, stated: **a migrated database is refused by v0.22.x and earlier** (their classifier expects the dropped objects); rollback
means export, then import into an older version. A read-only open of a v5 database reports `SchemaNotCurrent` until a writable open migrates it, as
with every earlier bump.

### R4 — The documents tell the truth

Every statement of "least recently **read**", of `last_accessed_at == 0`, and of `idx_files_namespace_path` (the book's `EXPLAIN` outputs now name
`sqlite_autoindex_files_1`) is rewritten. `performance.md` records the measured size and insert gain, with its limits.

## Test plan

- **Step 0, done (request 052, 2026-11-01; its findings are folded into R1 to R3 above).** On scratch copies with the bundled SQLite (3.53.2): 4.5 million use values returned
  to eight concurrent connections (separate processes and threads; a real, a stuck and a backwards clock) with no duplicate; the overflow behaviour; the backfill at 1M
  and 2M rows sharing one second, in at most 20 MiB; the 26 real statements on the v5 and v6 shapes (no path or namespace plan changes shape, `path_in_dir` included);
  every reader of `last_accessed_at`. Its verdict: the design stays simple. **Still to measure, at implementation:** the cost of the `MAX` seek on a warm `get` hit
  through the engine (0.3 to 5.9 µs of CPU in memory), reported, with a regression above 10% of the hit's time brought to the architect.
- The observed pathology as a test: the `a`/`g`/`h` sequence now evicts `a`.
- `touch` protects from eviction even inside one microsecond; a write counts as a use; an overwrite counts; read-only engines write nothing.
- Migration from **every** historical fixture (v1 to v5) reaches v6 with all entries and payloads intact; the failure-injection points of the
  existing framework; the idempotence of a second open; a hostile database with a use value at 2^62 fails with the typed error.
- Export then import round-trips the public seconds and preserves relative recency; `order_by(LastAccessed)` orders by last use.
- The `EXPLAIN` plans in the book are asserted; insert time and file size are measured through the crate on the same host, before and after.

## Security considerations

No new input. The migration keeps RFC 010's strictness: an unrecognised schema is refused and left unchanged. The overflow rule (R1) closes the one new
arithmetic hazard. No stored payload byte changes.

## Compatibility

Breaking: the stored schema (one-way, R3); `last_accessed_at` means last use; eviction order changes for databases that mix read and
unread entries (the intended change). The wire format of payloads, the public API's shape, and `max_entries` rules are unchanged. v0.23.0 is a minor
release; the CHANGELOG states the migration and the rollback route.

**The announcement owed.** The v0.21.5 notes say of this change: "A later release (v0.23.0) is planned to replace the LRU eviction policy, with a schema
change. It will be announced in detail one release ahead." v0.22.0 and v0.22.1 carried no detailed announcement. The project's own rule (Phase 24: a breaking change
is announced one release ahead) is met by a **documentation-only patch, v0.22.2**, that states R1 to R4 and the rollback route in the CHANGELOG, the book and the
README before v0.23.0 ships (decision 2).

## Alternatives considered

- **A millisecond clock.** Ties for operations faster than a millisecond (a WAL `set` can take tens of microseconds); "touch protects" would depend on timing.
- **A separate sequence column beside `last_accessed_at`.** Two sources of truth that must be documented not to disagree; one more index to maintain,
  which undoes part of the gain of dropping the duplicate.
- **Keep the schema; change only the tie-break.** Cannot make a touch outrank a write in the same second.
- **Drop only the duplicate index (B alone).** The floor, if step 0 fails; but a later A would cost every user a second one-way migration.
- **Keep the duplicate index.** A permanent 22% size and write cost for nothing.

## Rollback

Export, then import into v0.22.x. A revert of the release restores the old code, which refuses v6 databases; the CHANGELOG says so.

## Decisions

Accepted by the owner on 2026-11-01 with the recommended answers:
1. RFC 026 as written: `last_accessed_at` means last use (R2); an import keeps the exported recency and is not a use (R2); the one-way migration (R3);
   the removal rule if step 0 fails.
2. The announcement owed is made by a **documentation-only v0.22.2** before v0.23.0 (see Compatibility).

Handoffs: `../handoffs/026-use-order-and-one-path-index/` (v0.22.2 preparation, step 0, then Q12 and Q13).

## Open questions

None.
