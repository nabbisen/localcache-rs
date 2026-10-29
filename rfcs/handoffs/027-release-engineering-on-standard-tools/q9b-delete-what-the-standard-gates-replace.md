# Q9b Handoff — Delete What the Standard Gates Replace (RFC 027 R2–R4, R6.2)

RFC: `rfcs/accepted/027-release-engineering-on-standard-tools.md` (accepted by the owner, 2026-10-29),
including the Q9a findings recorded under R2.
Slice: Phase 24 **Q9b**, the second of three. Order for the dev team: Q9a (done, CI green on `4c016b6`) →
**Q9b-1 → Q9b-2** → Q9c → Q3d …
Review-request format: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6.
Authorship: high-capability model. Authorized: the owner.

## 0. What this is

Q9a proved, on the same trees, that the standard gates agree with the old ones. Q9b **removes the old
machinery** and leaves one way to do each thing. The owner's rule: *keep it simple, standard-oriented, clean
and easy to understand; don't make it complicated or messy.* Every deletion is justified by the standard
command that now does the job, shown failing on a deliberately broken input. **Nothing a gate checks is
dropped without being named in the request** (the three items the architect has already dropped are in § 4).

Two review requests, **one per part**; each part is pushed after its review approves it (workflow changes
are verified only by CI). Within a part, **one commit per tool** (RFC 027 R6.2), in the order given, each
commit leaving CI green. Do not start Part 2 before the architect has confirmed CI on Part 1.

If a step seems to need a design decision not made here, **stop and file a design request**.

## 1. Part 1 — the three gates already proven in Q9a

### 1.1 Commit 1 — declared-MSRV matrix (`feature_matrix.py --run-msrv`) → a `cargo make` task

`feature_matrix.py` cannot be deleted while the `msrv` job and `msrv-fresh.yaml` call it, so this goes first.

- New task `gate-msrv` in `Makefile.toml`: `cargo check` with `--all-targets --locked`, **the rows of
  `MSRV_ROW_NAMES`** exactly: `lib-all-features`, `lib-feature-async-std`, `lib-feature-smol`,
  `cli-all-features`. Literal commands, as in `gate-clippy`. Show in the request the old row's
  expanded command next to the new one (as the Q9a parity table did).
- The `msrv` job in `ci.yaml` keeps what it has (read the declared MSRV, `test "$MSRV" = "1.85"`, install
  that toolchain, cache) and runs `cargo make gate-msrv` instead of `scripts/release.py msrv`. Install
  `cargo-make` with the same pinned, cached step the Q9a jobs use. The MSRV toolchain is the job's default
  toolchain; document in the task's comment how to run it locally (`rustup run <MSRV> cargo make gate-msrv`).
- `msrv-fresh.yaml` (weekly, also dispatchable) keeps its triggers and MSRV steps. Its check becomes plain
  commands, in the workflow: export the tracked tree with `git archive HEAD` into a new empty directory
  outside the checkout, remove that copy's `Cargo.lock`, `cargo generate-lockfile` there (the MSRV cargo, so
  `resolver = "3"`'s MSRV-aware fallback applies), then `cargo make gate-msrv` **in that directory**. The
  repository's own lockfile is never touched, so the old "repository lockfile unchanged" check has nothing
  left to guard.
- **Dropped here, deliberately:** the evidence bundle and `upload-artifact` steps, the toolchain-identity
  table, and the "undeclared `rust-version` packages" count (informational; the passing fresh build is the
  control). Say so in the request.
- Demonstrate: a `compile_error!` behind `async-std` (or a `std` API newer than 1.85 in a scratch copy)
  fails `gate-msrv` for the right row; revert.
- `workflow_dispatch` the `msrv-fresh` workflow after the push and report the run.

### 1.2 Commit 2 — feature matrix

- Delete `scripts/feature_matrix.py` and `scripts/tests/test_feature_matrix.py`, and the CI jobs
  `matrix-setup` and `matrix`.
- `standard-gates` (feature matrix) becomes a required job: add it to `release-gate`'s `needs`.
- `bench-compile` and `Makefile.toml`'s `bench`/`bench-compile` call `feature_matrix.py --run-bench*`
  with `BENCH_FEATURES = ("json",)`. Replace with the literal `cargo bench` commands (read what the script
  runs; copy it), and report the expanded old and new commands.
- `Makefile.toml`: the tasks that call `feature_matrix.py` (`matrix-*`, `clippy-*`, `test-*` and their
  aliases) are removed, **except** that any task named in `CONTRIBUTING.md`, `README.md` or `docs/src/`
  keeps its name and now runs the matching `gate-*` task. Report your `grep` for task names and which you
  kept.
- `gate-features-assert` stays the unassigned-feature check (Q9a, accepted).
- The Q9a parity table is the proof. Re-run `cargo make gate-features` after the deletions (exit code and
  the 5289 / 259 totals).

### 1.3 Commit 3 — dependency security

- Delete `scripts/check_advisories.py`, `scripts/tests/test_check_advisories.py`, `security/advisory-policy.json`
  (and the empty `security/`), and the `dependency-security` CI job.
- `standard-deny` becomes required: add it to `release-gate`'s `needs`.
- `advisories.yaml` (Mondays; **it must keep running on a schedule**, because a new advisory appears without
  any push) now installs the pinned `cargo-deny` (cached, as in `standard-deny`) and runs
  `cargo make gate-deny`. No `cargo-audit`, no evidence upload.
- Demonstrate once more, on the final tree, the two behaviours the old gate had: a stale ignore fails
  (`RUSTSEC-2020-0071` in a scratch copy) and the `24544bc` tree fails with the same 15 ids. Reuse the Q9a
  logs only if the command line is byte-identical; otherwise re-run.
- `docs/src/dependency_security.md`: rewrite it for `deny.toml` (where the dispositions live; how to add or
  retire one: edit an `ignore` entry, with its reason and follow-up condition; the weekly run; the three
  pinned tool versions and where they are pinned: `Makefile.toml` header and `ci.yaml`/`advisories.yaml`).
  Re-read the whole page and fix anything else that is no longer true.

### 1.4 Part 1 gates

`cargo fmt --all --check`; `python3 -m unittest discover -s scripts/tests` (the suites that remain);
`python3 scripts/source_integrity.py --require-tracked` (still present in Part 1); `cargo make gate-features`,
`gate-msrv` (run with the MSRV toolchain), `gate-deny`; `mdbook build docs`. `release-gate` still runs and
still works with `scripts/release.py` present, because `aggregate-ci` is deleted only in Part 2: check that
its `--require-job` list matches the new `needs` list (jobs removed from `needs` must be removed from the
list in the same commit).

## 2. Part 2 — the archive, docs/package, version checks and the runner

### 2.1 Commit 4 — version consistency (RFC 009 R10, R11) extracted to one short script

New `scripts/check_versions.py` (about 100 lines, Python standard library only) containing exactly the logic
of `verify_version_references` and `verify_changelog_has_coming_version_section`, plus reading the
workspace version from the root `Cargo.toml` (`tomllib`, `[workspace.package].version`). Same
semantics, same messages, same fail-closed cases (README with no install example; an unparseable declaration
line; a stale version; a missing or empty CHANGELOG section). **No flags except an optional `--expect <version>`**
that `release.yaml` (Q9c) will use; the default compares to the workspace version.

- Move the tests that cover these functions out of `test_release_runner.py` into
  `scripts/tests/test_check_versions.py` unchanged in intent. Report how many moved and that each passes.
- CI: a small job `version-consistency` (`needs: source-integrity` while that job exists), added to
  `release-gate`'s `needs`.
- **Demonstrate** the RFC test-plan row "a stale install example fails the new CI": change one install line
  in `docs/src/` to an old version in a scratch copy; the script exits non-zero naming the file and line;
  revert.

### 2.2 Commit 5 — docs and package gate

New tasks and a job: `cargo doc --workspace --no-deps --all-features --locked`, `mdbook build docs`,
`cargo package --workspace --locked` (`gate-docs`). Copy the three commands from `doc_package_mode`
**exactly**; the old gate sets no `RUSTDOCFLAGS`, and adding `-D warnings` would change what the gate checks
(not in Q9b; if you think it should, say so in the request and the architect decides). The `doc-package`
job keeps its mdBook install step (pinned URL and SHA-256 exist and are fine); drop the evidence upload.

### 2.3 Commit 6 — the source archive (RFC 027 R3)

A task `gate-archive` (and the `archive` CI job running it). It:

1. builds `localcache-v<workspace version>.tar.gz` with `git archive --format=tar.gz` of `HEAD`, with the
   files at the archive root (RFC 009 R4; **the `v` is in the file name only**), into a directory outside
   the repository (`$RUNNER_TEMP` in CI, a `mktemp -d` locally);
2. checks archive completeness (RFC 009 R2): the sorted member list equals `git ls-files` (one command),
   with no `.git`, `.git-exclude`, `target` or `docs/book` member;
3. extracts into a new empty directory and runs, from there, `cargo metadata --locked --format-version 1`,
   `cargo check --workspace --all-targets --locked` and `mdbook build docs`, **with build output outside
   the source** (`CARGO_TARGET_DIR` and the mdBook destination outside the extracted tree);
4. prints the SHA-256 of the archive (the release notes will list it, Q9c). No two-builds determinism
   check, no structured tar parser, no export manifest: nothing untrusted produces this file.

**Demonstrate** the RFC test-plan row "a missing target fails": in a scratch clone, add a declared target
(`[[test]]`/`[[bench]]` with a `path`) whose file is not tracked; show `gate-archive` failing at step 2 or 3
with the log; revert.

### 2.4 Commit 7 — `source_integrity.py`: keep or delete, decided by a demonstration

RFC 027 R4 keeps it "only if the manifest-target check it performs is not already covered by `cargo
metadata --locked` failing on a missing target; the implementation shows which." Run, in a scratch clone,
for each of: (a) a manifest-declared target whose source file is missing; (b) a target source that exists
but is untracked; (c) a conventional-layout target (auto-discovered) that is untracked:

| Case | `python3 scripts/source_integrity.py --require-tracked` | `cargo metadata --locked` | `gate-archive` |
|---|---|---|---|

**Rule:** if `gate-archive` (or `cargo metadata --locked` on the checkout) fails on all three, delete
`source_integrity.py`, its test, the `source-integrity` job, the `needs: source-integrity` lines, and the
`Makefile.toml` task, and say so. If any case is caught **only** by `source_integrity.py`, keep the script
unchanged and report which case and why. Do not shrink it.

### 2.5 Commit 8 — the runner and the aggregation

- Delete `scripts/release.py`, `scripts/release_archive.py`, `scripts/release-tools.toml`,
  `scripts/tests/test_release_runner.py`, `scripts/tests/test_release_archive.py`, and every `Makefile.toml`
  task that called them (`release`, `release-check`, `archive`, `source`, `msrv`, `doc-package`, `security`,
  `version-check`, `changelog-check` …; keep `version-check`/`changelog-check` names only as tasks that run
  `check_versions.py`, if the book or `CONTRIBUTING.md` names them).
- Replace the `release-gate` job's body (`aggregate-ci`, evidence downloads) by the standard aggregator:
  `if: always()`, `needs:` every other job, one step that fails unless **every** needed job's result is
  `success` (print each result). Keep the job id and name (`release-gate`, "Release gate aggregation").
  The repository has no branch-protection rule today (`gh api …/branches/main/protection` → 404), so no
  required-check name is affected. A job that is skipped or cancelled **fails** the aggregator.
- No `upload-artifact`/`download-artifact` remains unless a job still needs to pass a file (report each
  remaining use).
- **Demonstrate** that the aggregator fails closed: in a scratch copy of the YAML, evaluate the step's shell
  with `needs` results `success success skipped` and `success failure success` (the step's body takes its
  inputs from an environment variable, so it can be run by hand); show exit codes.

### 2.6 Commit 9 — the book and the contributor docs

- `docs/src/source_archives.md`: rewrite for what exists after Q9b: the archive name and layout (unchanged),
  how to extract it safely (unchanged), `cargo make gate-archive`, what it checks, and that the release is
  published by the owner with `cargo publish --workspace --locked` **until Q9c**. Do not describe
  `release.yaml` (it does not exist yet; Q9c rewrites the procedure). Remove everything about the runner,
  `rc_eligible`, evidence bundles, the toolchain split and the structured tar validator.
- Search the book, `README.md`, `CONTRIBUTING.md`, `.github/` and `Makefile.toml` comments for the deleted
  names (`release.py`, `feature_matrix`, `check_advisories`, `release_archive`, `release-tools`,
  `advisory-policy`, `aggregate-ci`, `rc_eligible`) and fix every hit outside `CHANGELOG.md` and `ROADMAP.md`
  (history) and `rfcs/` (the RFCs are records; leave them). Paste the final grep.
- `CHANGELOG.md`: nothing user-visible, so no entry. If you think otherwise, say why.

### 2.7 Part 2 gates

`cargo fmt --all --check`; `python3 -m unittest discover -s scripts/tests`; `cargo make gate-features`,
`gate-msrv`, `gate-deny`, `gate-docs`, `gate-archive`; `python3 scripts/check_versions.py`;
`mdbook build docs`; `cargo test --workspace --all-features --locked`. After the push: CI green, **and** the
dispatched `msrv-fresh` and `advisories` workflows green. The architect confirms.

## 3. The request(s)

Two files, `.git-exclude/review-request/NNN-dev-q9b1-…-<date>.md` and `…-q9b2-…`, each describing the
**uncommitted** tree for its part (then, after approval, one commit per tool as listed, and a push). Each
contains: for every deleted file or job, the standard replacement and the line count removed; the
demonstrations above with exit codes and logs under `.git-exclude/tmp/`; every judgement call; every
required step, including those that pass; what could be verified only in CI.

## 4. Not yours, and already decided

- **Not in Q9b:** `release.yaml`, the Environment, the crates.io records, attestations (Q9c); any change to
  what a gate checks; `dependabot.yml` (exists); product code; `CHANGELOG.md`.
- **Dropped on purpose (record them in the request):** (1) the evidence bundles and toolchain-identity
  table; (2) the "undeclared `rust-version` count" in `msrv-fresh`; (3) the archive-determinism cross-check
  and the structured tar validator.
- **The old RC procedure ends here.** After Part 2, a release candidate is "CI green on the commit, including
  `archive`, `doc-package`, `msrv` and the dispatched `msrv-fresh` and `advisories`". The architect's RC
  handoffs of the 0.21 line no longer apply; Q9c documents the new procedure.
- `.git-exclude/release-candidate-v0.21.*` may be deleted by the dev team once Part 2 is accepted.
- Never pipe a command whose exit code matters; scratch under `.git-exclude/tmp/`; `cd` only with relative
  paths, after `pwd`; no personal data in a `User-Agent` or any request.
