# Q9b-3 Handoff — Retire `source_integrity.py`; Make `gate-versions` What It Says; Enforce Rustdoc Warnings

RFC: `rfcs/accepted/027-release-engineering-on-standard-tools.md` (R4: `source_integrity.py` is kept only if
nothing else covers it). Follows Q9b Part 2 (review `.git-exclude/reviewed/044-architect-q9b2-review-2026-10-30.md`).
Start only after the architect has confirmed CI on Part 2.
Review-request format: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6.
Authorship: high-capability model. Authorized: the owner (RFC 027).

## 0. Why

Part 2's demonstration found two cases (an untracked `build.rs` or `src/main.rs`) that only
`source_integrity.py` reports. The architect's Part 2 rule said "keep it", and you applied it correctly. The
same evidence also shows the rule's premise was too narrow:

- In CI the checkout **is** the commit, so an untracked file cannot exist there. The script's CI job cannot
  fire on those cases; it protects a developer's working tree, and only when the developer remembers to run it.
- A developer's working tree is already covered by a standard command: `cargo package --workspace --locked`
  (inside `gate-docs`) **refuses a tree with uncommitted or untracked files** (you showed the message in
  Part 2, `gate-docs-dirty.log`).
- The workspace has no `build.rs`; `crates/cli/src/main.rs` is tracked, and an absent `main.rs` fails the
  CLI build outright.

The owner's rule is *keep it simple, standard-oriented, clean*. 292 lines of script, 250 lines of tests, a CI
job that every other job waits for, and a book section, for a check CI cannot run, are not that. Retire it.

## 1. Three commits, in this order

### Commit A — retire `source_integrity.py`

- Delete `scripts/source_integrity.py`, `scripts/tests/test_source_integrity.py`, the CI job
  `source-integrity`, and every `needs: source-integrity` (the jobs that waited on it start immediately),
  including `release-gate`'s `needs`. Delete the `Makefile.toml` tasks `source-integrity` and
  `source-integrity-test`, and their header lines.
- `docs/src/source_archives.md`: replace the "Source integrity" section with two sentences: the archive is
  of `HEAD`, so a file that was never committed is not in it; `cargo package --workspace --locked` (in
  `cargo make gate-docs`) refuses a working tree that has uncommitted or untracked files, so run it before
  pushing. Search the whole repository (outside `CHANGELOG.md`, `ROADMAP.md`, `rfcs/`) for
  `source_integrity`, `source-integrity` and fix every hit; paste the grep.
- **Evidence to show, with exit codes, in scratch committed copies:** for the five cases of your Part 2 table
  — (a) declared target missing, (b) declared target untracked, (c1) nested untracked, (c2) untracked
  `build.rs`, (c3) untracked `src/main.rs` — the result of `cargo make gate-archive` (a, b) and of
  `cargo package --workspace --locked` on the dirty tree (b, c1, c2, c3). Say plainly which cases remain
  uncaught after the retirement and why that is acceptable (c1 was uncaught by every check before).

### Commit B — `gate-versions` is what its description says

Part 2's header says `gate-versions` runs "script tests; install examples and CHANGELOG name the version",
and your judgement call 8 says it runs the script tests first, but the task is only
`python3 scripts/check_versions.py`. Make the task do what you described: `python3 -m unittest discover -s
scripts/tests` and then `python3 scripts/check_versions.py` (a two-line script with `set -e`). The CI job
`version-consistency` takes over the script tests that the `source-integrity` job ran: it runs the same two
commands (plain `python3`, no `cargo-make` install). Show the job's step list.

### Commit C — rustdoc warnings fail the gate

`gate-docs` runs `cargo doc …` **with `RUSTDOCFLAGS="-D warnings"`** (set on that command only). Every
slice handoff since v0.21.5 already requires it, and a rustdoc warning in a published crate is a defect;
this is a strengthening of the gate, decided by the architect, not part of the parity. Show: the gate passes
on the tree (exit 0); a deliberately broken intra-doc link in a scratch copy fails it (exit and the rustdoc
message); the `doc-package` job needs no other change. If the tree does not pass today, **stop and report
the warnings**; do not fix them in this commit.

## 2. Gates and the request

`cargo fmt --all --check`; `python3 -m unittest discover -s scripts/tests`; `cargo make gate-versions`,
`gate-features`, `gate-deny`, `gate-docs`, `gate-archive` (the last two in a committed scratch copy);
`rustup run 1.85.0 cargo make gate-msrv`; `mdbook build docs`. Tree objects per commit as in Parts 1 and 2;
one review request, `.git-exclude/review-request/NNN-dev-q9b3-…-<date>.md`; push once after approval; no
workflow other than `ci.yaml` changes. Expected CI: **9 jobs** (`fmt`, `version-consistency`,
`standard-gates`, `standard-deny`, `bench-compile`, `msrv`, `archive`, `doc-package`, `release-gate`) plus Docs.

## 3. Not yours

`release.yaml`, Environments, crates.io records (Q9c); any change to what another gate checks; product code;
`CHANGELOG.md`; `ROADMAP.md`. `.git-exclude/release-candidate-v0.21.6/` may be deleted.
