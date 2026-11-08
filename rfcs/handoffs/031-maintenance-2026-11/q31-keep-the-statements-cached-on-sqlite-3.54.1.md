# Q31 — Keep our prepared statements from being recompiled on SQLite 3.54.1

Follows Q29 (request 089, review 108): on SQLite 3.54.1, six of our fourteen cached statements are re-prepared on every use, among them the two on the hot path (`RECORD_USE_SQL`, `UPSERT_SQL`), which makes `set` of a 64-byte entry about 1.97× and `get` of a hit about 1.67× as slow as on 3.53.2. It is released in v0.26.0 and v0.26.1. The owner has not yet decided the remedy; **stage 1 is scratch work and changes no committed file**, so it can start now. Stage 2 (commits) starts only when the architect says so in a review.
Format of the review request: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6.
Authorship: high-capability model.

## 0. The constraints (the owner's philosophy, applied)

* **No SQL text built from user input.** The security page says hostile text is never echoed into SQL; every user value stays a bound parameter. This rules out writing the namespace into the SQL as a literal (your option c1).
* **No `SQLITE_DBCONFIG_ENABLE_QPSG`** (your option b: 17× on a selective glob).
* **No behavior, schema, stored-byte or public-API change.** Query plans must stay the ones we have.
* Less is more: the smallest change that works, with a comment that names the defect, the SQLite release that will end the need, and how to find every site again (one marker word, for example `reprepare-workaround`, greppable). Temporary by design, like a `deny.toml` exception: it carries its ending condition.

## 1. Stage 1 — find out whether a text-level rewrite of the six statements works (scratch only)

The planner is peeking at the bound value of `namespace = ?` (the shape of request 089 § 1: equality on the leading column of an index in a statement that also orders, groups or takes `MAX` over the next column), and SQLite 3.54.1 then marks that parameter, so `sqlite3_clear_bindings` (called by `rusqlite`'s cache when the statement returns) expires the statement. The peek is done only for a bare parameter in some expression forms. **Candidates to test**, each keeping the parameter bound and the meaning identical (and others if you find them in the 3.54.1 source you already read):

1. `namespace = CAST(?1 AS TEXT)`; 2. `namespace = (?1 || '')`; 3. `namespace = +?1`; 4. `namespace = COALESCE(?1, NULL)`; 5. a bound parameter inside a scalar subquery or a CTE; 6. anything the source shows is not a variable at the call sites.

For each, on the standalone reproduction of request 089 (`logs/shape.txt` shapes, 1,000 uses on 3.54.1) and then on the six real statements in a scratch copy of the tree:

* **the re-prepare count** per use (`StatementStatus::RePrepare`), from the harness you built;
* **`EXPLAIN QUERY PLAN` identical** to the current statement's, text for text (a wrapper that makes the planner stop using the index is not acceptable: show the plans side by side for all six);
* the **results identical** on the workload of request 089 § 2 (rows, order, counts), including a namespace that contains `%`, `_`, quotes, a NUL-free but unusual Unicode string, and the empty string; `CAST(… AS TEXT)` or `||` must not change how a namespace compares (collation, affinity): show a case with a namespace that looks like a number (`'1'`, `'01'`, `'1e0'`) in a TEXT column;
* the **timing** of the operations of request 089 § 3 (paired, alternating, the same builds) for the best candidate, against 3.53.2 and against unmodified 3.54.1.

Also report, only as information: whether the same rewrite removes the four `LIMIT ?` re-prepares that 3.53.2 also has (not a regression; do not include it in stage 2 unless the architect asks).

If no candidate gives 0 re-prepares with identical plans and results, say so and stop: the remedy is then to wait for the SQLite release (Q29 option a) and to say so in the book.

## 2. Stage 2 — only after the architect's decision (not yet authorized)

The commits: the rewrite of the affected statements; **a regression test** that runs the hot operations (`set`, `get` with use recording, `touch`, eviction) a few hundred times on a scratch cache and asserts the re-prepare count of every statement is below a stated small number (it must fail on the unmodified statements: show the failing run as the negative control, as in request 090 § 2); the book (`docs/src/performance.md`: a short, dated known-issue paragraph for 0.26.0 and 0.26.1 with the measured ratios and their limits; no promise); the CHANGELOG entry; and the verification as for a patch (stored bytes unchanged, plans unchanged, the twelve gates). Version commit and release candidate follow separately.

## 3. The request

One review request for stage 1: the candidate table (count, plan, results, timing), the recommendation with reasons, the path of the harness. Do not commit, push or comment on any tracker.

## 4. Not yours

The decision, the version, tags, `ROADMAP.md`, `docs/src/roadmap.md`, any message to the `rusqlite` or SQLite projects.

## 5. Stage 2 — authorized to start (review 109, 2026-11-08)

Stage 1 (request 091) is accepted: `CAST(?N AS TEXT)` on the six statements gives zero re-prepares with text-for-text identical plans and byte-identical results, and the timings return to 3.53.2's (0.95 to 0.99×). Chosen form: **`CAST(?N AS TEXT)`**, not `+?N`, for the reasons in request 091 § 5. Do stage 2 as local commits on `main` (on top of your two Q30 commits); **do not push, version or tag until the architect has the owner's decision on v0.26.2** (the owner was asked on 2026-11-08; the work is reversible and the release is theirs).

1. **The six statements** (`UPSERT_SQL`, `RECORD_USE_SQL`, `list_entries`, `load_all_full`, `encoding_breakdown`, `count_by_version`): `namespace = ?N` becomes `namespace = CAST(?N AS TEXT)` and nothing else changes in them. At each site a comment with the three things of request 091 § 5: the defect (SQLite 3.54.0 and 3.54.1 expire a cached statement on `sqlite3_clear_bindings` when the planner marked a bare parameter; `rusqlite` #1931; SQLite check-in `535a77cf5a`), the ending condition (a SQLite release that contains the fix, bundled by a `libsqlite3-sys`; the day SQLite publishes one, `gate-requirements` fails the release, which is when this is removed), and the marker word **`reprepare-workaround`**. Do not touch the four `LIMIT ?` statements (not a regression) or the JSON-field queries.
2. **The regression test** of request 091 § 5: it must name the six statements through constants shared with the code (not a copy of the SQL), run the hot operations and the listings a few hundred times, read `StatementStatus::RePrepare`, and assert each of the six is below a small stated bound. Negative control: show the failing run with the cast removed (about 999 of 1,000). The test does not fail on SQLite 3.53.2 with or without the cast, and must say so in its comment. It must not weaken the layering rule (`tests/layering.rs`: nothing above `db` names `rusqlite`): keep the test inside the database layer.
3. **Book and CHANGELOG.** `docs/src/performance.md`: a short, dated known-issue paragraph: v0.26.0 and v0.26.1 bundle SQLite 3.54.1, which recompiles six of our cached statements on every use; the measured ratios of request 091 § 3 with their limits (one host, 64-byte payloads, 200 or 5,000 entries); fixed by the release that contains the rewrite (no version number until the architect names it); the cause is a SQLite defect, reported upstream as `rusqlite` #1931, and the workaround is removed when SQLite releases the fix. State the four `LIMIT ?` statements honestly as older behavior, not changed. CHANGELOG `[Unreleased]` (it already holds the Q30 *Security* entry): add *Fixed* (one entry, with the ratios and the limit) and fold both into one plain opening paragraph, "A patch. There is nothing to do when you upgrade." Do not edit the notes of any published release.
4. **Verification** (proportionally, as in request 087): the full test suite; stored bytes and cross-open against the published `0.26.1` (the harness of request 090 § 6, same size, 0 failures); `EXPLAIN QUERY PLAN` of the six statements on the final tree equal to the plans of request 091 § 2; the re-prepare counts on the final tree (the harness of request 089, expecting 5,470 in total, all from the four `LIMIT ?` statements); paired timings of `set`, `get`, eviction, `list_entries`, `cache_stats` against `0.26.1` on a quiet host (say the load); fuzz and Miri are not needed for SQL text; all twelve gates; `gate-deny`; `gate-requirements`.
5. **Stop and report** if anything shows a changed result or plan on the final tree, or if the cast is not enough in the real statements (the counts decide).

Report as one request (stage 2). The version commit and the release candidate are a separate task after the owner's decision.
