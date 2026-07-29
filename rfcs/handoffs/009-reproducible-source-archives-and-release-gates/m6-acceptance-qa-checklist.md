# RFC 009 M6 Acceptance and QA Checklist

Operationalizes M6 of [RFC 009](../../accepted/009-reproducible-source-archives-and-release-gates.md)
M6a's governing RFC 016 was [withdrawn](../../archive/016-published-crate-legal-file-completeness.md) on 2026-07-28.
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

## M6a — Published-crate legal files — WITHDRAWN

> **RFC 016 was withdrawn 2026-07-28.** Verify the **revert**, not the implementation.

- [ ] `crates/localcache/{LICENSE,NOTICE}` and `crates/cli/{LICENSE,NOTICE}` are absent.
- [ ] Repository-root `LICENSE` and `NOTICE` are unchanged.
- [ ] `grep -rn "verify_legal_file_mirrors\|verify_package_legal_files\|LEGAL_FILE_MIRRORS" scripts/`
      returns nothing.
- [ ] `scripts/release.py` has no `package` subcommand added for legal files, and `REQUIRED_PATHS`
      has no mirror entries.
- [ ] Every `scripts/release-tools.toml` `[implementations]` hash pin matches its file.
- [ ] `docs/src/source_archives.md` has no "Published-crate legal files" section.
- [ ] **`crates/cli/Cargo.toml` still has `readme.workspace = true`**, and
      `cargo package --list -p localcache-cli` includes `README.md`.
- [ ] `cargo package --workspace --locked` succeeds with no new warning.
- [ ] `license = "Apache-2.0"` unchanged; no `license-file` introduced.
- [ ] `python3 scripts/release.py source --output-dir <new>` passes end to end.
- [ ] Test suite back to its pre-M6a count and passing; fmt, source-integrity, `git diff --check` clean.

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

## M6c — CI provenance

> **Withdrawn by RFC 017 (accepted 2026-07-28)** — do not verify these; there is no container
> producer and no compressed-byte identity in the contract any more. Their replacements are in
> § "M6e — RFC 017 migration".
>
> - ~~`scripts/canonical-producer.sh` was executed end-to-end, not emulated.~~
> - ~~Two consecutive canonical builds from one commit produce an identical SHA-256.~~
> - ~~Each explicitly claimed non-canonical platform produces the same normalized content manifest.~~
> - ~~`[supported-host-tools]` no longer pins single-workstation binary hashes.~~ *(The pins were in
>   fact removed in `d86fda7`; RFC 017 then removed the tables outright.)*

- [ ] CI constructs and verifies an archive from a clean commit.
- [ ] A final CI job fails when any required row or artifact is missing, duplicated, stale, or bound
      to another workflow run or commit.
- [ ] Workflows declare explicit `permissions: contents: read`.
- [ ] Every third-party action is pinned to an immutable commit SHA.
- [ ] No `pull_request_target` execution of untrusted repository code.
- [ ] No publish, registry, or repository secrets are available to build/verification jobs.
- [ ] ~~RC eligibility is bound externally (wrapper or CI run identity), not self-asserted by the
      runner reading its own environment.~~ **Withdrawn by RFC 017 R3** — RC eligibility now derives
      from gates rather than from any environmental claim. Verified in § M6e instead.
- [ ] The R4/R5 layout is re-asserted **after** the artifact smoke run, proving build output stayed
      outside the extracted source.
- [ ] An unexpected exception (for example a write failure) still finalizes `summary.log` as
      `FAIL` — it never leaves `status: RUNNING`.

### M6c — R12 residual consolidation (implementation handoff § M6c item 9)

- [ ] The four declared-MSRV rows are driven from the canonical source; `ci.yaml`'s `msrv` job no
      longer hand-writes `cargo check … --features localcache/async-std` and its siblings.
- [ ] The bench-compile invocation is driven from the canonical source; `Makefile.toml`
      `bench-compile`/`bench` and `ci.yaml` `bench-compile` no longer restate
      `--features localcache/json`.
- [ ] `grep -rnE '\-\-features localcache/' Makefile.toml .github/workflows/ci.yaml` returns
      nothing — no feature literal survives outside the canonical source.
- [ ] The coverage check is extended to the MSRV rows: adding a declared runtime feature with no
      MSRV row **fails closed**, demonstrated the same way the R7 check was.
- [ ] All four MSRV rows still pass on the declared toolchain after the move, verified
      individually.
- [ ] A `--run` invocation that selects zero executable modes (for example
      `--run workspace-doctest --modes clippy`) returns **nonzero** rather than silently
      succeeding; `--run-all` still skips that combination, since the row is covered by its other
      mode.

## M6d — Coming-version housekeeping

- [ ] Workspace and both member versions equal the authorized coming version.
- [ ] The CLI's registry dependency on the library declares a registry-compatible version
      requirement. **Exact equality is not required** — per the 2026-07-28 owner resolution recorded
      under RFC 009 R9, workspace-internal path dependencies are exempt, and `workspace_version()`
      deliberately does not inspect the requirement. Verify only that the requirement is present and
      parseable by `cargo metadata`.
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

### RFC 017 migration (verify before the RC checks below)

- [ ] The archive's primary integrity identifier is the **uncompressed-tar** SHA-256; the compressed
      size and digest are recorded and labelled **advisory**.
- [ ] Two consecutive builds from one clean commit **on the same host** produce an identical
      uncompressed-tar SHA-256.
- [ ] No gate compares compressed bytes across hosts, and no evidence or release note asserts
      cross-host byte identity.
- [ ] `rc_eligible` is **false** for a dirty tree — demonstrated.
- [ ] `rc_eligible` is **false** when any required gate failed — demonstrated.
- [ ] `rc_eligible` is **false** when a required evidence step was skipped — demonstrated.
- [ ] `rc_eligible` is **true** for a clean commit with all gates green and complete evidence, with
      **no environment variable involved**.
- [ ] No **live** use of the retired identifiers remains. Run:
      `grep -rnE 'RFC009_PRODUCER_IMAGE|RFC009_RC_ELIGIBLE|canonical-producer|canonical-base-components|supported-host-tools|supported-platforms' scripts .github Makefile.toml rfcs/handoffs`
      and confirm every hit is one of: a comment explaining the retirement, a test asserting the
      identifier's **absence**, or struck-through handoff text recording the withdrawal. Any hit that
      is executable code, a CI step, or a live TOML key is a failure.
      *(Corrected 2026-07-28: the original wording demanded the grep return nothing, which is
      unsatisfiable — the absence tests necessarily name what they assert is gone.)*
- [ ] `scripts/canonical-producer.sh` is deleted and its `[implementations]` pin removed; every
      remaining pin still matches its file.
- [ ] The CI `archive` job no longer passes `--noncanonical`.
- [ ] Every RFC 009 **R5** archive test still passes unchanged, including the hostile-fixture set —
      structured header validation, exact export manifest, Git blob identity, and link rejection are
      untouched.
- [ ] Evidence records every RFC 017 R4 identity field (platform, `git`, Python, zlib, locale,
      timezone, stable and declared-MSRV `rustc`/`cargo`, mdBook, security tool); a missing field
      fails the bundle.

### RC construction

- [ ] `cargo doc --workspace --no-deps --all-features --locked` passes.
- [ ] `mdbook build docs` passes; generated `docs/book/` is absent from the archive and removed
      afterward.
- [ ] `cargo package --workspace --locked` verifies **both** interdependent packages.
- [ ] The evidence bundle carries every R14 field: project and coming version, UTC timestamp and
      timezone, exact commit SHA, clean-worktree assertion, host platform, RFC 017 R4
      toolchain identity (git, Python, zlib, locale, timezone), target triple, stable and declared-MSRV compiler
      and Cargo versions, mdBook and security-tool versions, every command or gate-mode name,
      per-step exit status and log, archive filename, byte size and SHA-256, layout assertion,
      extracted-archive results, CI run ID and commit binding, and the RFC 014 policy and
      advisory-database revisions.
- [ ] The summary **cannot** report pass when a required step was skipped — demonstrated, not
      assumed.
- [ ] **A canonical `release` entry point exists** (RFC 009 R12) and runs the gates in the Design
      section's order with a **hard-coded** required set — not one supplied by the caller. Forcing one
      gate to fail makes it exit nonzero and the consolidated summary read `FAIL`.
- [ ] **`aggregate-ci` fails closed when a required gate is omitted.** Invoking it with only a subset
      of `--require-job` values must **fail**, not pass. *(Before this correction it exited 0 having
      verified only `source`, with `msrv`, `doc-package`, and `security` unmentioned — reproduce that
      invocation and confirm it now fails.)*
- [ ] Evidence contains no registry token, environment dump, encryption key, or private review
      material.
- [ ] Historical logs from another checkout were not substituted for current RC evidence.

- [ ] **RC-2:** `python3 scripts/release.py release --output-dir <new>` completes end to end in **one**
      invocation under ambient stable, with the `msrv` gate run under the declared toolchain.
- [ ] **RC-2:** `release` mode **fails** (not skips) when the declared toolchain is not installed.
- [ ] **RC-2:** the declared toolchain version is read from `[workspace.package].rust-version`, not
      hard-coded.
- [ ] **RC-2:** `Cargo.toml`'s `localcache` requirement is unchanged, and `verify_declared_toolchain`
      is unchanged.
- [ ] **RC-2 precondition:** `cargo +1.85.0 package -p localcache-cli --locked --allow-dirty` was run
      and its resolved `localcache` version reported, confirming (or refuting) the diagnosed mechanism
      before the fix was implemented.

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
