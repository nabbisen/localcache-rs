# RFC 009 M6 Acceptance and QA Checklist

Operationalizes M6 of [RFC 009](../../accepted/009-reproducible-source-archives-and-release-gates.md)
and, for M6a, [RFC 016](../../proposed/016-published-crate-legal-file-completeness.md).
The RFCs remain authoritative; this list adds and relaxes nothing.

Testing is owned by the testing developer. **Every box must be backed by an observed result. An
unrun check is a failure, not a pass** — R14 requires that a summary cannot report success when a
required step was skipped, and the same standard applies to this checklist.

Checks are grouped by slice; each slice is a separate review point.

## Preconditions (all slices)

- [ ] The implementation commit under test is identified exactly.
- [ ] The tracked worktree is clean.
- [ ] No publish, tag, push, yank, or hosted-release action is present in the change or in CI.
- [ ] No schema, payload wire-format, Rust API, or Cargo feature name changed.
- [ ] Deferred items did not creep in: no `#[non_exhaustive]` error-enum change, no size-driven
      splitting of `repository.rs` / `query.rs` / `cli/src/main.rs` / `indexes.rs`.

## M6a — Published-crate legal files

- [ ] RFC 016 is Accepted with the owner decision recorded before implementation began.
- [ ] `crates/localcache/LICENSE`, `crates/localcache/NOTICE`, `crates/cli/LICENSE`, and
      `crates/cli/NOTICE` are tracked regular files — **not** symlinks.
- [ ] Each mirror is byte-identical to the repository-root file of the same name.
- [ ] The drift gate fails when one mirror is altered by a single byte.
- [ ] The drift gate fails when one mirror is deleted.
- [ ] `cargo package --workspace --locked` succeeds without `--allow-dirty` and without
      `--no-verify`.
- [ ] **`localcache-<version>.crate` contains `LICENSE` and `NOTICE`, with bytes matching root.**
- [ ] **`localcache-cli-<version>.crate` contains `LICENSE` and `NOTICE`, with bytes matching root.**
- [ ] The in-artifact check **fails** when a file is present at the repository root but absent from
      the `.crate` — the original defect shape, exercised deliberately.
- [ ] The CLI's `readme` field, if set, names a file present in its package.
- [ ] `license = "Apache-2.0"` is unchanged; `license-file` was not introduced.
- [ ] The source archive still validates: no link members, export manifest matches, extraction
      succeeds, and it now contains the four new members.

## M6b — Canonical gate consolidation (closes B-07)

- [ ] `Makefile.toml` `ALL_FEATURES` covers every library feature including `async-std`, `smol`,
      and `opentelemetry`.
- [ ] Every clippy invocation in `Makefile.toml` and CI passes `-- -D warnings`.
- [ ] `scripts/check_advisories.py` appears in `scripts/release-tools.toml` `[implementations]`
      with a matching SHA-256, and the runner verifies it before any gate runs.
- [ ] Altering `check_advisories.py` by one byte fails the producer-tool verification.
- [ ] `scripts/release.py` exposes a dependency-security step with fail-closed aggregation.
- [ ] A denied advisory result fails the release gate; the failure is visible in the summary.
- [ ] The R7 matrix is driven from one checked-in source: no-feature row, each library feature
      individually, all-features, non-Tokio async-std, non-Tokio smol, both CLI rows, and an
      explicit locked doctest row.
- [ ] Library rows select only `-p localcache`; CLI rows are not counted as library coverage.
- [ ] Adding an unassigned library feature **fails closed** until a row covers it.
- [ ] `Makefile.toml` and CI invoke the canonical source rather than maintaining their own command
      lists.
- [ ] The declared-MSRV matrix reads `rust-version` and installs that exact toolchain; a job using
      stable is not named or counted as MSRV.
- [ ] **B-07 is closed**, with the evidence identified in the review request.

## M6c — Canonical producer and CI provenance

- [ ] `scripts/canonical-producer.sh` was executed **end-to-end**, not emulated.
- [ ] Two consecutive canonical builds from one commit produce an identical SHA-256.
- [ ] Each explicitly claimed non-canonical platform produces the same normalized content manifest;
      the claimed platform list is non-empty and checked in.
- [ ] `[supported-host-tools]` no longer pins single-workstation binary hashes in a way that
      prevents CI execution.
- [ ] CI constructs and verifies an archive from a clean commit.
- [ ] A final CI job fails when any required row or artifact is missing, duplicated, stale, or bound
      to another workflow run or commit.
- [ ] Workflows declare explicit `permissions: contents: read`.
- [ ] Every third-party action is pinned to an immutable commit SHA.
- [ ] No `pull_request_target` execution of untrusted repository code.
- [ ] No publish, registry, or repository secrets are available to build/verification jobs.
- [ ] RC eligibility is bound externally (wrapper or CI run identity), not self-asserted by the
      runner reading its own environment.
- [ ] The R4/R5 layout is re-asserted **after** the artifact smoke run, proving build output stayed
      outside the extracted source.
- [ ] An unexpected exception (for example a write failure) still finalizes `summary.log` as
      `FAIL` — it never leaves `status: RUNNING`.

## M6d — Coming-version housekeeping

- [ ] Workspace and both member versions equal the authorized coming version.
- [ ] The CLI's registry dependency on the library equals that same version exactly.
- [ ] `CHANGELOG.md` has a non-empty section for the coming version with its intended date or an
      approved RC placeholder.
- [ ] `README.md` and every mdBook install example reference the coming version; the previously
      observed `0.20.1`-vs-`0.20.0` split is gone.
- [ ] The version-reference gate **fails** when a stale previous-version install example is
      reintroduced.
- [ ] Deliberate compatibility ranges and historical changelog entries are excluded explicitly, not
      silently rewritten.
- [ ] The archive filename matches Cargo metadata exactly.
- [ ] The coming version is not marked released in roadmap or RFC prose before owner authorization.
- [ ] Implemented-RFC prose is refreshed; the RFC 002 supersession note is present if RFC 011 moved.

## M6e — RC construction and evidence

- [ ] `cargo doc --workspace --no-deps --all-features --locked` passes.
- [ ] `mdbook build docs` passes; generated `docs/book/` is absent from the archive and removed
      afterward.
- [ ] `cargo package --workspace --locked` verifies **both** interdependent packages.
- [ ] The evidence bundle carries every R14 field: project and coming version, UTC timestamp and
      timezone, exact commit SHA, clean-worktree assertion, host platform, canonical producer
      identity, target triple, git/archive/compressor versions, stable and declared-MSRV compiler
      and Cargo versions, mdBook and security-tool versions, every command or gate-mode name,
      per-step exit status and log, archive filename, byte size and SHA-256, layout assertion,
      extracted-archive results, CI run ID and commit binding, and the RFC 014 policy and
      advisory-database revisions.
- [ ] The summary **cannot** report pass when a required step was skipped — demonstrated, not
      assumed.
- [ ] Evidence contains no registry token, environment dump, encryption key, or private review
      material.
- [ ] Historical logs from another checkout were not substituted for current RC evidence.

## Phase exit readiness (verify before requesting M7)

- [ ] All eight blocking findings B-01…B-08 are closed with tests or reproducible gate evidence.
- [ ] Checkout and extracted archive pass the same applicable gates.
- [ ] Stable and declared MSRV pass the complete target/feature policy.
- [ ] Historical fixtures prove payload preservation from v1 and v4.
- [ ] Security scanning passes the approved deny/warn/exception policy.
- [ ] Every residual pre-RC correction is closed with regression evidence.
- [ ] Documentation, implemented-RFC prose, Cargo metadata, CI, and release tooling describe one
      consistent contract.
- [ ] A fresh evidence bundle identifies the exact commit, toolchains, commands, results, and
      archive.
- [ ] No release action has been performed; the release decision remains M7 plus owner
      authorization.
