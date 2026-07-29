# RFC 009 M6 Implementation Handoff

## 1. Summary

Implement the M6 release-controls milestone of
[RFC 009](../../accepted/009-reproducible-source-archives-and-release-gates.md), together with
[RFC 016](../../archive/016-published-crate-legal-file-completeness.md) — **withdrawn 2026-07-28**; see § M6a.

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

### M6a — Published-crate legal files — **WITHDRAWN, revert required**

> **RFC 016 was withdrawn by owner decision on 2026-07-28. Do not implement it.** Its Motivation
> claimed Apache-2.0 §4(a)/§4(d) require `LICENSE`/`NOTICE` inside each published `.crate`. That was
> **false**: §1 defines "You" as an entity *exercising permissions granted by this License*, and §4
> conditions that grant — it binds redistributors, not the copyright holder publishing their own
> work. Root-only is sufficient. See `rfcs/archive/016-published-crate-legal-file-completeness.md`.
>
> You implemented the RFC correctly as written; the RFC was wrong, not your work. Nothing about this
> revert reflects a defect in what you built.

**Task:** revert the uncommitted RFC 016 implementation, retaining one unrelated improvement.

**Change scope — remove:**

1. `crates/localcache/LICENSE`, `crates/localcache/NOTICE`, `crates/cli/LICENSE`,
   `crates/cli/NOTICE` (untracked; delete).
2. `scripts/release.py` — `verify_legal_file_mirrors`, `verify_package_legal_files`, the `package`
   subcommand added for them, the four `REQUIRED_PATHS` entries for the mirrors, and any
   `LEGAL_FILE_MIRRORS` constant.
3. `scripts/tests/test_release_runner.py` — the 11 tests added for the above.
4. `scripts/release-tools.toml` — restore the `release.py` implementation hash pin to match the
   reverted file.
5. `docs/src/source_archives.md` — the "Published-crate legal files" section.
6. `crates/localcache/Cargo.toml` — the explanatory comment about mirrors.

**Change scope — KEEP:**

7. `crates/cli/Cargo.toml`'s **`readme.workspace = true`**. This is unrelated to licensing: without
   it the CLI's crates.io page is blank. Remove only the mirror-related comment beside it, not the
   field. Confirm with `cargo package --list -p localcache-cli` that `README.md` still appears.

**Explicit non-change scope:**

- Do **not** touch the repository-root `LICENSE` or `NOTICE`. They remain the sole copies.
- Do **not** change `license = "Apache-2.0"` in `[workspace.package]`, and do **not** introduce
  `license-file`.
- Do **not** touch anything belonging to M6b, M6c, M6d, or M6e's completed items 1-6.

**Required tests:** the suite returns to its pre-M6a state and passes; `cargo package --workspace
--locked` succeeds with no new warning; `python3 scripts/release.py source --output-dir <new>`
still passes end to end (the `REQUIRED_PATHS` change touches it).

**Acceptance criteria:**

- `git status` shows no residue of the four mirrors.
- `grep -rn "verify_legal_file_mirrors\|verify_package_legal_files\|LEGAL_FILE_MIRRORS" scripts/`
  returns nothing.
- Every `[implementations]` hash pin matches its file.
- `cargo package --list -p localcache-cli` includes `README.md` and no `LICENSE`/`NOTICE`.
- Full test suite, `cargo fmt --all --check`, `source_integrity.py --require-tracked`,
  `git diff --check` all clean.

**Prohibited shortcuts:** do not leave the functions in place unused "in case"; do not leave the
mirrors untracked-but-present; do not stale-pin `release-tools.toml`.

**Required evidence:** the greps above, the `cargo package --list` output, the test count returning
to its prior value, and confirmation that a full `release.py source` run still passes.

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

### M6c — CI provenance *(RFC 009 R3–R6, R14)* — ✅ complete

> **Items 1, 2, 5, and 8 below are withdrawn by
> [RFC 017](../../accepted/017-content-reproducible-archives-without-a-container-producer.md)
> (accepted 2026-07-28), which supersedes RFC 009 R16 and retires the container producer.** They are
> struck rather than deleted so the record of what was originally required, and why it changed,
> survives. Their replacements live in § M6e. Items 3, 4, 6, 7, and 9 were delivered in `d86fda7`.

1. ~~Execute `scripts/canonical-producer.sh` end-to-end.~~ **Withdrawn (RFC 017 R5).**
2. ~~Prove two consecutive canonical builds from one commit produce an identical SHA-256.~~
   **Withdrawn** — replaced by RFC 017 R2, per-host uncompressed-tar determinism, verified in M6e.
3. Add CI archive construction plus a final aggregator that fails closed when any required row or
   artifact is missing, duplicated, stale, or bound to a different workflow run or commit.
4. Enforce R16: explicit `permissions: contents: read`, every third-party action pinned to an
   immutable commit SHA, no `pull_request_target` execution of untrusted code, no publish or
   repository secrets in build/verification jobs.
5. ~~Replace self-asserted RC eligibility with an external binding.~~ **Withdrawn** — RFC 017 R3
   removes the environmental claim entirely: `rc_eligible` derives from a clean committed tree, all
   required gates passing, and a complete evidence bundle. There is nothing environmental left to
   attest. Implemented in M6e.
6. Re-assert the R4/R5 layout **after** the artifact smoke run, so "build output stayed outside the
   extracted source" is observed rather than assumed.
7. Finalize the failure summary on unexpected exceptions — `main()` currently catches only three
   error types, so an `OSError` can leave `summary.log` reading `status: RUNNING`.
8. ~~Replace the per-binary SHA-256 pins in `[supported-host-tools]` with an explicit, non-empty
   claimed-platform policy.~~ The pins were removed in `d86fda7`; **RFC 017 R3 then removes
   `[supported-platforms]` and `[supported-host-tools]` outright**, since there is no longer a
   canonical/noncanonical distinction to police. Removal happens in M6e.
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

### M6e — RC construction and evidence *(RFC 017; RFC 009 R9, R14)*

**Do the RFC 017 migration first (items 1–6); the RC is built on top of it.**
[RFC 017](../../accepted/017-content-reproducible-archives-without-a-container-producer.md) was
accepted 2026-07-28 and supersedes RFC 009 R16. There is no container producer to execute and no
compressed-byte identity to prove; M6c's deferred canonical-producer items are **withdrawn**, not
carried here. RFC 009 **R5's archive-safety controls are unchanged** — do not touch structured header
validation, the export manifest, Git blob identity, or link rejection.

1. `release_archive.py` — compute and return the **uncompressed-tar** SHA-256; keep `compress_tar`
   for producing the deliverable.
2. `release.py` — record the uncompressed digest as the primary identity; the compressed size and
   digest stay recorded but **advisory**. Replace `rc_eligibility()`'s environment check with RFC 017
   R3: clean committed tree **and** every required gate passed **and** the evidence bundle complete
   with no skipped required step. Drop `verify_tool_manifest`'s canonical branch. Add RFC 017 R4
   toolchain-identity capture (platform, `git`, Python, zlib, locale, timezone, stable and
   declared-MSRV `rustc`/`cargo`, mdBook, security tool).
3. `release-tools.toml` — remove `[producer].image`, `[canonical-base-components]`,
   `[supported-platforms]`, `[supported-host-tools]`, and the `canonical-producer`
   `[implementations]` pin. Keep `[implementations]` for the remaining gate scripts.
4. Delete `scripts/canonical-producer.sh`.
5. `.github/workflows/ci.yaml` — the `archive` job drops `--noncanonical` and becomes the ordinary
   archive gate. Remove `RFC009_PRODUCER_IMAGE` / `RFC009_RC_ELIGIBLE` handling wherever it appears.
6. Leave no dangling reference to the wrapper in `Makefile.toml`, CI, `release-tools.toml`, or these
   handoffs.

Then the RC itself:

7. `cargo doc --workspace --no-deps --all-features --locked`, `mdbook build docs`, and
   `cargo package --workspace --locked` — no `--allow-dirty`, no `--no-verify`.
8. Joint package verification. *(RFC 016's legal-file assertion was withdrawn 2026-07-28; there is no legal-file check here.)*
9. Archive plus its uncompressed-tar digest; prove two consecutive builds from one clean commit on
   the same host produce an identical uncompressed-tar digest.
10. Evidence bundle carrying every R14 field as amended by RFC 017 R4, including the RFC 014 policy
    revision and the advisory-database revision. A required step that was skipped must render the
    summary a failure.


#### RC-2 — `release` mode must not run every gate under one toolchain

**Finding:** `release` mode cannot currently complete in a single local invocation. Under stable the
`msrv` gate fails closed by design (`verify_declared_toolchain` correctly rejects a non-1.85 rustc);
under a 1.85 override, `doc-package`'s `cargo package --workspace --locked` fails because **cargo 1.85
cannot verify interdependent workspace members** — the CLI's verification does not see the
just-packaged `localcache` sibling and resolves the published `0.20.0` instead, which requires
`rusqlite 0.40` → `libsqlite3-sys 0.38.1` → `cfg_select!`, unavailable before Rust 1.95.

**This is not a dependency-requirement problem and requires no manifest change.** The `^0` requirement
stays; the 2026-07-28 owner resolution under RFC 009 R9 stands. RFC 009 R9 anticipates this case
directly: *"If that Cargo version cannot verify the joint operation…"*. `cargo package` is release
tooling and asserts nothing about MSRV compatibility; the MSRV gate's purpose is proving the library
*compiles* at 1.85 via `cargo check`. Running both under one toolchain was RC-1's composition
assumption, and that assumption is the defect.

**Required implementation:**

1. In `release` mode, invoke the `msrv` gate under the declared toolchain explicitly — e.g.
   `rustup run 1.85.0 python3 scripts/release.py msrv …`, with the version read from
   `[workspace.package].rust-version` rather than hard-coded. Every other gate continues under the
   ambient (stable) toolchain.
2. `release` mode must fail closed if the declared toolchain is not installed — a missing toolchain is
   a failure, not a skip.
3. `msrv_mode`'s own `verify_declared_toolchain` check stays exactly as is. It is correct and is what
   made this visible.
4. Document in `docs/src/source_archives.md` that `release` runs `msrv` under the declared toolchain
   and everything else under stable, and why packaging is stable-only.

**Explicit non-change scope:** do not touch `Cargo.toml`'s `localcache` requirement; do not weaken
`verify_declared_toolchain`; do not change `msrv_mode` or `doc_package_mode` internals.

**Verify first, before implementing.** My mechanism above is inference from artifact and manifest
inspection, not from a reproduction — and my first analysis of this finding was wrong. Run:

```sh
cargo +1.85.0 package -p localcache-cli --locked --allow-dirty 2>&1 | grep -E "localcache|libsqlite3-sys"
```

If it reports resolving `localcache 0.20.0`, the mechanism holds and RC-2 is the right fix. If it
reports something else — an unresolvable requirement, or a different version — **stop and report**,
because the diagnosis is then wrong and the fix may be too.

**Required evidence:** the command above; a full `python3 scripts/release.py release --output-dir <new>`
completing end to end in one invocation under ambient stable; and confirmation that removing the 1.85
toolchain makes `release` fail rather than skip.

### Explicitly out of scope

- Any tag, push, publication, yank, or hosted release. M6 ends at a verified RC.
- `LocalFileCacheError` `#[non_exhaustive]` and a dedicated poisoning variant — 0.21.0.
- Size-driven splitting of `repository.rs`, `query.rs`, `cli/src/main.rs`, or `indexes.rs`.
- RFC 011 N-01/N-02 and the RFC 010 migration space/time documentation — opportunistic, and they
  must not expand a slice's diff.
- Changing the SPDX `license` field to `license-file`.

### Sequencing

`M6a` ∥ `M6b` → `M6c` → `M6d` → `M6e` → M7.

M6a is independent and delegable now that RFC 016 is Accepted. M6c depends on M6b's
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

- **Docker is no longer required anywhere.** RFC 017 retired the container producer; the archive
  gates run on any supported host. If a future change reintroduces a container, it may only be an
  optional execution wrapper that changes no gate semantics (RFC 017 R5).
- The two `warn` advisory dispositions carry an `expires` field; once reached the security gate
  denies. Renewing or resolving them is an owner decision that gates M6b's security step.
- RFC 016 is Accepted (2026-07-28); M6a is unblocked. No outstanding design question remains in M6. The only
  design question outstanding in the entire milestone.

## 8. Recommended next step

Implement in the §2 order, gating after each slice, and prepare a focused implementation review
request per slice that identifies the exact commit, maps each requirement to its evidence, and
discloses any gate that failed or was not run rather than omitting it.

After M6e, request the M7 independent architecture review of the RC and the extracted archive. Do
not record Phase 21 complete, move any RFC to `done/`, or perform any release action until M7 is
accepted and the owner authorizes it.
