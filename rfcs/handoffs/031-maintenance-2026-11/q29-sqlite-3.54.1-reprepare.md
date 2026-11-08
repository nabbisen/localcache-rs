# Q29 — Does SQLite 3.54.1's statement re-preparation cost us anything? (measure first)

Raised by the architect on 2026-11-08 while checking `rusqlite/rusqlite` for existing reports of CVE-2026-119375 (Q27). `rusqlite` issue **#1931** (open, 2026-10-29), "Bundled SQLite `3.54.1` repeatedly recompiles cached statements", reports that with SQLite 3.54.x a statement from `prepare_cached` can be **re-prepared on every use**: the planner marks some `col = ?` parameters in two internal masks while estimating row counts; `rusqlite`'s cache clears the bindings when it returns a statement; `sqlite3_clear_bindings` then expires the statement if a marked parameter is in one of the masks, and the next `sqlite3_step` recompiles it (their example: about 25% less throughput in an exporter; SQLite fixed it upstream on 2026-10-17 on trunk and `branch-3.54`; no release since, 3.54.1 is still the latest on 2026-11-08). **`localcache` 0.26.0 bundles 3.54.1 and calls `prepare_cached` at 15 sites** (`crates/localcache/src/db/repository.rs`), most of them `WHERE col = ?`. We do not know whether we are affected, or by how much. This task finds out; it changes no code.
Format of the review request: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6.
Authorship: high-capability model.

## 1. What to establish

1. **Read the issue and its reference** (public; the issue text, the linked SQLite check-in `535a77cf5a8e1a7b`, and #1920 it mentions). State, in your words and with the lines you read, the condition under which a cached statement is expired by `sqlite3_clear_bindings`, and **which of our 15 statements meet it** (a statement-by-statement table: SQL text, parameters, whether the planner would mark them, how you know). If you cannot decide a statement by reading, decide it by measurement (item 2).
2. **Count re-preparations directly.** SQLite exposes the count (`SQLITE_STMTSTATUS_REPREPARE`, `rusqlite::StatementStatus::RePrepare`). For each of our cached statements, exercised through the engine as a user does (set, get, touch/use order, delete, evict, list, stats), report the re-prepare count per execution on **SQLite 3.53.2** (the tree at `0.25.1`) and on **3.54.1** (the tree at `0.26.1`/`main`), over a fixed number of operations. A harness with a test-only hook in a scratch copy of the tree is fine; **no change to the committed tree**. If the count cannot be read from outside, say what you did instead.
3. **Measure the cost on the operations users run**, paired and alternating as in request 080: `set` and `get` of a 64-byte entry, `get` of a hit with the use order on and off, `delete`, a range/glob listing, `evict`, on 0.25.1 (3.53.2) against `main` (3.54.1); host, load, rounds and ranges stated, no promise.
4. **What would remove it, if it is real,** with the cost of each: (a) wait for a SQLite release that carries the upstream fix, and `rusqlite`/`libsqlite3-sys` that bundles it (our `gate-requirements` will say when SQLite publishes it: the row `SQLite (bundled)` fails); (b) a build option or a `PRAGMA`/db-config that avoids the condition (name it only if you read it in SQLite's documentation or source); (c) a change in how we use the cache. Say what each would cost in code and in behavior. **Do not propose or make any change in this request**; the architect and the owner decide after the numbers.

## 2. The request

One review request: the table of item 1, the counts of item 2 on both versions, the measurements of item 3, the options of item 4, the path of the harness. If the counts are zero on 3.54.1 for every statement, say so plainly and stop: that is the answer.

## 3. Not yours

Any change to the committed tree, the version, `rusqlite`, a workaround, a report or comment on the `rusqlite` or SQLite trackers (the architect asks the owner), `ROADMAP.md`.
