# Q12 After Review — one change, one commit, one attribution

RFC: `rfcs/accepted/026-use-order-and-one-path-index.md` (already updated for everything below). Review that raised it: `.git-exclude/reviewed/075-architect-q12-schema-v6-review-2026-11-03.md`.
Format of the review request: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6. Local commit; **do not push**.
Authorship: high-capability model. Authorized: the owner (RFC 026).

## 0. What is decided

The Q12 tree is approved for commit with **one change** (§ 1). The warm-hit cost is accepted as measured (RFC 026, Test plan, "The warm hit"); only its attribution is open (§ 3).

## 1. The change: the migration's range is the import's range

A v5 row whose last-use second `s` is negative, or whose `s × 10^6` reaches 2^62, fails the migration with the error you already use for the top of the range
(`DatabaseState`, "a stored recency is outside the range the use order can hold"), the file byte-identical and `user_version` still 5. Today only the top is refused
(your § 9 judgement 2). After this change every stored use value is in `[0, 2^62)`, the same range the import enforces, so what a database can hold, an export can
carry back.

* Make the check before the backfill, as a read, so the message is the same whichever side is out of range. Keep the backfill one statement. `validate_use_order` must also require `0 ≤ stored` (today it recomputes the same negative number and accepts it).
* Tests: the existing test that a negative `updated_at` migrates to a negative value becomes a test that it fails, bytes identical, version 5, the retry after the row is fixed succeeds; plus a negative `last_accessed_at` with a positive `updated_at` (still "never read", still valid: `s = updated_at`).
* The CHECK itself is **not** changed (a use can never produce a negative value; `MAX + 1 ≥ 1`).

## 2. The commit

**One commit** for the whole tree, code and tests together: `feat!: schema v6 and the use order (RFC 026)`. Your suggested three-way split would leave commits that do not build (the repository and the migration refer to each other's column). Before it, rerun every gate of Q12 § 4
on the committed tree, plus `cargo make gate-docs` and `cargo make gate-archive` (they need a committed tree), and report the results.

## 3. The one open measurement: attribute the `scale_profile` rise

Your § 6 point 2: a 12% rise on a 2 ms `get` is about 300 µs, which a 6.6 µs statement cannot explain. Find out what it is, **without changing any code of the crate that is committed**. Use a throwaway worktree and delete it.

1. Check `/proc/loadavg` before every round and write it in the log. Use rounds only when the load is below the number of cores divided by four; if the host never gets there, say so and treat the result as a bound, not a value.
2. Reproduce the `scale_profile` rise with the micro harness at 100,000 entries, with the payload size `scale_profile` uses, one hot entry and spread hits, seven alternating rounds.
3. In the throwaway worktree only, separate the pieces of the statement: (a) the base statement; (b) the head statement; (c) the head statement with the `MAX` sub-select replaced by a constant (still moving the index entry); (d) a plain update of a column with no index. The
   differences say which piece costs what.
4. Report the table, the loads and the conclusion in the next request.

**Stop condition (new, absolute).** If the statement's cost per hit exceeds **50 µs** in a 100,000-entry database, or grows with the payload size, stop and report; that is what the 10% rule was for. Anything at or below the 6.6 µs figure is the accepted cost. If the 300 µs turns out to be the host,
say so with the loads that show it.

## 4. Then Q13

Start `q13-docs-cli-and-the-announcement-made-true.md` after the commit (§ 2). Three facts from request 054 that it must state, and one it must not:

* The export order changes (oldest use first). State it in `portability.md`, the rustdoc is done, and the CHANGELOG.
* The migration needs about 0.6 of the database's size in free space at 1,000,000 entries and takes about 8 seconds (request 054 § 7).
* **A migrated file does not shrink** (507 → 529 MiB); a new one is smaller and `shrink_database()` reclaims the pages. No sentence of Q13 may promise a smaller file after migrating; `migration.md` and `CHANGELOG.md` `[0.22.2]` already say this.
* The hit-cost figure goes into `performance.md` only after § 3 has attributed it, with its limits (a loaded host, 64-byte payloads, a 100,000-entry database), never as a promise.

## 5. Report

One request: the change and its tests (§ 1), the commit hash and the gates (§ 2), the attribution table (§ 3). Then the Q13 request when Q13 is done. Housekeeping you may do after the commit: delete `.git-exclude/tmp/s0/` (2.8 GB) and `q12m/`'s databases and 100,000 source files; keep the harness sources and the logs.
