# Q2b Handoff — Module-Size Split (a pure move), v0.21.5

Milestone: Phase 24 **Q2b** (ROADMAP milestones table). It is listed in RFC 024's slice table, and
it runs after Q2e because it touches the files Q2c–Q2d changed.
Precedent: Phase 22 N5 (`rfcs/handoffs/011-safe-sqlite-identifier-boundary/n5-module-size-and-identifier-hygiene.md`;
ROADMAP "N5 completion" and "Module-size register — after N5").
QA: § 6 of this file.

## 0. What this is

**A pure move. Nothing in the program changes.**
- No behaviour, no signature, no name, no doc text, no test assertion.
- **A move never carries a fix.** If you find a defect while moving code, **report it and leave
  it**, exactly as N5 did with `namespace_copy`.

Measured by the architect on 2026-09-24 as production ELOC (non-blank, non-comment lines; test
code is measured separately, the N5 lesson):

| File | Now | After the split |
|---|---|---|
| `crates/localcache/src/cache/query.rs` | **809** (719 at 0.21.3; grown by RFC 024) | about 343 in `query.rs` and 466 in `query/execution.rs` |
| `crates/localcache/src/db/repository.rs` | **871** (618 at N5's reasoned refusal) | about 631 in `repository.rs` and 222 in `repository/candidates.rs` |

## 1. Split A — `crates/localcache/src/cache/query.rs` at the builder/execution seam

**Stays** in `query.rs`, which is lines 1–695 today:
- the public types: `QueryReport`, `SkippedEntry`, `SortOrder`, `SortKey`;
- `OrderBy` and `Predicate`, with their impls, and `get_field`;
- `QueryBuilder` and its `impl`;
- `decode_with` and the test-only decode counter;
- the `mod tests` declaration.

**Moves** to a new `crates/localcache/src/cache/query/execution.rs`: everything from
`struct PreparedPathFilters` (line 696 today) to the end of the file. That is:
- `PreparedPathFilters` and its impl;
- `execute_query`, both `execute_report` definitions, `execute_tier3`, `materialize`;
- `QueryPlan`, `is_safe_json_field_path`, `classify_query`, both `describe_query_plan` definitions;
- the comparison helpers (`cmp_candidate_basic`, `cmp_candidate_json`, `cmp_key_json`, `ord_dir`,
  `json_sort_key`).

The block moves **as one contiguous piece, in its current order.**

**Keep every existing path valid.** The wrappers and the engine call
`crate::cache::query::execute_query` and `…::execute_report`. Add `mod execution;` to `query.rs`,
plus `pub(crate) use execution::{…}` for exactly the items used outside the module, so no caller
changes. Whatever `QueryBuilder`'s methods call in the moved code (for example
`prepare_path_filters`, `describe_query_plan`) becomes `pub(super)`: the narrowest visibility that
crosses the new boundary.

## 2. Split B — `crates/localcache/src/db/repository.rs`: the RFC 021 candidate queries

N5 refused to split `repository.rs` because its free functions share SQL-construction helpers.
Since then, RFC 021 added one section with **a single consumer, query execution**. That is a real
seam.

**Moves** to a new `crates/localcache/src/db/repository/candidates.rs`: the contiguous section under
`// RFC 021: one-pass, late-materialization candidate queries` (line 833 today), in its current
order, up to but **not** including `fn escape_like`. That covers `query_candidates`,
`JsonPushdownRow`, `query_candidates_json_pushdown`, `payloads_for_ids`,
`query_candidates_with_payloads`, and `namespace_all_json`.

**Stays** in `repository.rs`:
- the row types, including `CandidateRow` and `FullCandidateRow`, in the "Row types" section;
- `ID_LIST_CHUNK`, which the delete path also uses;
- the path-listing SQL builder (`build_path_sql`, `path_filter_clauses`) and `escape_like`, which
  `keys` and `explain_query` share. This is N5's coupling, and it stays where it is;
- `list_namespaces`, and everything else.

`candidates.rs` reaches the shared items through `super::`, with `pub(super)` added where needed.
Add `mod candidates;` and `pub(crate) use candidates::{…}` to `repository.rs`, so that every
`repository::query_candidates`-style path still resolves and `query/execution.rs` needs no change
for Split B.

**If you find the seam is not clean**, for example a moved function calls something private that
would have to widen well beyond `pub(super)`, stop and report it. A reasoned refusal is an
acceptable outcome, as it was at N5. A forced split is not.

## 3. Allowed changes, and nothing else

1. `mod` declarations, and the `//!` doc header of each new file (one short paragraph saying what
   it holds).
2. `use` lines, including `#[cfg(feature = "json")]` on a `use` whose items are feature-gated.
   **Watch the N5 trap:** an unconditional `use` of a gated item compiles under `--all-features`
   and fails without them. Every matrix row catches it.
3. Visibility widening to `pub(super)`, or `pub(crate)` where an existing path requires it, only
   where an item must cross the new boundary.
4. `pub(crate) use` re-exports that keep existing paths valid.
5. In `crates/localcache/src/cache/query/tests.rs`: `use` lines only, if the tests reach moved
   private items. No test body changes.

rustfmt may re-wrap moved code. That is why the check below is token-level, not line-level.

## 4. Verification — at token level, as N5 did

Write a small script under `.git-exclude/tmp/`. Do not commit it. For each split:
1. Tokenize the **old** file (from `HEAD`) and the **new** files. The tokenization is
   whitespace-insensitive. **Comments count as tokens**, because doc comments are API documentation
   and must move unchanged.
2. Reconstruct the "new" sequence: the new parent file's tokens, with the child file's tokens
   (minus its `//!` header) substituted at the position where the moved block used to begin.
3. `difflib` the old sequence against the reconstructed one.
4. **Every remaining difference must fall into a § 3 category.** List each one in the request,
   with its category.

Show the script's full output. **Line-based diffs are not evidence here.** N5 found them
misleading after rustfmt rejoined a signature.

## 5. Gates

- **The test count is exactly unchanged** from the Q2e commit, under
  `cargo test --workspace --all-features --locked`. Report both counts.
- `python3 scripts/feature_matrix.py --run-all` green on **every** row, with clippy `-D warnings`.
  This is what catches a mis-gated `use`.
- `cargo make msrv-check` under 1.85, in a fresh `--output-dir`.
- `RUSTDOCFLAGS="-D warnings" cargo doc -p localcache --no-deps --all-features --locked`.
- `cargo fmt --all --check`, and `python3 scripts/source_integrity.py --require-tracked` (use
  `git add -N` for the two new files).
- **No CHANGELOG entry.** A pure move changes nothing a user can observe. The architect records
  the new sizes in ROADMAP's module-size register.

File the request as `.git-exclude/review-request/NNN-dev-q2b-module-split-<date>.md`. Commit
locally after review, and **do not push** (review 019 § 4). Use two commits, one per split, so each
is a reviewable pure move. The review gives the messages.

## 6. QA checklist

- [ ] Split A: `query/execution.rs` holds exactly the § 1 block, contiguous and in order; `query.rs` keeps the rest
- [ ] Split B: `repository/candidates.rs` holds exactly the § 2 section, contiguous and in order; the row types, `ID_LIST_CHUNK`, the path-SQL builder, `escape_like`, and `list_namespaces` stay
- [ ] Every existing `crate::cache::query::…` and `crate::db::repository::…` path still resolves through re-exports. No caller outside the two modules changed, except `use` lines if unavoidable, which must be reported
- [ ] Visibility widened only to `pub(super)`, or to `pub(crate)` where required, each listed
- [ ] The token-level comparison output is attached, with each remaining difference categorized under § 3
- [ ] `query/tests.rs` changed in `use` lines only, or not at all
- [ ] Any defect found is reported and left unfixed
- [ ] The test count is identical before and after, with both reported
- [ ] Every matrix row is green, including the no-default-features and single-feature rows
- [ ] MSRV, rustdoc, fmt, and source integrity pass
- [ ] Production ELOC after the split is reported per file, measured as in § 0
- [ ] No CHANGELOG entry, no push, and two local commits after review
