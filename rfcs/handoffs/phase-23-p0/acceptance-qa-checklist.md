# Phase 23 P0 Acceptance and QA Checklist — v0.21.1

Operationalizes `implementation-handoff.md` in this directory. That document is
authoritative; this list adds and relaxes nothing.

**Every box must be backed by an observed result. An unrun check is a failure, not a pass.**

Each part is independently reviewable. Tick only the parts under review and say which.

## Preconditions (all parts)

- [ ] The implementation commit under test is identified exactly.
- [ ] The tracked worktree is clean.
- [ ] No version was bumped; `CHANGELOG.md` has no `0.21.1` heading yet.
- [ ] No schema, migration, payload wire format, or SQL change is in the diff.
- [ ] No new crate and no new runtime dependency were added.

## Part A — Glob guidance

- [ ] `path_glob`'s **rustdoc** states the leading-literal rule — the call site a user sees
      in their editor, not only the book.
- [ ] `docs/src/querying.md`'s glob section states it.
- [ ] Both link to `docs/src/performance.md` rather than restating the measured numbers.
- [ ] `path_glob`'s behaviour, signature, and validation are **unchanged**.
- [ ] `mdbook build docs` succeeds; generated `docs/book/` is removed afterwards.

## Part B — `ConnectionPool` batch length

- [ ] `batch_get`, `batch_get_fresh`, and `check_status_batch` each return **one result per
      requested path** on lock failure — verified by a test asserting
      `results.len() == paths.len()`, not by reading the diff.
- [ ] The error is `Poisoned { resource: "ConnectionPool" }`.
- [ ] Each method's rustdoc documents the per-path guarantee, as `ReadPool`'s does.
- [ ] **No test, doctest, or example depended on the single-element shape.** If one did, that
      was **reported** rather than quietly updated.
- [ ] `namespace_copy` and `import_from` no longer duplicate a byte-identical body.
- [ ] **Both public methods keep their names, signatures, and documented behaviour.**
- [ ] If the two were found to be meaningfully different rather than duplicated, that was
      reported instead of unified.

## Part C — Tooling hygiene

- [ ] `fetch_with_retry` catches a **dedicated transient error type**, not the broad
      `AdvisoryGateError`.
- [ ] `live_fetch` raises that type only for network conditions.
- [ ] A non-5xx status, a validation failure, and a size-limit breach are each still **never
      retried** — each demonstrated, not assumed.
- [ ] `security/advisory-policy.json`'s `follow-up` values are **self-describing sentences**;
      the reporter emits them verbatim without prepending prose.
- [ ] Gate output still names each standing disposition's re-raise condition and prints no
      expiry date.
- [ ] `error.rs`'s doctest is pinned to ```` compile_fail,E0004 ```` and still passes.
- [ ] `scripts/release-tools.toml`'s `check-advisories` hash pin matches the modified file.
- [ ] A one-byte change to `check_advisories.py` still fails producer-tool verification.
- [ ] `scripts/tests` suite passes, counts before and after reported.
- [ ] The same suite passes under RC-3's **restricted `PATH`** (no `cargo`/`rustc`/`mdbook`/
      `rustup`/`cargo-audit`).
- [ ] `python3 scripts/release.py security` exits 0 with `denied=0`.

## Part E — Async test deduplication

- [ ] `pool_observe.rs`'s three runtime modules are generated from one body by a
      `macro_rules!` helper.
- [ ] **No new crate**; `syn`, `quote`, and `proc-macro2` are absent from `Cargo.toml` and
      `Cargo.lock`.
- [ ] **Test count is unchanged** — reported before and after. A macro that collapses three
      tests into one is a coverage regression a green suite would not reveal.
- [ ] Each generated module keeps its current name, so a failure remains attributable to a
      specific runtime.
- [ ] Every generated test still runs under its own runtime feature — confirmed via the
      feature matrix, not `--all-features` alone.
- [ ] Single-runtime async tests elsewhere were **not** swept into the macro.
- [ ] Any test that looked triplicated but differs subtly between runtimes was **left alone
      and reported**.

## Gates (all parts)

- [ ] Full test suite passes; counts before and after reported.
- [ ] `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` clean.
- [ ] `cargo fmt --all --check` clean.
- [ ] `feature_matrix.py --run-all` exits 0.
- [ ] The declared-MSRV matrix passes on 1.85.0.
- [ ] `python3 scripts/source_integrity.py --require-tracked` OK.
- [ ] **Any new module or test file is tracked** — `source_integrity.py` does not catch an
      untracked `#[cfg(test)]` submodule, which was nearly lost in N1.
- [ ] `git diff --check` clean.

## Scope containment

- [ ] No public API changed, other than Part B's return-length correction on the lock-failure
      path.
- [ ] If any change would have made the release **breaking**, that was reported before
      proceeding — v0.21.1 is a patch target and a breaking change invalidates it.
- [ ] Part C's two `check_advisories.py` edits landed together in one commit with the pin
      update, per the handoff's same-file note.
