# Q0f Handoff — Release Preparation for v0.21.4

Milestone: Phase 24 **Q0f**
Ships: RFC 022 (`rfcs/accepted/022-correctness-and-contract-reconciliation.md`), meaning every
slice Q0a–Q0e and Q0g–Q0j
Precedent: `rfcs/handoffs/021-query-execution-one-pass-late-materialization/p2d-release-preparation.md`
(v0.21.3). **Do not carry line numbers or values over from it.** Every reference below was
re-verified against the tree on 2026-09-23.

Two parts, then the release-candidate run:
- **A** — coming-version housekeeping;
- **B** — a paired re-measurement of LRU eviction, the one published figure whose code changed.

**Start only after Q0j's CI run is confirmed green.** Q0j changes the runner every job uses, and
the release candidate must be built on the CI configuration it will be verified on.

**No release action here.** Tagging and publishing remain the owner's (RFC 009 R15).

---

# Part A — Coming-version housekeeping (0.21.3 → 0.21.4)

## A.1 — Workspace version

`Cargo.toml:16`: `version = "0.21.3"` → `"0.21.4"`. Both members inherit it through
`version.workspace = true`. Do not add a version to either member manifest.

## A.2 — Install examples

Since Q0d, the version gate (`scripts/release.py`, `verify_version_references`) checks **every**
declaration line in `README.md` and `docs/src/**/*.md`. Today these eight lines exist, and each
must name `0.21.4`:

| File | Line |
|---|---|
| `README.md` | 53 |
| `docs/src/getting_started.md` | 9 |
| `docs/src/introduction.md` | 7 |
| `docs/src/features.md` | 8 |
| `docs/src/async.md` | 154, 157 |
| `docs/src/cookbook.md` | 223, 248 |

Re-run `git grep -n '^[[:space:]]*localcache[[:space:]]*=' -- README.md docs/src` before editing.
If it finds a line not listed here, update it too, and say so. The gate is the authority, not this
table.

## A.3 — CHANGELOG

1. `## [Unreleased]` becomes `## [0.21.4]`, with the RC-placeholder line saying the date is set at
   owner authorization. Replace the preamble with a short release summary in the house style
   (see the `## [0.21.3]` section):
   - **A correctness patch. Not breaking.** No public signature, schema, wire-format, dependency,
     or MSRV change. **No input v0.21.3 accepted now returns an error.**
   - Six correctness fixes (RFC 022 R1, R2, R6, R7, R8, R9), one sentence each, pointing at the
     `### Fixed` entries below.
   - The announcement, already in `### Changed`: `max_entries(0)`, a `batch_set` larger than
     `max_entries`, and a TTL under one second **will be rejected with an error from v0.22.0**.
     Repeat it in the summary, since it is the one forward-looking change a user must plan for.
2. The existing `### Fixed` and `### Changed` entries stay as the slices wrote them. Do not rewrite
   them.
3. Links: add `[0.21.4]: https://github.com/nabbisen/localcache-rs/compare/0.21.3...0.21.4`, and
   change `[Unreleased]` to `…/compare/0.21.4...HEAD`.

## A.4 — The advisory gate's User-Agent

`scripts/check_advisories.py:555`: `"localcache-rfc014-security-gate/0.21.3"` → `0.21.4`. No gate
catches this string. **Changing it changes the file's SHA-256**, so re-pin
`[implementations.check-advisories]` in `scripts/release-tools.toml` in the same change. Show the
old and new hashes.

## A.5 — `Cargo.lock`

It regenerates from the bump. Confirm that the diff is **only** the two workspace members'
`version` lines.

## A.6 — Must not change

Every other `0.21.3` is history and stays:
- the `## [0.21.3]` CHANGELOG section and its compare link;
- `ROADMAP.md`;
- `docs/src/roadmap.md`, `docs/src/performance.md`, and `docs/src/dependency_security.md`
  wherever they name a past release;
- everything under `rfcs/`, including the `p2d` precedent.

A blanket substitution would still pass the version gate and would rewrite history into
falsehoods. List every remaining `0.21.3` occurrence in the review request and say why each
stays.

---

# Part B — Paired re-measurement of LRU eviction

## B.1 — Why only this row

Q0c rewrote eviction:
- one ordered `SELECT` over-fetching by the protected-id count;
- a filter in Rust;
- `DELETE … WHERE id IN (…)` in 500-id chunks;
- all of it inside the write's `IMMEDIATE` transaction.

It replaced a single `DELETE … WHERE id IN (SELECT … LIMIT n)`. `docs/src/performance.md` publishes
**"LRU eviction, per evicted entry: 5.17 / 4.08 / 4.15 µs"**, measured on the old code. Phase 24's
exit criterion 6 requires the docs to match the code at each release. No other published figure's
code path changed in a way the profile measures:
- `get`, `get_if_fresh`, and the queries at `offset = 0` are unchanged;
- `batch_set` now uses an `IMMEDIATE` instead of a deferred transaction, and serves as a
  **control**.

## B.2 — Method: the P1d paired comparison

This is the discipline P1a established and P1d applied correctly first time.
1. **Two builds, one session:**
   - a detached worktree at tag **`0.21.3`** (before);
   - the Part A tree (after).
2. **The same harness in both:** `crates/localcache/benches/scale_profile.rs` has not changed
   since `0.21.3`. Confirm that with `git diff 0.21.3 -- crates/localcache/benches/scale_profile.rs`
   and report the empty diff.
3. **Scale 1M**, `LOCALCACHE_SCALE=1000000`, on real storage, with `TMPDIR` under
   `.git-exclude/tmp/`.
4. **Matched stored-path length across both arms.** Before reading any timing, check that the
   harness's own `TMPDIR path length` and `example stored path length` lines are identical in
   both arms.
5. **Interleave:** before, after, before, after. Eviction is destructive, so it gives one sample
   per run (limitation 5). Two runs per arm give a spread.
6. **Controls:** `get`, `get_if_fresh`, and `batch_set` per entry, from the same runs. If a
   control moves beyond its own before-arm spread, the session is not clean. Stop and report.

## B.3 — What to change

- **If the after/before eviction ratio is within the before arm's own spread:** leave the
  published row as it is, and record the ratio and both spreads in the review request.
- **If it is outside the spread in either direction:** replace the whole row's figures with a
  single fresh profile run at the page's stated path length, as P2d did for its table. Do not
  splice. Add one sentence to the page explaining the change: eviction now runs inside the write's
  transaction and never evicts the rows being written.

If the eviction row regresses by more than 1.5×, **stop and report before releasing.** A patch
release does not ship a performance regression unannounced. The architect will decide.

---

# Required evidence (review request)

File as `.git-exclude/review-request/NNN-dev-q0f-release-preparation-<date>.md`.
- Version bump in `Cargo.toml` and `Cargo.lock` (two lines), and all eight install examples.
- The version gate passes on the working tree:
  `python3 -c 'import sys; sys.path.insert(0,"scripts"); import release; from pathlib import Path; release.verify_version_references(Path("."), "0.21.4"); print("PASS")'`.
  The full `release.py source` gate needs a **clean, committed** tree, so it runs after the
  commit, in step 2 of "What comes next", not here.
- The User-Agent change, and its re-pin with both hashes.
- CHANGELOG `## [0.21.4]` present, summary written, undated, and both links correct.
- The A.6 list of remaining `0.21.3` occurrences, each with its reason.
- Part B:
  - the empty harness diff;
  - both arms' path-length lines;
  - a table of all four runs (eviction, `get`, `get_if_fresh`, `batch_set`);
  - the ratio, and the decision under B.3.

  Put the raw logs under `.git-exclude/tmp/` and cite them. Do not paste them.
- Gates:
  - `cargo fmt --all --check`;
  - `python3 scripts/feature_matrix.py --run-all`;
  - `rustup run 1.85.0 python3 scripts/release.py msrv --output-dir .git-exclude/msrv-evidence-q0f`
    (a fresh directory, as in every Q0 slice);
  - `python3 scripts/source_integrity.py --require-tracked`;
  - the script tests, normally and under the restricted `PATH`;
  - `cargo test --workspace --all-features --locked`, reporting the count you observe.

# What comes next, in this order

1. This work, reviewed by the architect.
2. **Commit, then run `python3 scripts/release.py source --output-dir .git-exclude/tmp/q0f-source`
   on the clean committed tree** (it must exit 0 with `version-contract: PASS (0.21.4)`). **Then
   push, and confirm CI green on the exact tip.** You may push once the review approves. The
   architect confirms the run.
3. **RC production run** on that pushed, CI-verified commit. It follows
   `rfcs/handoffs/009-reproducible-source-archives-and-release-gates/m6-implementation-handoff.md`
   § "M6e — RC production run" and its § "Bundle retention", as the v0.21.3 cycle did:
   - `python3 scripts/release.py release --output-dir .git-exclude/release-candidate-v0.21.4`;
   - the extracted-archive result reported separately;
   - the three negative `rc_eligible` demonstrations;
   - the secret scan;
   - retention applied, and the superseded `release-candidate-v0.21.3/` bundle removed.

   File it as its own review request.
4. **Release decision:** the architect's recommendation to the owner.
5. **Owner:** authorize, and the architect sets the CHANGELOG date. Then the owner tags `0.21.4`
   (unprefixed, GPG-signed) on that commit, pushes the tag, and runs
   `cargo publish --workspace --locked`.
6. **Architect:**
   - RFC 022 → `rfcs/done/` with `Status: Implemented (0.21.4)`;
   - `rfcs/README.md`;
   - `ROADMAP.md`;
   - the post-publication fresh-consumer MSRV check (`rust-version = "1.85"` resolves and builds).
