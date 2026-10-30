# Q3e Handoff — The Removal List (RFC 024 B4, RFC 025 R9)

RFC: `rfcs/accepted/024-api-consistency.md` Part B, **B4**; `rfcs/accepted/025-error-taxonomy-and-input-validation.md` **R9**
("Execute RFC 024 B4 as written, including the two observable `ConnectionPool` strings").
Scope: authorized by the owner on 2026-10-29; everything removed here was deprecated in v0.21.5 with a
`#[deprecated]` note naming its replacement. Order: … Q2f → **Q3e** → Q10 → release preparation. Start when
Q2f is committed locally. **Local commits; do not push.**
Format of the review request: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6.
Authorship: high-capability model. Authorized: the owner.

## 0. What this is

Pure deletion plus two strings. **No behaviour of anything that stays changes.** Every removed item has a
documented replacement (the deprecation note, `docs/src/api.md`). If a deletion would break something not in
this file, or an item here cannot be removed cleanly, **stop and file a design request**. Principle: *less is
more*; a removal that leaves dead code, a stale doc sentence, or an `#[allow(deprecated)]` that no longer
guards anything is incomplete.

## 1. The list

| # | Remove | Replacement (already in the book) |
|---|---|---|
| 1 | the eight bool sort methods of `QueryBuilder`: `order_by_field`, `order_by_updated_at`, `order_by_last_accessed`, `order_by_path`, `then_by_field`, `then_by_updated_at`, `then_by_last_accessed`, `then_by_path` (`cache/query.rs`) | `order_by(key, order)` / `then_by(key, order)` with `SortKey` and `SortOrder` |
| 2 | `ConnectionPool` (the alias, `pool.rs:76`), `SharedEngine`, `shared_engine` (`pool.rs`), and their `pub use` in `lib.rs` | `SyncCacheEngine` (`open`, `with`, `with_mut`) |
| 3 | `CacheEngine::namespace_copy` (`cache/engine/portable.rs`) | `import_from` |
| 4 | `CacheWatcher::watched_count` (`cache/watcher.rs`) | `CacheWatcher::entry_count()` (returns a `Result`; `watched_count` returned the entry count, not watched paths, and reported errors as 0) |
| 5 | `CacheEngine::create_path_index`, `AsyncCacheEngine::create_path_index`, `QueryBuilder::index_hint` and its `index_hint` field and plumbing | none: the index duplicated the built-in unique index and could not speed up a query |
| 6 | the CLI `migrate` command: the `Commands::Migrate` variant, `MigrateArgs`, `cmd_migrate`, its dispatch arms and help text (`crates/cli/src/main.rs`, `commands/write.rs`) | `copy --from-db` (and `--upgrade-source`) |

**Stay, deprecated** (RFC 024 B4): `drop_path_index` and `list_path_indexes` (sync and async), until RFC 026
decides what schema v6 does with existing `lc_user_*` indexes. Users need a way to see and drop them. Do not
touch them except where a removed item's plumbing is shared.

**The two observable strings (R9):**
- `LocalFileCacheError::Poisoned { resource }`: the five sites in `pool.rs` that set `resource: "ConnectionPool"`
  (with the "Still `ConnectionPool`: the string is observable" comments) become `"SyncCacheEngine"`; update the
  rustdoc that quotes the value (`pool.rs` near lines 148, 171, 232) and the comments.
- The `ReadPool` `:memory:` reason already names `SyncCacheEngine` (done in Q3b); confirm with a grep.

## 2. What "clean" means for each removal

1. Delete the item, its rustdoc, its tests (a test of removed behaviour is deleted, **and listed** in the request
   with the reason), its entry in `crates/localcache/tests/api_surface.rs`'s `MANIFEST` if it has one (that test
   fails on a stale entry; keep it passing), and every doc/README/example mention.
2. Delete what only the removed item used: private helpers, fields, imports, the `index_hint` field and its
   threading through the query plan, the creation half of `db/indexes.rs` if nothing else calls it.
   **Keep** the identifier validation and the discovery/drop code that `drop_path_index`/`list_path_indexes`
   still use. Let `cargo clippy -D warnings` (`dead_code`) tell you what is left; do not add `#[allow(dead_code)]`.
3. Remove every `#[allow(deprecated)]` that guarded a removed item or a use of one. Report the remaining ones
   (there should be only those for `drop_path_index` and `list_path_indexes`).
4. Tests that need a legacy `lc_user_*` index (the drop/list tests) now create it with a raw SQL statement on a
   test-owned connection, since `create_path_index` is gone. Tests of the identifier grammar that went through
   `create_path_index` move to the functions that remain, or are deleted with a reason if they tested only the
   removed creation path. Report each.
5. `QueryBuilder` loses `index_hint`: check the plan/`EXPLAIN` code for a hint branch and remove it; the planner
   behaviour without a hint is unchanged (the existing plan tests pass unmodified).
6. The CLI: `localcache migrate …` becomes an unknown subcommand (clap's own error and exit status 2). Add a test
   that says so; keep `copy --from-db` tests. Remove the deprecation warning text and the `main.rs` doc comment.

## 3. Documentation and CHANGELOG

- `docs/src/api.md` (the migration table near lines 255–260 says "removed in 0.22.0"): make it past tense and
  keep it as the migration table, one row per removed item with its replacement. `docs/src/errors.md` (the
  `Poisoned` text near line 40), `docs/src/cli.md` (`migrate`), `docs/src/querying.md` and any page about index
  hints or path indexes, `docs/src/roadmap.md` is history (leave it). **Re-read each page in full** and fix
  anything else that is no longer true; say what.
- `README.md` and `crates/localcache/examples/`: no removed name may appear.
- Rustdoc: no intra-doc link to a removed item (`RUSTDOCFLAGS="-D warnings" cargo doc` is the check).
- `CHANGELOG.md`, `## [Unreleased]` / `### Removed` (create the heading if absent, in Keep-a-Changelog order),
  one entry per item with its replacement, and the `Poisoned` string change under `### Changed` as **Breaking**.
  Keep the other entries.
- **The gate:** `git grep -n "order_by_field\|order_by_updated_at\|order_by_last_accessed\|order_by_path\|then_by_field\|then_by_updated_at\|then_by_last_accessed\|then_by_path\|ConnectionPool\|SharedEngine\|shared_engine\|namespace_copy\|watched_count\|create_path_index\|index_hint" -- crates docs README.md`
  returns only: the migration table in `docs/src/api.md`, the CLI test for the removed `migrate`/`namespace_copy`
  if it names them, `docs/src/roadmap.md`, and any CHANGELOG-style history. Paste the output with its exit code
  and justify each line that remains. (`order_by_*` names such as `order_by` and `then_by` themselves stay.)

## 4. Tests

1. `Poisoned { resource }` is `"SyncCacheEngine"` for every site that produced `"ConnectionPool"` (find the
   existing poisoned-lock tests and update them; they should now fail on the old strings, so show the
   failing-before run).
2. `localcache migrate` is rejected as an unknown subcommand (exit 2); `copy --from-db` works (existing tests).
3. `drop_path_index` and `list_path_indexes` still work on a legacy index created by raw SQL, including an
   out-of-grammar legacy spelling that can be dropped.
4. `api_surface.rs` and the public-boundary test (`fixture_integrity/public_boundaries.rs`) pass; the latter
   contains an `allow(deprecated)`: report what it checks and whether it needs updating.
5. Everything else passes; compat fixtures unmodified; the doctests (a removed item in a doc example would fail them).

## 5. Gates and the request

Same set as Q3b's handoff § 6, plus the gate grep above. File `.git-exclude/review-request/NNN-dev-q3e-…-<date>.md`
describing the **uncommitted** tree; the diff is large and mostly deletions: give the line counts per removed
item (lines removed, tests removed). Commit locally after approval; no push, tag, version bump or `ROADMAP.md`
edit. List every required step and every judgement call.

## 6. Not yours

Any change to what stays; `drop_path_index`/`list_path_indexes` (schema v6 decides); new error variants; the
bounded-decode work (Q10); version, release preparation.
