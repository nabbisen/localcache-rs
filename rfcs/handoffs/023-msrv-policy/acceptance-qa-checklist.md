# RFC 023 Acceptance & QA Checklist — Q1a, Q1b, Q1c

Companion to `rfcs/handoffs/023-msrv-policy/implementation-handoff.md`. This is what each slice's
review checks. A slice is accepted only when every box in its own section and in **D** holds.

## A. Q1a — Documentation correction (R10)

- [ ] "The chain" is keyed by `libsqlite3-sys` version: 0.38.0/0.38.1 need 1.95; 0.38.2 builds on 1.85.0. The measurement date is stated
- [ ] No sentence still says that `rusqlite 0.40` requires Rust 1.95 without naming 0.38.0/0.38.1
- [ ] The current position is stated: 0.21.x keeps `^0.39` for compatibility (public `rusqlite::Error`, `links`), and **0.22.0 moves to `rusqlite 0.40.2`**
- [ ] "Recorded cases" rows are unchanged. One follow-up sentence records 0.38.2 and the 0.22.0 outcome
- [ ] "Affected published versions" separates what was true at publication from what is true now (fresh resolution builds; a lockfile holding 0.38.0/0.38.1 fails; `cargo update -p libsqlite3-sys`)
- [ ] The advice to use `0.20.1`+ stays, with the correctness-fixes reason
- [ ] "MSRV policy" section, under 25 lines: minor only; necessity and lowest version; 12-month floor with no floor for security; notice one release ahead; verification; 6-month previous line; link to RFC 023
- [ ] It does **not** describe the fresh-resolution check as running (that is Q1b)
- [ ] Every version claim cites a command or RFC 023's evidence table
- [ ] The CHANGELOG entry is under `## [Unreleased]` → `### Changed`
- [ ] `mdbook build docs` succeeds

## B. Q1b — Fresh-resolution drift check (R6.2)

- [ ] Plain `msrv` behaves exactly as before. The push-time CI job is unchanged
- [ ] `msrv --fresh` verifies the declared toolchain first, and refuses any other toolchain
- [ ] The copy is of **tracked** files only, with no `Cargo.lock` and no `target/`
- [ ] The rows come from `scripts/feature_matrix.py --run-msrv`. There is no second row list
- [ ] Undeclared crates.io packages are listed with versions. Path and git packages are excluded
- [ ] The repository `Cargo.lock` hash is identical before and after, and a changed hash fails the check
- [ ] The evidence (`fresh-Cargo.lock`, the undeclared list, row logs, `summary.log`, the `manifest.json` `fresh` object) follows the existing conventions
- [ ] `release` runs locked `msrv` then `--fresh`, and fails on drift
- [ ] `scripts/release-tools.toml` is re-pinned, with both hashes shown
- [ ] `.github/workflows/msrv-fresh.yaml`: weekly schedule plus `workflow_dispatch`; `contents: read`; no secrets; `ubuntu-24.04`; the same SHA pins as `ci.yaml`; evidence uploaded on pass and on fail; not in `aggregate-ci`
- [ ] `Makefile.toml` `[tasks.msrv-fresh]`
- [ ] Unit tests for the copy, the parser, the guard, `release` ordering and failure, and plain `msrv` running no fresh step. They pass normally and under the restricted `PATH`
- [ ] Demonstration 1 (positive, on 1.85.0): PASS, with the summary and undeclared list quoted
- [ ] Demonstration 2 (negative, `libsqlite3-sys = "=0.38.1"` pinned): FAIL at `cfg_select!`, and `libsqlite3-sys` is listed as undeclared
- [ ] Demonstration 3: lockfile hash identical across both runs
- [ ] The docs sentence and the CHANGELOG `### Added` entry are present

## C. Q1c — `rusqlite 0.40.2` (R8, R9)

- [ ] Opened by the architect **after v0.21.5 was released**
- [ ] The requirement is exactly `"0.40.2"`, with `bundled` and `limits` unchanged
- [ ] The `Cargo.lock` diff is limited to `rusqlite`, `libsqlite3-sys`, and their own transitive requirements, each listed
- [ ] The `aes-gcm` comment is corrected, and the requirement is unchanged
- [ ] The upstream release-note reading is reported (`rusqlite` 0.40.0–0.40.2, SQLite 3.52.0–3.53.2), covering JSON, `PRAGMA`s, transactions, `limits`, and error codes
- [ ] The four locked rows pass on 1.85.0, and so does `msrv --fresh`
- [ ] The full suite passes, and its count is reported. `crates/localcache/tests/compat.rs` passes **unmodified**
- [ ] `rusqlite::version()` reports `3.53.2`
- [ ] The CHANGELOG `### Changed` entry is marked breaking, and names the SQLite versions, the `LocalFileCacheError::Database` type change, and the `links` consequence for direct `rusqlite` users
- [ ] `docs/src/dependency_security.md` is updated to past tense for 0.22.0

## D. Gates and reporting (every slice)

- [ ] `cargo fmt --all --check` clean
- [ ] `python3 scripts/source_integrity.py --require-tracked` OK
- [ ] Q1b, Q1c: `cargo make matrix` green (clippy `-D warnings`); `cargo make msrv-check` green under 1.85, with every attempt reported
- [ ] Q1a, Q1b: `mdbook build docs` succeeds
- [ ] **No MSRV change in any slice**
- [ ] The review request is `.git-exclude/review-request/NNN-dev-q1X-<topic>-<date>.md`, and names this handoff and the slice
- [ ] Every required step is listed, including the ones that pass
- [ ] Nothing is committed before the review lands
- [ ] Judgement calls and differences are **reported, not absorbed**

## What will not count against you

- Finding that this handoff is wrong about a line number, a function, a pin, or a page section,
  and saying so.
- Finding a further stale claim in `docs/src/dependency_security.md` and fixing it, with the
  evidence.
- Stopping at a design question instead of guessing.
- Upstream release notes that turn out to matter for Q1c. Report them before you adapt anything.
