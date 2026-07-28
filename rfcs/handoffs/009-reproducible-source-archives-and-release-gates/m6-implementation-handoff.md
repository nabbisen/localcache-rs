# RFC 009 M6 Implementation Handoff

## 1. Summary

Implement the M6 release-controls milestone of
[RFC 009](../../accepted/009-reproducible-source-archives-and-release-gates.md), together with
[RFC 016](../../proposed/016-published-crate-legal-file-completeness.md) once it is Accepted.

RFC 009 is Accepted and **its requirements are authoritative**; this handoff sequences the work and
names the concrete gaps, but adds no design decision and overrides none. Where this document and an
RFC appear to disagree, the RFC wins — report the discrepancy rather than choosing.

M6 is decomposed into five slices (M6a–M6e) matching `ROADMAP.md` § "M6 slice breakdown". Each slice
is an independent implementation review point. **M6 closes B-07, the last open blocker**, but by
itself authorizes no tag, publication, or release; that is M7 plus explicit owner authorization.

The M1 slice of RFC 009 is already implemented and accepted (`e54cfe2`); see
`implementation-handoff.md` in this directory for what already exists. **Do not create a second
release runner** — M6 extends `scripts/release.py`, `scripts/release_archive.py`, and
`scripts/release-tools.toml` in place.

## 2. Scope followed

### M6a — Published-crate legal files *(RFC 016; blocked on owner acceptance)*

Gap: `localcache-0.20.0.crate` contains no `LICENSE`/`NOTICE`; `localcache-cli-0.20.0.crate`
contains neither plus no `README`. Publication is blocked until fixed.

1. Add tracked `LICENSE` and `NOTICE` to `crates/localcache/` and `crates/cli/` as byte-identical
   mirrors of the repository-root files.
2. Add a source-context gate step comparing each mirror to its root original by raw bytes.
3. Add an **in-artifact** check: after `cargo package --workspace --locked`, open each produced
   `.crate` and assert both files are present with matching bytes. Presence on disk is not
   sufficient — that is exactly the condition under which the defect went unnoticed.
4. Resolve the CLI `readme` situation so the field, if set, names a packaged file.

Do not use symlinks (RFC 009 R5 forbids link members), and do not materialise files at package time
(RFC 009 R9 forbids `--allow-dirty`).

### M6b — Canonical gate consolidation *(RFC 009 R7, R12, R13; RFC 014)*

Known gaps, each verified present in the tree:

1. `Makefile.toml` `ALL_FEATURES` omits `async-std`, `smol`, and `opentelemetry`.
2. CI clippy invocations do not all pass `-- -D warnings`.
3. `scripts/check_advisories.py` is **not** listed in `scripts/release-tools.toml`
   `[implementations]` — it is the only gate implementation whose hash is unverified.
4. `scripts/release.py` has no dependency-security step; R13 requires one with fail-closed
   aggregation, and R14 requires the policy and advisory-database digests in the evidence bundle.
   The checker already emits both (`advisory-policy.sha256`, `rustsec-database.json`), so this is
   plumbing rather than new design.
5. R7's canonical matrix (no-feature, each library feature individually, all-features, non-Tokio
   async-std, non-Tokio smol, the two CLI rows, and an explicit locked doctest row) must be driven
   from **one** checked-in source that `Makefile.toml` and CI both invoke. A new unassigned feature
   must fail closed.

**This slice closes B-07.** Record that in its review request.

### M6c — Canonical producer and CI provenance *(RFC 009 R3–R6, R14, R16)*

1. Execute `scripts/canonical-producer.sh` end-to-end. It has **never been run** — the M1 evidence
   used a checksum-verified OCI filesystem under `bwrap` because Docker was unavailable, and that
   exception was accepted for M1 only.
2. Prove two consecutive canonical builds from one commit produce an identical SHA-256.
3. Add CI archive construction plus a final aggregator that fails closed when any required row or
   artifact is missing, duplicated, stale, or bound to a different workflow run or commit.
4. Enforce R16: explicit `permissions: contents: read`, every third-party action pinned to an
   immutable commit SHA, no `pull_request_target` execution of untrusted code, no publish or
   repository secrets in build/verification jobs.
5. Replace self-asserted RC eligibility with an external binding. Today `producer_class: canonical`
   and `rc_eligible: true` follow from an environment variable the runner reads about itself; bind
   them to the wrapper or CI run identity instead.
6. Re-assert the R4/R5 layout **after** the artifact smoke run, so "build output stayed outside the
   extracted source" is observed rather than assumed.
7. Finalize the failure summary on unexpected exceptions — `main()` currently catches only three
   error types, so an `OSError` can leave `summary.log` reading `status: RUNNING`.
8. Replace the per-binary SHA-256 pins in `[supported-host-tools]` with an explicit, non-empty
   claimed-platform policy. The current pins describe one workstation, which is why the
   noncanonical runner cannot execute in CI.
9. **(Added after the M6b implementation review, N-1.)** M6b consolidated the R7 feature-and-package
   matrix into `scripts/feature_matrix.py`, but the declared-MSRV rows (`ci.yaml` `msrv` job's four
   `cargo check` invocations) and the bench-compile invocation (`Makefile.toml` `bench-compile`/
   `bench`, `ci.yaml` `bench-compile`) still hand-write their own `--features` lists outside that
   source. This is exactly the drift class M6b/R12 exists to eliminate: a new async-runtime feature
   is caught by `feature_matrix.py --check-coverage` but silently missed by the MSRV matrix. Move
   these rows into `feature_matrix.py` (or a sibling canonical source `Makefile.toml` and CI both
   invoke) and extend the coverage check so a new runtime feature cannot be added without a
   corresponding MSRV row. RFC 009 M6 sequence item 2 ("integrate the exact MSRV policy delivered by
   RFC 014") requires this before M6 as a whole can be recorded complete; it is not required for M6c
   specifically to close.

### M6d — Coming-version housekeeping *(RFC 009 R10, R11)*

1. Set the authorized coming version across `[workspace.package]`, both members, the CLI's registry
   dependency on the library, `CHANGELOG.md`, `README.md`, and the mdBook install examples.
2. Add the version-reference consistency gate. It must currently fail: `README.md:52`,
   `docs/src/getting_started.md:9`, and `docs/src/introduction.md:7` say `0.20.1` while both
   packages say `0.20.0`.
3. Deliberate compatibility ranges and historical changelog entries must be excluded explicitly, not
   silently rewritten.
4. Refresh implemented-RFC prose and add the RFC 002 supersession note when RFC 011 moves.

**`0.20.0` is already tagged.** Any archive built before this slice carries a colliding name.

### M6e — RC construction and evidence *(RFC 009 R9, R14)*

1. `cargo doc --workspace --no-deps --all-features --locked`, `mdbook build docs`, and
   `cargo package --workspace --locked` — no `--allow-dirty`, no `--no-verify`.
2. Joint package verification including M6a's in-artifact legal-file assertion.
3. Canonical-environment archive plus SHA-256.
4. Evidence bundle carrying every R14 field, including the RFC 014 policy revision and the
   advisory-database revision. A required step that was skipped must render the summary a failure.

### Explicitly out of scope

- Any tag, push, publication, yank, or hosted release. M6 ends at a verified RC.
- `LocalFileCacheError` `#[non_exhaustive]` and a dedicated poisoning variant — 0.21.0.
- Size-driven splitting of `repository.rs`, `query.rs`, `cli/src/main.rs`, or `indexes.rs`.
- RFC 011 N-01/N-02 and the RFC 010 migration space/time documentation — opportunistic, and they
  must not expand a slice's diff.
- Changing the SPDX `license` field to `license-file`.

### Sequencing

`M6a` ∥ `M6b` → `M6c` → `M6d` → `M6e` → M7.

M6a is independent and may run in parallel with M6b once RFC 016 is Accepted. M6c depends on M6b's
canonical runner. M6d must follow the others because R10 sets the version immediately before the
final gates. M6e consumes all of them.

## 3. Files to change

- `crates/localcache/{LICENSE,NOTICE}`, `crates/cli/{LICENSE,NOTICE}` — **new**, M6a.
- `scripts/release.py` — M6a gate steps, M6b security step, M6c provenance and summary finalization,
  M6d version gate, M6e evidence.
- `scripts/release-tools.toml` — M6b hash pin; M6c platform policy and producer manifest.
- `scripts/release_archive.py` — only if M6c's layout re-assertion needs it; otherwise untouched.
- `scripts/canonical-producer.sh` — M6c, only if execution reveals a defect.
- `Makefile.toml`, `.github/workflows/ci.yaml` — M6b aliasing, M6c provenance.
- `Cargo.toml`, `crates/*/Cargo.toml`, `CHANGELOG.md`, `README.md`, `docs/src/*.md` — M6d.
- `scripts/tests/` — tests for every new gate step, in the existing unittest style.

## 4. Design decisions and assumptions

- **One runner, extended in place.** R12 requires a single canonical gate implementation;
  `Makefile.toml` and CI are orchestration and aliases only.
- **Fail closed everywhere.** A missing tool, a skipped required row, or an unparseable result is a
  failure, never a pass. This already holds in `check_advisories.py`; match its shape.
- **Evidence must bind to identity.** Commit SHA, archive digest, toolchain versions, and CI run ID
  travel together; a human-readable summary may never disagree with the machine exit status.
- **Legal files are verified mirrors, not independent copies.** Root remains the file a human edits.
- **`0.20.0` is published and tagged.** Version immutability applies; the coming version is set in
  M6d and confirmed before the RC.

### Escalation triggers — stop and report

1. Any need to add a symlink, or to weaken RFC 009 R5's link prohibition.
2. Any need for `--allow-dirty` or `--no-verify` during packaging.
3. Canonical producer builds that are not byte-identical across two runs from one commit.
4. Any gate that cannot be driven from the canonical source without duplicating its command list.
5. Discovering that joint workspace packaging cannot verify both interdependent crates — RFC 009 R9
   requires the RFC to return for design review before an isolated-registry strategy is adopted.

## 5. Tests and gates to run

Per-slice minimum, run after each slice rather than once at the end:

```sh
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked
cargo test --workspace --doc --all-features --locked
cargo fmt --all --check
python3 -m unittest discover -s scripts/tests -v
python3 scripts/source_integrity.py --require-tracked
```

Plus the RFC 014 R8 declared-MSRV matrix (all four rows on the declared toolchain) after any slice
touching manifests, features, or dependencies — M6b and M6d certainly.

Full acceptance criteria are in `m6-acceptance-qa-checklist.md`.

## 6. Generated artifacts

M6e produces the RC archive and evidence bundle. Both are build outputs: they must live outside the
source tree or under the ignored `.git-exclude/` boundary, and must never enter the project source
archive. Generated `docs/book/` must be removed before any review request.

## 7. Known limitations

- The canonical producer requires Docker. If it is still unavailable, that is an **escalation**, not
  a repeat of M1's `bwrap` exception — M6c exists specifically to close that gap.
- The two `warn` advisory dispositions carry an `expires` field; once reached the security gate
  denies. Renewing or resolving them is an owner decision that gates M6b's security step.
- RFC 016 is Proposed. M6a cannot begin until it is Accepted, and its owner decision is the only
  design question outstanding in the entire milestone.

## 8. Recommended next step

Implement in the §2 order, gating after each slice, and prepare a focused implementation review
request per slice that identifies the exact commit, maps each requirement to its evidence, and
discloses any gate that failed or was not run rather than omitting it.

After M6e, request the M7 independent architecture review of the RC and the extracted archive. Do
not record Phase 21 complete, move any RFC to `done/`, or perform any release action until M7 is
accepted and the owner authorizes it.
