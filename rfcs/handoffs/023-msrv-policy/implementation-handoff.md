# RFC 023 Implementation Handoff — MSRV Policy (Q1a, Q1b, Q1c)

RFC: `rfcs/accepted/023-msrv-policy.md` (accepted 2026-09-23; all three owner decisions as
recommended)
Milestones: Phase 24 **Q1a** (documentation, live on push), **Q1b** (fresh-resolution drift
check, ships in v0.21.5), **Q1c** (`rusqlite 0.40.2`, ships in v0.22.0)
QA companion: `rfcs/handoffs/023-msrv-policy/acceptance-qa-checklist.md`

## 0. What this is

RFC 023 is a policy, and it **changes no MSRV**. The declared MSRV stays `1.85` through every
slice here. If any step seems to need a higher toolchain, stop and file a design request. Do not
work around it.

Two standing principles from the owner govern every judgement call:

- **"Finally clean, safe and secure, robust and sophisticated design."**
- **Public material must not confuse or mislead users.** That covers the API, the book, and the
  CHANGELOG.

## 1. How to work through the slices

| Slice | RFC | Files (primary) | Ships |
|---|---|---|---|
| **Q1a** documentation correction | R10 | `docs/src/dependency_security.md`, `CHANGELOG.md` | Pages, when pushed after review. No crate release |
| **Q1b** fresh-resolution drift check | R6.2 | `scripts/release.py`, `scripts/tests/`, `scripts/release-tools.toml` (re-pin), `.github/workflows/msrv-fresh.yaml` (new), `Makefile.toml`, `docs/src/dependency_security.md`, `CHANGELOG.md` | v0.21.5 |
| **Q1c** `rusqlite 0.40.2` | R8, R9 | `Cargo.toml`, `Cargo.lock`, `docs/src/dependency_security.md`, `CHANGELOG.md` | v0.22.0 |

**Order.**
1. **Q1a now.** It corrects published material that is wrong today.
2. **Q1b next,** before the Q2 slices. It is independent of them, and the sooner it runs weekly,
   the sooner it can catch drift.
3. **Q1c only after v0.21.5 is released.** It is breaking. `main` carries no v0.22.0 change until
   every v0.21.5 slice has shipped, because non-breaking releases ship first (Phase 24 exit
   criterion 3). The architect opens Q1c explicitly.

Use the same cadence as RFC 022:
1. implement;
2. file the review request describing the **uncommitted** tree;
3. wait for the review in `.git-exclude/reviewed/`;
4. commit with the message the review gives;
5. push only when the review allows it.

The working tree holds exactly one slice when you file.

**CHANGELOG.** `## [Unreleased]` is at the top of `CHANGELOG.md` (the architect added it with this
handoff). Each slice writes its own entries under it, in the same tree, and shows the diff in its
request.

---

## 2. Q1a — Correct `docs/src/dependency_security.md` (RFC 023 R10)

### Why

The page is deployed from `main` to https://nabbisen.github.io/localcache-rs/ and is wrong on two
counts (RFC 023, "The `rusqlite 0.40` question, re-measured"):

1. It says `rusqlite 0.40` "would raise this crate's MSRV from 1.85 to exactly 1.95". That was
   true only while `libsqlite3-sys` was at 0.38.0 or 0.38.1. **0.38.2 (2026-08-08) polyfills
   `cfg_select!`**, and `rusqlite 0.40.2` requires `^0.38.2` on every non-wasm target.
2. It says `0.19.1` and `0.20.0` are broken on their declared 1.85. A **fresh** resolution of
   either now selects `libsqlite3-sys 0.38.2` and builds on 1.85. The architect verified this for
   `localcache =0.20.0`. They still fail from a **lockfile** that holds `libsqlite3-sys` 0.38.0 or
   0.38.1, and `cargo update -p libsqlite3-sys` repairs that.

### What to change

Re-read the whole page first. The list below is what the architect found. If you find more
that is no longer true, fix it and say so.

1. **Opening section** (the MSRV contract and the four rows): unchanged.
2. **"Why `rusqlite` is pinned below its newest line":**
   - **"The chain":** keep the two-line diagram. Replace the bisection table with one keyed by
     `libsqlite3-sys` version:
     - 0.38.0 and 0.38.1 need 1.95 (the standard-library `cfg_select!`);
     - 0.38.2 builds on 1.85.0 (local polyfill).

     State the date each fact was measured (2026-09-23).
   - Replace the "So moving to `rusqlite 0.40` would raise this crate's MSRV…" paragraph with the
     current position:
     - localcache 0.21.x keeps `^0.39`, for compatibility, not for the toolchain;
     - `rusqlite::Error` is part of the public `LocalFileCacheError::Database`, and `links`
       allows one SQLite line per graph, so changing the line is breaking;
     - **localcache 0.22.0 moves to `rusqlite 0.40.2`** (RFC 023 R9);
     - consumers who pin `rusqlite 0.40` directly can use localcache from 0.22.0.
   - **"Why this cannot be worked around downstream":** still true (`links`). Keep it. Its
     "tell us" advice stays.
   - **"The upstream cause":** keep the point that neither crate declares `rust-version`.
     Replace "If `libsqlite3-sys 0.38.x` declared `rust-version = "1.95"`, this whole conflict
     would disappear" with the lesson the 0.38.x series actually teaches: the floor moved **within
     a patch series**, up and then back down, invisibly to Cargo's MSRV-aware resolver. That is
     why RFC 023 adds a fresh-resolution check. Keep the "we have not filed an upstream issue"
     paragraph unchanged.
   - **"Recorded cases":** history. **Do not rewrite the two rows.** Add one sentence after the
     table: on 2026-08-08, `libsqlite3-sys 0.38.2` removed the 1.95 requirement, so the second
     case's request (`rusqlite >= 0.40`) is being met in 0.22.0, without an MSRV change.
   - **"Affected published versions":** rewrite in full. Say what was true when they were
     published: they did not build on 1.85, because they resolved `libsqlite3-sys` 0.38.0/0.38.1.
     Then say what is true now:
     - a fresh resolution builds on 1.85;
     - an existing lockfile holding 0.38.0/0.38.1 still fails, and the fix is
       `cargo update -p libsqlite3-sys`.

     Keep the advice to use `0.20.1` or later, but give the real reason now: later releases carry
     correctness fixes (point to `CHANGELOG.md`), not an MSRV repair. Keep the yank reasoning.
   - **"If this blocks you":** unchanged.
3. **New section "MSRV policy"**, placed directly after the opening section. Summarize RFC 023
   R2–R7 in user terms, in under 25 lines, and link the RFC on GitHub
   (`https://github.com/nabbisen/localcache-rs/blob/main/rfcs/accepted/023-msrv-policy.md`):
   - minor releases only;
   - a named necessity, and the lowest version that meets it;
   - the 12-month age floor, with no floor for security fixes;
   - notice one release ahead;
   - how it is verified;
   - the 6-month previous line after a raise.

   **Do not** mention the fresh-resolution check as already running. That is Q1b; Q1b adds its
   sentence.
4. **CHANGELOG** under `## [Unreleased]`, `### Changed`: one entry saying the MSRV and dependency
   page was corrected (the `rusqlite 0.40` toolchain requirement and the status of `0.19.1`/`0.20.0`)
   and gained the MSRV policy. Link nothing that 404s.

### Verification

- `mdbook build docs` into a scratch directory under `.git-exclude/tmp/`: builds, with no
  warning you introduced.
- Every factual claim you write about a crate version must be one you checked. Name the command,
  or cite the RFC 023 evidence table. The architect's evidence is under
  `.git-exclude/tmp/rfc023-evidence/` and `.git-exclude/tmp/rfc023-sqlite-floor/`.
- `git grep -n "1\.95" -- README.md docs/src`: every remaining hit is historical and dated, or
  names 0.38.0/0.38.1 specifically.
- No failing-before test applies to a documentation slice. Quote the old and new wording of each
  corrected claim instead.

---

## 3. Q1b — Fresh-resolution drift check (RFC 023 R6.2)

### What it is

A check that does what a new consumer does:
- resolve dependencies with no lockfile, using the **declared-MSRV** Cargo, so that
  `resolver = "3"`'s MSRV-aware fallback applies;
- then build the declared-MSRV rows.

It catches a dependency that declares no `rust-version` raising the effective floor. That is the
failure `0.19.1` and `0.20.0` shipped with.

### Interface

Add `--fresh` to the `msrv` context of `scripts/release.py`:

```text
python3 scripts/release.py msrv --fresh --output-dir <dir>
```

- **`msrv`** (no flag): unchanged. The locked rows, as today. CI's push-time `msrv` job keeps
  calling exactly this.
- **`msrv --fresh`:** the fresh check **only**. The scheduled workflow calls it.
- **`release`:** runs the locked `msrv` step **and then** the fresh check, both in the `msrv`
  context of the bundle. A drift failure fails the release (R6.2).

### Mechanism

`msrv_mode` is in `scripts/release.py`, around line 820.

1. Verify the declared toolchain exactly as the locked path does (`verify_declared_toolchain`).
   The fresh check is meaningless under any other Cargo.
2. Record the SHA-256 of the repository's `Cargo.lock`.
3. Copy the **tracked** tree (`git ls-files`) into a temporary directory under the output
   boundary. Leave out `Cargo.lock`, and never copy `target/`.
4. In the copy: `cargo generate-lockfile`.
5. In the copy: run the existing declared-MSRV row generator. **Reuse `scripts/feature_matrix.py
   --run-msrv` as it is.** Its `--locked` is correct here, because the lockfile was just
   generated and must not change during the rows. Never write a second copy of the row list.
   That is the "one rule" the project applies to its designs.
6. In the copy: `cargo metadata --format-version 1 --locked`. List every **crates.io** package
   whose `rust_version` is `null`, with its version. This names the likely culprit when the
   check fails.
7. Re-hash the repository's `Cargo.lock`. **If it changed, fail.** The check must never touch
   the repository's lockfile.
8. Evidence goes in the output directory:
   - the generated lockfile, as `fresh-Cargo.lock`;
   - the undeclared-package list;
   - the rows' logs;
   - `summary.log` lines: `fresh-resolution: PASS`, `declared-msrv-matrix-fresh: PASS|FAIL`,
     `undeclared-rust-version: <n> packages`;
   - a `fresh` object in `manifest.json` with the resolved versions of `rusqlite` and
     `libsqlite3-sys`.

   Follow the existing `GateLog` / `append_summary` / `write_manifest` conventions.
9. Clean up the temporary copy on success and on failure, but keep the evidence.

`scripts/release.py` is hash-pinned: re-pin `[implementations.release-runner]` in
`scripts/release-tools.toml`, and show both hashes.

### CI

Add a new workflow, **`.github/workflows/msrv-fresh.yaml`**. Do not add a `schedule:` to
`ci.yaml`: that would run the whole CI weekly, and a push-time failure caused by an upstream
publication must not block unrelated work.

- `on: schedule` weekly (`cron: "17 3 * * 1"`, Monday 03:17 UTC), plus `workflow_dispatch`.
- `permissions: contents: read`, with no secrets, as in `ci.yaml`'s header rule (RFC 009 R16).
- `runs-on: ubuntu-24.04`. Pin every action by full commit SHA **with the same pins `ci.yaml`
  uses today**, including `upload-artifact` v7.0.1. Install the declared toolchain the way
  `ci.yaml`'s `msrv` job does: reuse its step shape, don't reinvent it.
- Run `python3 scripts/release.py msrv --fresh --output-dir "$RUNNER_TEMP/msrv-fresh-evidence"`,
  and upload the evidence whether it passes or fails.
- It is **not** part of `aggregate-ci`. Don't touch that job's required set.

Add `[tasks.msrv-fresh]` to `Makefile.toml`, mirroring `[tasks.msrv-check]`.

### Tests (`scripts/tests/`)

Unit tests in `scripts/tests/test_release_runner.py`, with the command runner mocked in the style
that file already uses:
- the tracked copy leaves out `Cargo.lock` and `target/`;
- the undeclared-package parser, on a `cargo metadata` fixture with a mix of declared, undeclared,
  path, and git packages (only crates.io packages are listed);
- the lockfile guard fails when the hash changes;
- `release` runs the fresh step after the locked step, and fails when it fails;
- plain `msrv` runs no fresh step.

Run the suite normally **and under the restricted `PATH`**, as for every `scripts/` change.

### Demonstrations (in the review request, not the unit suite)

Both need the network and the 1.85 toolchain:

1. **Positive:** `rustup run 1.85.0 python3 scripts/release.py msrv --fresh --output-dir
   .git-exclude/tmp/q1b-fresh-pass` on the tree. PASS. Quote `summary.log` and the undeclared
   list.
2. **Negative (the failing-before):** a throwaway copy of the tree whose `Cargo.toml` adds
   `libsqlite3-sys = { version = "=0.38.1", features = ["bundled"] }` to `localcache`'s
   dependencies. It must FAIL at `cfg_select!`, and its evidence must list `libsqlite3-sys`
   among the undeclared packages.

   The pin must be on `libsqlite3-sys` itself. `rusqlite =0.40.1` alone would resolve 0.38.2 and
   pass (RFC 023 test plan).

   Delete the copy afterwards. Its output directory stays under `.git-exclude/tmp/` as evidence.
3. **The lockfile guard:** show the repository `Cargo.lock` hash before and after both runs.
   Identical.

After the push, the architect triggers the workflow once with `workflow_dispatch` and records the
run.

### Documentation and CHANGELOG

- `docs/src/dependency_security.md`, "MSRV policy" section: one sentence saying the
  fresh-resolution check runs weekly and at every release.
- CHANGELOG `## [Unreleased]`, `### Added`: the check, in one entry, named as release tooling.

---

## 4. Q1c — `rusqlite 0.40.2` (RFC 023 R8, R9), in v0.22.0

**Do not start until the architect opens it after v0.21.5 ships.** It is written down now so the
v0.21.5 notice and the slice agree.

### Change

1. `Cargo.toml` (`[workspace.dependencies]`):
   `rusqlite = { version = "0.40.2", features = ["bundled", "limits"] }`.

   The requirement is **exactly `"0.40.2"`**, never `"0.40"`. `"0.40"` would leave a consumer's
   lockfile holding `rusqlite 0.40.1` / `libsqlite3-sys 0.38.1` valid, and that needs 1.95
   (RFC 023 R8.2).
2. `cargo update -p rusqlite`. The `Cargo.lock` diff must be `rusqlite`, `libsqlite3-sys`, and
   only the transitive packages those two require. List every changed package.
3. Same file, a comment-only correction: the `aes-gcm` line's comment says "0.11.x still RC".
   That has been false since `aes-gcm 0.11.1` (2026-08-21). Make the comment say why the
   requirement is `0.10.3` (the advisory floor), and nothing about 0.11. **Do not upgrade
   `aes-gcm`.** That is a separate decision in the Phase 24 register.
4. **Read the upstream release notes** for `rusqlite` 0.40.0–0.40.2 and SQLite 3.52.0–3.53.2.
   Report anything that touches what localcache uses:
   - JSON functions (the query engine's tiers 1 and 2);
   - `PRAGMA` behaviour (journal mode, `synchronous`, `user_version`);
   - statement or transaction semantics;
   - `limits`;
   - error codes that `LocalFileCacheError::Database` surfaces.

   "Nothing relevant" is an acceptable answer if you say what you read.
5. `docs/src/dependency_security.md`: the "Why `rusqlite` is pinned…" section, as Q1a left it,
   becomes past tense. 0.22.0 uses 0.40.2, and the section explains why the requirement is the
   exact patch.
6. CHANGELOG `## [Unreleased]`, `### Changed`, marked **breaking**:
   - `rusqlite` 0.39 → 0.40.2 (bundled SQLite 3.51.3 → 3.53.2);
   - `LocalFileCacheError::Database` now wraps `rusqlite` 0.40's `Error`;
   - because of `links = "sqlite3"`, a crate that depends on `rusqlite` directly must use
     0.40.2 or later alongside localcache 0.22.

### Verification

- The four locked rows on **1.85.0** (`cargo make msrv-check`, fresh `--output-dir`).
- `msrv --fresh` on 1.85.0 (Q1b): PASS.
- `cargo test --workspace --all-features --locked`: report the count. The compatibility fixtures
  (`crates/localcache/tests/compat.rs`) must pass **unmodified**: the payload wire format and the
  schema cannot change with the SQLite library.
- `cargo make matrix`, `cargo fmt --all --check`, clippy `-D warnings`.
- `rusqlite::version()` printed on the new build: `3.53.2`.
- No failing-before test applies to a dependency change. The compatibility fixtures passing
  unmodified are the regression evidence.

---

## 5. Not yours — do not do these

- **No MSRV change**, in any slice.
- No dependency change other than Q1c's `rusqlite` line and its lockfile consequences. In
  particular, no `aes-gcm`, `criterion`, or `clap` upgrade.
- No change to `LocalFileCacheError`. Whether `rusqlite::Error` stays public is Q3's decision
  (RFC 025; RFC 023 R8.3).
- No change to the push-time `msrv` job, the four rows, or `aggregate-ci`.
- No release action. The v0.21.5 notice of Q1c is written by the architect's v0.21.5
  release-preparation handoff, not by Q1c.
- **Commit and push only your own work, and only after its review approves it** (owner rule,
  2026-09-23).

## 6. Gates, for every slice

- `cargo fmt --all --check` clean.
- `python3 scripts/source_integrity.py --require-tracked` OK.
- Q1b, Q1c: `cargo make matrix` green on every row, with clippy `-D warnings`.
- Q1b, Q1c: `cargo make msrv-check` under 1.85, in a fresh `--output-dir`. Report every attempt.
- Q1b: the script tests, normally and under the restricted `PATH`.
- Q1c: the full suite. **Report the count you observe.**
- Q1a, Q1b: `mdbook build docs` into `.git-exclude/tmp/`.
- `git status --porcelain` shows only the slice's files. Scratch belongs in `.git-exclude/tmp/`.

## 7. Review request, for every slice

File it as `.git-exclude/review-request/NNN-dev-q1X-<topic>-<YYYY-MM-DD>.md`. `NNN` is the next
free number in that folder. Name this handoff, the slice, and the RFC requirement at the top.

The contents follow the organization workflow § 9.2:
1. implementation summary;
2. RFC 023 requirements addressed;
3. changed files;
4. important decisions;
5. differences from this handoff;
6. tests added and run;
7. evidence: Q1a's old and new wording; Q1b's three demonstrations; Q1c's lockfile diff, the
   release-note reading, and the SQLite version; plus each slice's CHANGELOG diff;
8. gate results, with the commands as run;
9. unresolved issues;
10. known limitations;
11. requested review focus.

**List every required step, including the ones that pass** (review 014 N1). Report every
judgement call. Do not absorb it silently.
