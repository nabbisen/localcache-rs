# Roadmap

## Phases 1–15 ✅  (see CHANGELOG for details)

## Phase 16 — Documentation Overhaul (v0.16.x) ✅

- [x] `docs/book.toml` — updated repository URL, search, fold configuration
- [x] `docs/src/SUMMARY.md` — restructured into User Guide + Reference +
      Development sections with 14 chapters
- [x] `introduction.md` — feature table, quick-links, value proposition
- [x] `getting_started.md` — installation, first cache, `preload`, maintenance
- [x] `features.md` — all 7 Cargo features with code examples
- [x] `builder.md` — all `CacheEngineBuilder` options with examples
- [x] `async.md` — `AsyncCacheEngine`, `ConnectionPool`, `shared_engine`,
      SQLite concurrency model, decision table
- [x] `querying.md` — `QueryBuilder` predicates, sorting, pagination,
      `explain()`, namespace management
- [x] `watching.md` — `CacheWatcher`, `CacheDebouncedWatcher`, platform table,
      thread-ownership pattern
- [x] `portability.md` — export/import, `import_from`, `preload`, glob patterns
- [x] `cookbook.md` — 7 complete recipes (embedding pipeline, multi-threaded
      server, reactive pipeline, versioned cache, encryption, TTL, metrics)
- [x] `cli.md` — all 17 subcommands with usage examples
- [x] `errors.md` — full error variant table, recovery patterns, `anyhow` example
- [x] `change_detection.md` — all 4 modes, decision table, `explain()` usage
- [x] `api.md` — complete type catalogue, `CacheOptions` fields, `ScanOptions`
- [x] `architecture.md` — schema v4, encoding tags, write/read path diagrams,
      LRU SQL, SQLite settings
- [x] `migration.md` — bincode 1→2 upgrade guide, `payload_version` bump,
      DB migration, builder migration, schema version history
- [x] `changelog_summary.md` — phase-by-phase history from 0.1 to 0.15
- [x] `roadmap.md` — completed phases table + future directions

## Phase 17 — RFC Backlog Clearance (v0.17.0) ✅

Five pending RFCs implemented in a single release:

- [x] **RFC 001** — Recursive directory watching: `watch_dir` / `unwatch_dir`
      on both watcher types; `watch_dirs(bool)` builder flag; `contains()`
      membership filter in callbacks
- [x] **RFC 002** — Query index hints & explain plan: `QueryBuilder::index_hint`,
      `QueryBuilder::dry_run`, `AsyncCacheEngine::query_dry_run`
- [x] **RFC 003** — OpenTelemetry spans: `opentelemetry` feature
      (opentelemetry 0.32 + tracing-opentelemetry 0.33); `namespace` field
      added to all tracing spans; `check_status` promoted to `debug_span!`
- [x] **RFC 004** — Read-only shared-cache mode: `CacheOptions::shared_cache`,
      `CacheEngineBuilder::shared_cache()`; SQLite URI + `query_only` ON;
      `:memory:` shared in-process variant
- [x] **RFC 005** — async-std / smol runtime variants: `async-std` and `smol`
      features; `src/cache/runtime.rs` `SpawnBlocking` trait; precedence-based
      dispatch (Tokio > async-std > smol) for additive feature compatibility
- [x] RFC 000 lifecycle policy adopted: `rfcs/` restructured into
      `proposed/` / `done/` / `archive/` folders

## Phase 18 — Directory-scoped Query Predicates (v0.18.0) ✅

- [x] `QueryBuilder::path_in_dir(dir, recursive: bool)` — SQL-native directory
      scoping; LIKE-metacharacter-safe prefix construction with `escape_like`
- [x] `QueryBuilder::path_glob(pattern)` — brace-expanding glob in SQL via
      `expand_braces` + SQLite `GLOB`; `[` escaped to `[[]`
- [x] Shared `build_path_sql` helper + `params_from_iter` in `repository.rs`
- [x] RFC 006 authored, implemented, and moved to `rfcs/done/`

## Phase 19 — Read-only Pool and Compatibility Guarantees (v0.19.0) ✅

- [x] **RFC 007** — `ReadPool<T>`: N-slot read-only pool, `Clone+Send+Sync`,
      round-robin checkout, independent/shared-cache backends,
      full read-side API including `query_run`/`query_dry_run`
- [x] **RFC 008** — Compatibility guarantees:
      wire-format stability documented + enforced by golden fixture
      (`tests/fixtures/compat-v0_18.sqlite3`); path-semantics contract
      documented in code and docs; 9 regression tests
- [x] Release tarball structure changed to `localcache-vX.Y.Z/(files)`

## Phase 20 — Nanosecond Change Detection (v0.20.0) ✅

- [x] Store and compare file modification times at nanosecond precision
- [x] Add the schema v4-to-v5 migration
- [x] Add regression coverage for same-second, same-size overwrites
- [x] Preserve payload compatibility through the v4-to-v5 migration fixture

## Phase 21 — Stabilization and Compatibility Recovery (v0.20.1) ✅

### Goal and scope

Restore a trustworthy release baseline after the 2026-07-17 independent
architecture review returned **No-Go** for the current v0.20.0 tree.
This phase closes the review's release blockers before feature development
resumes.

In scope: source/archive buildability, migration data integrity, SQLite
identifier safety, read-only enforcement, panic-free path and glob handling,
async failure semantics, Rust 1.85 compatibility, dependency-security policy,
release gates, documentation consistency, and fresh release evidence.

Out of scope: new cache features, large-cache performance work, cross-process
shared memory, and other Future items. Those remain deferred until this phase
has an independent **Go** review.

### Planning assumptions

- Original schedule baseline: **2026-07-17**, Asia/Tokyo; remaining work was
  rebaselined on **2026-07-23** after M1–M3 completed ahead of plan.
- One primary implementer; independent architecture review is a separate gate.
- Dates are targets, not permission to bypass an exit gate.
- Target windows do not require idle time when the preceding acceptance gate
  completes early; dependency and authorization gates, not calendar dates,
  control when the next milestone may begin.
- Non-trivial work is designed and approved in an RFC before implementation.
- Because design review and implementation are separate roles, an approved RFC
  must have a durable repository-visible Accepted state before delegation;
  ignored review records alone do not authorize implementation.
- The release version is v0.20.1, confirmed by M6d coming-version housekeeping
  across `[workspace.package]`, both members, the CLI's dependency on the
  library, `CHANGELOG.md`, and every install example, under the project's
  version-immutability policy.

### Milestone schedule

| Milestone | Target window | Scope | Exit gate |
|---|---|---|---|
| **M0 — Plan and design ✅** | Completed Jul 17 | Approve the schedule; resolve initial archive-layout and canonical-producer authority; adopt a durable Accepted RFC state; establish the RFC 009–015 design queue | Roadmap accepted; RFC review order agreed; owner decisions recorded; no implementation starts without an Accepted RFC |
| **M1 — Buildable source and archive ✅** | Completed Jul 21 | Author or remove the declared benchmark coherently; create source-context and artifact-context runners; make the source archive self-buildable and safely verifiable | Current checkout and extracted archive pass their applicable RFC-defined smoke gates; exact export manifest and malicious archive fixtures pass |
| **M2 — Data integrity and SQL safety ✅** | Completed Jul 22 | Preserve v1 payloads through v1-to-v5 migration; make migrations atomic; constrain and safely handle SQLite identifiers | Historical fixture and rollback tests pass; hostile identifier tests pass; focused security review accepted |
| **M3 — Mutation boundaries and input safety ✅** | Completed Jul 23 | Enforce read-only schema/mutation rules; prevent watcher privilege bypass; make glob/path/CLI handling Unicode-safe and non-panicking; align deleted-path behavior | Negative read-only and Unicode/property tests pass; public behavior matches approved RFCs |
| **M4 — MSRV and supply-chain recovery ✅** | Completed Jul 28 | Select a Rust-1.85-compatible SQLite stack or approve a new MSRV; update vulnerable dependencies; define advisory deny/warn/exception policy | Full declared-MSRV build succeeds; security policy gate is green or has approved, expiring exceptions |
| **M5 — Async and maintainability hardening ✅** | Completed Jul 28 | Remove unnecessary unsafe generic casts; unify runtime panic/poison handling; surface watcher setup failures; perform only risk-reducing module splits | Runtime-backend tests and mutex-panic tests pass; no unexplained unsafe remains; focused review accepted |
| **M6 — Release controls, docs, and RC ✅** | Completed Jul 30 | Correct CI/Makefile feature matrices; enforce warning policy; reconcile archive and published-crate legal-file rules; refresh docs/RFC final prose; assemble fresh evidence | Stable and MSRV gates, tests, clippy, docs, package/archive smoke, and advisory gate all pass on the RC |
| **M7 — Independent review and release decision ✅** | Completed Jul 30 | Independent architecture re-review of the RC and extracted archive | Every blocker closed; reviewer verdict **Accept with notes**; owner authorized release 2026-07-30 |

#### M6 slice breakdown

M6 carries the largest remaining surface and the last open blocker (B-07), so it is decomposed
into five slices. The table gives **dependency order, not dates**; scheduling remains the owner's.
Each slice is an independent implementation review point.

| Slice | Scope | Authority | Depends on | Exit gate |
|---|---|---|---|---|
| **M6a — Published-crate legal files — WITHDRAWN ✅ resolved** | ~~Ship `LICENSE`/`NOTICE` inside both `.crate` artifacts~~ — withdrawn 2026-07-28 with RFC 016; root-only is sufficient. Residual revert completed | **RFC 016** (Withdrawn) | — | No legal-file work ships; the four mirrors are absent and the repository-root `LICENSE`/`NOTICE` remain the sole copies |
| **M6b — Canonical gate consolidation ✅** | One executable gate source of truth; `Makefile.toml`/CI become thin aliases; full package-scoped feature + doctest matrix; `-D warnings` everywhere; wire the RFC 014 security step; hash-pin `check_advisories.py` | RFC 009 R7, R12, R13; RFC 014 | — | Every gate row runs from the canonical source; **B-07 closed** |
| **M6c — CI provenance ✅** | CI archive construction and fail-closed aggregation; least-privilege permissions and immutable action SHAs; post-smoke layout re-validation; failure-summary finalization | RFC 009 R3–R6, R14 | M6b | CI aggregates by run ID and commit; layout re-asserted after the artifact smoke run |
| **M6d — Coming-version housekeeping ✅** | Set the authorized coming version across manifests, docs, and CHANGELOG; add the version-reference consistency gate; refresh docs and implemented-RFC prose | RFC 009 R10, R11 | M6b, M6c | No stale version reference; library and CLI versions match; changelog section present |
| **M6e — RC construction and evidence ✅** | Implement the RFC 017 migration (uncompressed-tar content digest, per-host determinism, gate-derived RC eligibility, retire the container wrapper); joint workspace package verification; full gate run; archive and evidence bundle | **RFC 017**; RFC 009 R9, R14 | M6b–M6d | Two same-host builds produce an identical uncompressed-tar digest; RC eligibility derives from gates, not environment; complete evidence bundle satisfying R14 as amended |

M6a is withdrawn; no legal-file work shipped. M6d required only M6b and M6c: R10 constrains version
housekeeping to precede the **final gates**, which is M6e's RC construction, and nothing in it
depended on M6a's legal files. M6e consumes all of them, and M7 begins only once M6e has produced the
evidence bundle.

**All five slices are complete and M6 is closed.** The release candidate is commit `3005ac2`, whose
project source archive has uncompressed-tar SHA-256
`46ac66b0616264ae289e089c161a51361fe8c55b67bfa5e8756b358b4e51534d`. CI run **30510083815** is green on
that exact commit — all 26 jobs, with the eight R12/R14 required jobs recorded `success` and four
evidence-binding cross-checks passing. **M7 is the only remaining milestone in Phase 21.**

**RFC 017 (accepted 2026-07-28) supersedes RFC 009 R16 and retires the container producer.** M6c's
deferred canonical-producer items are therefore withdrawn rather than carried into M6e: there is no
container to execute and no compressed-byte identity to prove. In their place M6e implements RFC
017's migration. M6c's recorded completion stands on the CI-provenance work it actually delivered;
its scope line above no longer claims producer or platform-policy work, since RFC 017 removed both.

RFC 015 may be drafted near the end of M4, but its design review and acceptance
must use the dependency and supported-toolchain baseline delivered by RFC 014.
M5 implementation still requires RFC 015 to be durably Accepted. The residual
pre-RC corrections below were required before M6 constructed the release
candidate; both are complete. M6 performed coming-version housekeeping before
constructing the RC; it did not defer housekeeping until after an actual release.

An actual tag, publication, or hosted release remains unset until M7 acceptance
and explicit owner authorization.

M1 completed on 2026-07-21 at implementation commit `e54cfe2` after focused
independent review and correction of its review record. CI archive construction,
post-smoke layout re-validation, and failure-summary finalization were deferred
to M6 and delivered by M6c; the producer-policy, externally-attested-eligibility,
and canonical-wrapper items were withdrawn by RFC 017 rather than carried
forward. M1 completion authorizes no release action.

RFC 010 implementation was independently accepted on 2026-07-21 at commit
`95fd1a0`, closing B-02. RFC 011 implementation was independently accepted on
2026-07-22 at commit `d4fe505`, closing B-03 and completing M2. This milestone
closure does not authorize a release action or move RFC 010 or RFC 011 from
`rfcs/accepted/`.

RFC 012 implementation was independently accepted on 2026-07-22 at commit
`6c14df3`, closing B-04. M3 remains open until RFC 013 and the combined
mutation/input-safety exit gate are independently accepted. This finding
closure does not authorize release work or move RFC 012 from
`rfcs/accepted/`.

RFC 013 implementation was independently accepted on 2026-07-23 at commit
`34fcc78`, closing B-05 and completing M3 together with RFC 012. The deferred
tracked-source integrity gate passed against the committed source. This
milestone closure does not authorize release work or move RFC 012 or RFC 013
from `rfcs/accepted/`.

RFC 014 implementation was independently accepted on 2026-07-28 at commit
`b5e85da`, closing B-06 and B-08 and completing M4. The declared Rust 1.85
contract is restored through `rusqlite 0.39` / `libsqlite3-sys 0.37` with zero
packages added to the graph, and a fail-closed dependency-security gate binds
RustSec results and fresh crates.io yanked state to the exact lockfile. This
closure does not authorize release work or move RFC 014 from `rfcs/accepted/`.

RFC 015 implementation was independently accepted at commit `772b3e5`,
completing M5. The library now contains zero `unsafe` blocks,
`AsyncCacheEngine` mutex poisoning returns a recoverable error instead of
panicking every subsequent caller, blocking-closure panics produce
`AsyncTaskPanicked` on all three async backends, and watcher registration
failures, dropped events, and failed invalidations are all observable. This
closure authorizes no release action and does not move RFC 015 from
`rfcs/accepted/`.

M6b was independently accepted at commit `11a8bc8`, closing **B-07** — the last
of the eight 2026-07-17 blocking findings. The R7 matrix now has one
checked-in source of truth (`scripts/feature_matrix.py`) invoked by both
`Makefile.toml` and CI, every clippy invocation denies warnings, all six gate
scripts are hash-pinned, and the R13 dependency-security step is wired with
fail-closed aggregation. B-07's fresh-evidence bullet is delivered by M6e's
R14 bundle, not by this closure. **All eight original blocking findings are
now closed**; the remaining gate to release is the M6a/M6c–M6e work plus M7,
not an open defect. This closure authorizes no release action.

M6a was **resolved as withdrawn**, not implemented. RFC 016 was withdrawn at commit `660b8a9`
after its Apache-2.0 §4 premise was found false — §4 binds redistributors exercising the granted
licence, not the copyright holder publishing their own work — so root-only `LICENSE`/`NOTICE` is
sufficient and no per-crate mirrors ship. The residual revert was independently accepted at commit
`6f5161b`, confirmed byte-identical to the pre-M6a baseline, retaining only `readme.workspace = true`
for the CLI, which is unrelated to licensing. The originating finding's "publication blocker" framing
was corrected; there was never a blocker. This resolution authorizes no release action.

M6d (coming-version housekeeping) and M6e's RFC 017 migration (items 1-6) were
independently accepted at commits `7aaa5bf`, `84fb7f2`, and `77f8b84`. The
release version is confirmed as v0.20.1 across both packages, `CHANGELOG.md`,
and every install example, enforced by a version-reference gate; the container
producer is retired, the archive's integrity identifier is now the
uncompressed-tar digest, and `rc_eligible` derives from a clean tree, passing
gates, and complete evidence rather than from any environmental claim. The
review raised one blocking finding (the CLI dependency requirement relaxed from
exact to `"0"`, contradicting RFC 009 R9); it was resolved by the 2026-07-28
owner resolution recorded under R9, which exempts workspace-internal path
dependencies and records the accepted publish-time hazard explicitly. These
closures authorize no release action.

M6e's remaining work — RC construction (items 7-10) and two reviewer corrections
— was independently accepted at commits `3ceb08d`, `257ac0a`, and `95f7d5d`,
**closing M6e and with it all of M6**. RC-1 replaced the fail-open CI
aggregation with a fail-closed required-job set and added `release` as RFC 009
R12's canonical entry point. RC-2 then scoped that entry point's toolchain
handling: only the `msrv` gate runs under the declared MSRV via `rustup run`,
resolved from `[workspace.package].rust-version` to an exact installed toolchain
name and failing closed when that resolves to zero or several. **`cargo package`
is stable-only by design**, because cargo 1.85 cannot see a just-packaged
workspace sibling and resolves the published `localcache` instead, whose
transitive `libsqlite3-sys` requires a newer compiler — a constraint RFC 009 R9
anticipates and CI already reflects in its independent job split.

**M6e closed after three release-candidate re-cuts, completing M6.** The RC
production run itself was accepted, but the first push of the phase revealed that
CI had never actually run any Phase 21 commit, and two environment-dependent
defects surfaced in sequence once it did:

- **RC-3** (`c2da67f`) — `test_toolchain_identity_returns_every_r4_field` invoked
  the real `toolchain_identity()`, which shells out to `mdbook`, `cargo`, and
  `rustc`. The `source-integrity` job installs none of them, so the test passed
  only on hosts that happened to have them. Fixed by stubbing the subprocess
  layer.
- **RC-4** (`3005ac2`) — `run_gate` merged stderr into stdout, and
  `cargo_metadata` parsed that merged stream. On a **cold** cargo cache
  `cargo metadata` writes progress to stderr, so the parse failed. Fixed with an
  opt-in `separate_stderr` that keeps stderr in the evidence log but out of the
  parsed value.

Both were invisible locally: the maintainer host has every tool installed and a
warm registry cache. Two verification techniques are now standing requirements
for changes under `scripts/` — a `PATH` stripped of `cargo`/`rustc`/`mdbook`/
`rustup`/`cargo-audit`, and a `CARGO_HOME` pointed at an empty directory.

The release candidate is **`3005ac2`**, archive uncompressed-tar SHA-256
`46ac66b0616264ae289e089c161a51361fe8c55b67bfa5e8756b358b4e51534d`, with **CI run
30510083815 green on that commit** — satisfying R14's run-ID and commit binding.
`aggregate-ci` failed closed twice on the way there, on genuinely missing
evidence rather than a synthetic case. These closures authorize no release action.

The virtual-workspace relocation at `fe9fe88` was accepted for continued
development on 2026-07-21. Its recorded legal-file publication blocker **never
existed** and was withdrawn on 2026-07-28 with RFC 016: Apache-2.0 §4 binds
redistributors, not the copyright holder publishing their own work, so the
repository-root `LICENSE` and `NOTICE` are sufficient. These files must never be
placed in member crate directories; the repository-root files remain the sole
authoritative copies.

### Residual pre-RC corrections

The following correctness items were confirmed against the newer tree while
reconciling the older 2026-07-18 architecture review. They are required before
M6 constructs the release candidate, but they are not part of RFC 015's async
runtime and watcher scope and do not require a separate RFC unless implementation
uncovers a material compatibility or design decision:

- [x] Make `explain` compare partial stored hashes using the matching partial
      hash strategy rather than comparing them with a full-file digest; cover
      both unchanged and changed partially hashed files with regression tests.
- [x] Make the CLI import contract truthful while preserving the public
      `--overwrite` spelling: implement and document distinct overwrite and
      no-overwrite behavior, and cover both paths with regression tests.
      Removing or renaming the option is a material CLI compatibility change
      and requires explicit design and owner approval before implementation.

These corrections may be implemented alongside M4 or M5 for scheduling
convenience. Their completion is recorded separately and must not broaden RFC
015. They do not create an additional review request; include their evidence in
the next meaningful implementation or pre-RC acceptance package.

### RFC design queue

RFC numbers are provisional until each file is created and indexed according
to RFC 000.

| RFC | Working title | Primary review findings | Planned implementation milestone | Handoff expectation |
|---|---|---|---|---|
| **009** | Reproducible Source Archives and Release Gates | B-01, B-07 | M1, completed in M6 | Required implementation and QA handoff after acceptance |
| **[010](rfcs/done/010-transactional-payload-preserving-schema-migrations.md)** | Transactional, Payload-Preserving Schema Migrations | B-02 (closed by `95fd1a0`) | M2 | Implementation and fixture handoffs accepted |
| **[011](rfcs/done/011-safe-sqlite-identifier-boundary.md)** | Safe SQLite Identifier Boundary | B-03 (closed by `d4fe505`) | M2 | Hostile-input QA checklist accepted |
| **[012](rfcs/done/012-read-only-schema-and-mutation-contract.md)** | Read-only Schema and Mutation Contract | B-04 (closed by `6c14df3`) | M3 | API-boundary implementation matrix accepted; no handoff required |
| **[013](rfcs/done/013-panic-free-path-glob-and-cli-text-handling.md)** | Panic-free Path, Glob, and CLI Text Handling | B-05 (closed by `34fcc78`) and related path findings | M3 | Detailed RFC matrix; handoff only if delegated |
| **[014](rfcs/done/014-declared-msrv-and-dependency-security-policy.md)** | Declared MSRV and Dependency Security Policy | B-06, B-08 (closed by `b5e85da`) | M4 | Detailed RFC matrix; handoff only if delegated |
| **[015](rfcs/done/015-async-runtime-and-watcher-failure-safety.md)** | Async Runtime and Watcher Failure Safety | Runtime/watcher non-blocking findings | M5 (accepted at `772b3e5`) | Implementation and QA handoffs accepted |
| **[016](rfcs/archive/016-published-crate-legal-file-completeness.md)** | Published Crate Legal-File Completeness | Workspace-relocation review R1 — **its Apache-2.0 premise was false; never a blocker** | M6a (withdrawn) | **Withdrawn 2026-07-28**; root-only is sufficient |

An implementation handoff is created only when the approved RFC still needs
non-obvious sequencing, fixture provenance, cross-runtime validation, or a
multi-developer task split. Handoffs remain companion documents under
`rfcs/handoffs/` and inherit their RFC's lifecycle state.

### Review and commit points

- **Roadmap review:** initial milestone acceptance and any material rebaseline.
- **Design review 2:** each RFC independently; RFC 009 first, then RFCs 010
  and 011, then the remaining queue.
- **Design acceptance:** after an independent acceptance recommendation and
  explicit owner approval, move the RFC into the repository's Accepted state
  before implementation or handoff delegation.
- **Implementation review 1:** M1 buildable-source and extracted-archive proof.
- **Implementation review 2:** M2 migration-integrity and SQL-safety proof.
- **Implementation review 3:** M4 declared-MSRV and advisory-policy proof.
- **Implementation review 4:** one combined M5 runtime/watcher and residual
  pre-RC correction proof; do not create a separate review request for the two
  residual corrections.
- **Release review:** M6 evidence bundle, followed by M7 independent review.

Each accepted RFC and each completed milestone is a separate logical commit
point unless the RFC explicitly justifies a smaller atomic sequence.

### Phase exit criteria

Phase 21 is complete only when all of the following are true:

- All eight blocking findings from the 2026-07-17 architecture review are
  closed with tests or reproducible gate evidence.
- The current checkout and extracted source archive pass the same applicable
  code, documentation, package, benchmark, and security gates; source-only
  Git provenance and archive construction run only in source context.
- Stable Rust and the declared MSRV pass the complete target/feature policy.
- Historical migration fixtures prove payload preservation from v1 and v4.
- Security scanning passes the approved deny/warn/exception policy.
- Every residual pre-RC correction is closed with regression evidence.
- Public documentation, implemented RFC prose, Cargo metadata, CI, and release
  tooling describe one consistent contract.
- A fresh evidence bundle identifies the exact commit, toolchains, commands,
  results, and archive under review.
- Independent architecture review changes the release recommendation from
  No-Go to Go, and the project owner authorizes the release.

**Status 2026-07-30: eight of nine met; the release is authorized.** M7 returned
**Accept with notes** on release candidate `3005ac2` (archive uncompressed-tar
SHA-256 `46ac66b0616264ae289e089c161a51361fe8c55b67bfa5e8756b358b4e51534d`,
CI run 30510083815 green on that commit), changing the 2026-07-17 recommendation
from No-Go to Go, and the owner authorized release the same day.

The unmet criterion is **one consistent contract**, on two documentation and CI
findings that do not affect the crate, the archive, or any published artifact:

- RFC 009 **R16 is half-retired and marked nowhere**. Its canonical-producer
  designation was superseded by RFC 017, while its CI trust-boundary clauses —
  read-only permissions, no `pull_request_target`, immutable action SHAs —
  remain fully in force. A reader arriving at R16 cannot tell which half
  applies.
- **`docs.yaml` violates R16's live action-pinning clause**: its four actions use
  mutable tags (`@v6`/`@v5`) while the workflow holds `pages: write` and
  `id-token: write`. `.github/workflows/ci.yaml` pins all five of its actions to
  commit SHAs and is compliant. This should be fixed **before** GitHub Pages is
  enabled.

Both are tracked for correction. Neither blocks the release, and the owner
accepted the notes explicitly.

### Phase 21 — released 2026-07-30

**v0.20.1 shipped.** Phase 21 is complete.

| | |
|---|---|
| Release commit | `1744378` |
| Tag | `0.20.1`, GPG-signed, pointing at the release commit |
| Release archive uncompressed-tar SHA-256 | `9a696e7423b6b4023ec31b9de27f088db5d93e5749eb2298a541c175043a3ed2` |
| CI on the release commit | run 30514015017, 26/26 green |
| Published | `localcache 0.20.1` and `localcache-cli 0.20.1` on crates.io |

The eight RFCs implemented in this phase moved from `rfcs/accepted/` to
`rfcs/done/` with Status **Implemented (0.20.1)** per RFC 000: 009, 010, 011,
012, 013, 014, 015, and 017. RFC 016 remains withdrawn in `rfcs/archive/`, its
Apache-2.0 premise having been false. `rfcs/accepted/` is now empty, which RFC
000 anticipates for a project where review and implementation are close together.

**One release-process lesson worth keeping.** `localcache-cli` was initially not
published: a bare `cargo publish` in a virtual workspace selects
`default-members`, which is `["crates/localcache"]`, so it shipped the library
and said nothing about skipping the CLI. No error, no warning. Because
`README.md` — which ships inside the published library crate — instructs
`cargo install localcache-cli`, the release briefly documented a command that
could not work. Corrected by publishing the CLI the same day.

**For future releases use `cargo publish --workspace --locked`**, which packages
and verifies every member in dependency order. Do *not* fix this by adding
`crates/cli` to `default-members`: that would make every routine `cargo build`
and `cargo test` compile the CLI binary, imposing a daily cost to guard one
per-release step, and it would replace one implicit behaviour with another.

Publication remains a separate owner-authorized action under RFC 009 R15; the
release tooling must never publish as a side effect.

## Phase 22 — Consolidation and Measurement (v0.21.0) ✅

Approved by the owner on 2026-07-30. The version target is v0.21.0 because N1 is a
breaking public-API change and cannot ride a patch release.

### Goal and scope

Phase 21 restored a trustworthy release baseline; Phase 22 pays down what it
deliberately deferred and **establishes a performance evidence base that does not
currently exist**.

In scope: the two M7 findings, the error-type contract, dependency-advisory
disposition, release-tooling hygiene, a large-namespace performance baseline, and
the module-size debt with its register corrected.

Out of scope, with reasons recorded below: performance *tuning*, cross-process
read-write shared cache, and the `#[async_test]` macro.

**This phase is mostly inward-facing.** N1's error types are the only user-visible
change. That is a deliberate choice, not an oversight — the owner accepted it after
it was raised as a concern.

### Milestones

Dependency order, not dates. Each is an independent review point.

| Milestone | Scope | Authority | Depends on |
|---|---|---|---|
| **N0 — Close M7's notes** | RFC 009 R16 supersession banner; pin `docs.yaml`'s four actions; enable GitHub Pages | M7 findings §5.1/§5.2 | — |
| **N1 — Error-type contract ✅** | `#[non_exhaustive]` on `LocalFileCacheError`; distinct poisoning variant replacing the `UnsupportedFeature` misuse; migration note | **RFC 018** (Accepted) | — |
| **N2 — Advisory dispositions ✅** | Renew, resolve, or replace the `async-std` and `bincode` dispositions | **RFC 019** (Accepted) | — |
| **N3 — Release-tooling hygiene ✅** | `command_version` stderr separation; `target_triple` from `rustc -vV`; thread real gate results into `rc_eligibility`; RFC 014 H1–H4 | recorded findings | N0 |
| **N4 — Performance baseline ✅** | Extend benchmarks to 10k/100k/1M; publish a measured profile; **no tuning** | measurement only | — |
| **N5 — Module-size debt ✅** | Risk-reducing splits only, per the corrected register below; plus RFC 011 N-01/N-02 | — | N1 |
| **N6 — Release and review ✅** | v0.21.0 gates, evidence bundle, independent re-review | owner authorization | all |

### N1 completion

RFC 018 was implemented and independently accepted at commit `1048686`.
`LocalFileCacheError` is now `#[non_exhaustive]` with a `Poisoned { resource:
&'static str }` variant; `ConnectionPool`, `AsyncCacheEngine`, `CacheWatcher`
construction, and `ReadPool` all report poisoning instead of returning
`UnsupportedFeature` or — in `ReadPool`'s case — silently recovering the stale
guard. JSON codec failures now return `Serialization`. 401 tests pass, the full
feature matrix and the declared-MSRV matrix are green, and no schema, payload wire
format, SQL, or public type signature changed.

**This is a breaking change and fixes v0.21.0 as the next version**, since an
exhaustive downstream `match` must now add a `_` arm.

Two things worth recording. `ReadPool::checkout`'s `try_lock` scan still skips a
poisoned slot exactly as it skips a busy one — only the blocking fallback reports
poisoning — so contention was not turned into an error, which was the likeliest
regression in the task. And the exhaustiveness guarantee is enforced by a
`compile_fail` doctest whose validity was confirmed by mutation: adding a `_` arm
makes the test fail, so it passes only because the match is genuinely
non-exhaustive.

### N2 completion

RFC 019 was implemented and independently accepted at commit `ae07d79`. `expires` is now
optional for `unmaintained` and `notice` dispositions and still mandatory for
`vulnerability` and `unsound`. Both live entries — `async-std 1.13.2` and
`bincode 2.0.1` — are standing dispositions with condition-based follow-ups and **no
renewal date**, so the 2026-10-21 deadline no longer exists.

The premise was verified rather than assumed: a `vulnerability` finding against a
package already accepted as `unmaintained` is denied twice over — the new finding has no
disposition, and the old entry goes stale — because the policy key includes `kind`. Nine
independent schema probes confirmed the widening did not become a hole; a misspelled
`expries` key is still rejected.

RFC 014 carries three inline amendments at the point of use, including one marking its
historical acknowledgement table as historical. That last one matters most: a normative
clause invites checking, whereas a stated fact invites belief, so an unmarked stale fact
is the more dangerous of the two.

### N3 completion

All seven parts were implemented and independently accepted at commit `47f7417`.
`command_version` no longer merges stderr into strings that are both prefix-parsed
and stored as evidence — the same defect class RC-4 fixed in `run_gate`, one layer
over. `target_triple` is now read from `rustc -vV`'s `host:` line and records
`x86_64-unknown-linux-gnu` rather than the invented `x86_64-linux`. `rc_eligible` is
computed from tracked step completion, a clean-worktree flag, and on-disk evidence
rather than three hard-coded `True` literals. On the advisory side, a knowingly
accepted vulnerability now renders as `EXCEPTION` rather than a bare `PASS`;
transient sparse-index failures retry up to three times with backoff while a 404,
a validation failure, and a size-limit breach never do; and packages excluded from
advisory coverage are enumerated in evidence — which immediately revealed that the
two silently dropped packages were the workspace's own members.

159 script tests pass, including under a `PATH` stripped of every Rust and mdBook
binary, and the live gate exits 0.

Two things worth recording. The `rc_eligible` change means drift between
`REQUIRED_SOURCE_STEPS` and the tracking calls could now write a manifest asserting
`rc_eligible: false` — which is **safer** than the old behaviour, where the same
drift would have asserted `true` and lied — and a source-inspection test turns that
drift into a test failure before it can occur. And moving `target_triple` off
`command_version` would have broken the toolchain-free CI job, because the existing
RC-3 test stubbed only `command_version`; that was caught before it shipped and
confirmed with a real restricted-`PATH` run.

### N5 completion

Both parts were implemented and independently accepted, in two commits: RFC 011's
N-01/N-02 at `10434ac`, and the module splits at `f9fe0fd`.

**N-01 changed two of three quote sites.** The third — `create_path_index` — was
declined with reasoning, and correctly: control only reaches that branch because
`resolve_schema_object` returned `None`, so there is no catalog spelling to
substitute. Independently, `full` is `"lc_user_"` plus a suffix that
`validate_new_suffix` restricts to ASCII alphanumerics and `_` **before** the quote
runs, so no `"` is representable there at all.

**The split was verified as a pure move at token level**, not by reading the diff. A
whitespace- and comment-insensitive token comparison of the old `indexes.rs` against
the new `indexes.rs` plus `indexes/tests.rs` shows the only changes are `full` →
`object.name` at exactly two sites and the `mod tests { … }` wrapper collapsing to
`mod tests;`. Line-based comparison had been misleading, because rustfmt rejoined
`set_hook`'s signature after it dedented one level.

374 tests pass with no count change anywhere, the hostile-identifier suite passes with
zero test edits, the full feature matrix is green across 17 rows, and both default and
all-features builds are clean — the last of which mattered: an unconditional
`use crate::now_secs;` compiled under `--all-features` and failed without them,
because `now_secs` is itself feature-gated. Caught before it shipped.

### N6 progress — coming-version housekeeping complete

The authorized coming version **v0.21.0** is set across every live carrier at commit
`1243e27`: `[workspace.package].version`, the three gated install examples, the
`CHANGELOG.md` heading (RC placeholder, date deliberately unset until owner
authorization), and the advisory gate's crates.io User-Agent — which was **outside**
`VERSION_REFERENCE_TARGETS` and had gone stale silently through all of Phase 22.
Changing it re-pinned `check-advisories` in `scripts/release-tools.toml`.

Everything historical was left alone: RFC `Implemented (0.20.1)` statuses, the v0.20.1
changelog section, roadmap narratives, handoff records, and the deliberate `^0` CLI
dependency requirement from the 2026-07-28 owner resolution. A blanket substitution
would have rewritten all of those into falsehoods **and still passed the version gate**,
so nothing would have caught it.

Two policy `reason` fields dropped their version reference: the claims they make —
preserve the async-std backend, preserve the bincode wire format — do not depend on
which release is next, and the `approved` date already records when each decision was
made without decaying.

Verified after commit: the full `source` gate exits 0 with all ten steps passing,
`version-contract: PASS (0.21.0)`, and the archive correctly named
`localcache-v0.21.0.tar.gz` (uncompressed
`1a11f88ccb45ec029352a4b01ddac143c6cb39a929602370547c14922459048f`). CI run
**30680708079** is green on `1243e27`, 26/26 jobs.

Remaining in N6: the RC production run, the release review, and the owner's
tag/publish/tarball actions.

### Phase 22 — released 2026-08-01

**v0.21.0 shipped.** Phase 22 is complete.

| | |
|---|---|
| Release commit | `90a2c0b` |
| Tag | `0.21.0`, GPG-signed, on the release commit |
| Release archive uncompressed-tar SHA-256 | `9630a182edd3366707504fdedbc010a014b2e63487feabb952c334038e79c937` |
| CI on the release commit | run 30684858900, 26/26 green |
| Published | `localcache 0.21.0` and `localcache-cli 0.21.0` on crates.io |

RFCs **018** and **019** moved to `rfcs/done/` with Status **Implemented (0.21.0)**.
`rfcs/accepted/` is empty again.

`cargo publish --workspace --locked` published both crates in dependency order in one
command — the correction to v0.20.1, where a bare `cargo publish` silently honoured
`default-members` and shipped only the library.

**One result worth keeping.** The v0.21.0 release-candidate archive reproduced
**byte-identically** on an unrelated host — this workstation and a GitHub runner, with
different OS, `git`, zlib, locale and timezone — with identical 213-member lists. RFC 017
R2 guarantees only per-host determinism and RFC 009's non-goals disclaim cross-
implementation byte identity, so both were conservative. This retrospectively validates
RFC 017 R1's choice of the uncompressed-tar digest as the primary identifier, and the
decision to label the compressed digest advisory: only the compressed digest differed.

**Two process findings, recorded because they should change how Phase 23 runs.**

*Release cadence was mishandled.* The project's cadence rule names logical breaking
points — an RFC resolved, a compliance process completed. Phase 22 hit four (RFC 018,
RFC 019, the N3 tooling audit, the N5 code audit) and proposed a release at none,
treating the phase as one block. Two costs followed: non-breaking work (N2, N3, N5) was
trapped behind a breaking release when it could have shipped as a patch first, and
thirteen commits accumulated with no CI verification — during which RUSTSEC-2026-0221 was
public for eighteen days before a push surfaced it.

*Phase 22 defined no exit criteria.* Phase 21 had nine and M7 assessed against each; this
phase had goal, scope, milestones and registers but no completion checklist, so its
release decision assessed against the stated goal instead. **Phase 23 must define exit
criteria before work starts.**

### Why performance *tuning* is not in this phase

The backlog item read "performance tuning for very large namespaces (> 1M
entries)." **No measurement supports it.** The benchmark suite's largest dataset is
**250 entries** — four orders of magnitude below the stated target — and the schema
carries two indexes (`idx_files_namespace_path`, `idx_files_lru`). Whether 1M
entries is slow, where, and by how much is unknown.

N4 therefore measures and publishes a profile; tuning becomes Phase 23, scoped from
real numbers.

**N4 is complete, and it overturned the hypotheses it was built to test.** Measured
on one host (`TMPDIR` on tmpfs, release profile) at 10k / 100k / 1M entries via
`crates/localcache/benches/scale_profile.rs`:

| Operation | 10k | 1M | Growth |
|---|---|---|---|
| `get`, `get_if_fresh` | ~7.0 µs | **~7.5 µs** | **O(1)** |
| `path_glob`, leading **literal** | 2.87 ms | **3.05 ms** | **O(1)** |
| `cleanup_missing_files` | 14.4 ms | 1.40 s | linear |
| LRU eviction (~450k rows) | — | 932 ms | linear, 2.07 µs/row |
| `path_in_dir` non-recursive | 3.48 ms | 46.2 ms | 13× |
| `path_glob`, leading **wildcard** | 3.31 ms | 56.3 ms | 17× |
| **`field_gt` + `order_by` + `limit 25`** | 38 ms | **4.381 s** | **115×** |

Storage is ~950 bytes per entry, so 1M entries ≈ 950 MB.

**The dominant cost — a JSON field query with sort, 4.4 s at 1M to return 25 rows —
was not predicted.** `dry_run()` shows why: no index can serve JSON field extraction
or an `ORDER BY` on one, so every row is decoded and sorted before `LIMIT` applies.
Of the original hypotheses, `cleanup_missing_files` proved real but ~3× cheaper, LRU
eviction proved a non-issue, and the leading-wildcard glob was real **only at scale**
(1.15× at 10k, 18.45× at 1M) — a single-point measurement would have dismissed it.

Point operations being O(1) across a 100× range is the most reassuring result and
retires the broadest concern.

Full findings, limitations, and Phase 23 ranking:
`.git-exclude/reviewed/architect-n4-scale-profile-findings-2026-07-30.md`. Two
limitations matter: tmpfs **understates** the I/O-bound `cleanup_missing_files`
figure, and a single namespace holds every entry, which maximises what a
`namespace=?` plan scans.

### Deferred register

Recorded findings not scheduled into a milestone. Each is tracked, none is lost.

| Item | Origin | Note |
|---|---|---|
| `ConnectionPool`'s batch methods return **one** element on lock failure, regardless of `paths.len()` | N1 review §4.1 | Latent correctness bug, pre-existing. A caller doing `paths.iter().zip(results)` silently drops every path but one. `ReadPool` now guarantees and documents one result per path; `ConnectionPool` does neither, so the two pool types disagree on a contract callers would assume is shared. Only manifests on a poisoned lock. Fix alongside future `ConnectionPool` work rather than standalone. |
| Pin the exhaustiveness `compile_fail` doctest to `E0004` | N1 review §3.1 | Optional hardening. Makes permanent the guarantee currently established only by mutation testing. |
| `follow-up` in `advisory-policy.json` is a sentence fragment that only reads correctly once the reporter prepends "reassess if" | N2 review §4.1 | Data that parses only inside one template is fragile once a second consumer appears. Prefer self-describing data and a reporter that emits it verbatim. Fold into any future touch of the reporting code. |
| `fetch_with_retry` catches a broad `AdvisoryGateError` rather than a dedicated transient type | N3 review §4.1 | Correct today, because `live_fetch`'s only failure mode is `OSError`/`URLError`. But `Fetch` is an injection point: a substitute raising `AdvisoryGateError` non-transiently would be retried three times, turning a fast failure into a slow one. Fix is a `TransientFetchError` so the fetcher declares transience rather than the wrapper inferring it. |
| RFC 011 N-01/N-02 (quote the catalog's spelling; comment the ASCII-fold invariant) | Phase 21 | Verified safe; hardening only. **Moved from N3 to N5**, since both land in `indexes.rs`, which N5 also touches. |
| `namespace_copy`'s body is byte-identical to `import_from`'s in `cache/engine/portable.rs` | N5 review §2 | Verified identical (184 chars each). Became visible once the concern was isolated in one file. Reported and deliberately **not** fixed during a move commit. |
| The exhaustiveness `compile_fail,E0004` annotation is **documentation-only** | P0 review §3.1 | **Rustdoc does not verify a `compile_fail` block's error code against the actual diagnostic.** Confirmed by mutation from both sides: a block annotated `E0004` that fails with an unresolved-path error, a type mismatch, or `E0425` all still report `ok`. The guarantee that the match is genuinely non-exhaustive rests on mutation testing at review time, not on the annotation. `trybuild` or a custom `rustc --error-format=json` harness would enforce it; neither judged proportionate for one assertion. |
| **`rusqlite` held at `^0.39`, and the conditions that would change it** | orbok/arama requests, 2026-08-01 | Not a defect to fix — a standing external constraint to **re-evaluate at each replanning**. `rusqlite 0.40` needs `libsqlite3-sys 0.38.x`, which needs **Rust 1.95** (measured: 1.94 fails, 1.95.0 passes), so adopting it would move this crate's floor from 1.85 to 1.95. Cost of holding: bundled SQLite stays at 3.51.3 rather than 3.53.2, and a consumer pinning `rusqlite 0.40` directly cannot use this crate at all — `links = "sqlite3"` makes that a hard resolution failure with no downstream remedy. **Revisit if any of:** `libsqlite3-sys` declares `rust-version` (then MSRV-aware resolution serves both audiences and no choice falls to us); the `cfg_select!` dependency goes away; 1.95 stops being a recent floor; or a consumer confirms a parallel line would unblock them. User-facing detail in `docs/src/dependency_security.md`. |
| `preload`, concurrent access, bincode codec at scale, watcher on large trees, cold-open cost | N4 §6 | Unmeasured. Candidate additions to the scale profile; none blocks Phase 23 scoping. |

### Module-size register — after N5

Production ELOC (non-blank, non-comment). **Measure production and test code
separately**: `indexes.rs`'s original 914 was mostly an embedded `#[cfg(test)]`
module, so the register previously read a *file* size as a *production* size and
ranked it second-worst when its production surface was always ~338.

| File | Before | After | Outcome |
|---|---|---|---|
| `crates/localcache/src/cache/engine.rs` | 946 | **762** | two genuine seams extracted (`engine/diagnose.rs` 128, `engine/portable.rs` 74); core CRUD surface deliberately left together |
| `crates/localcache/src/db/indexes.rs` | 914 | **338** | complies; 573 ELOC were an embedded test module, moved to `db/indexes/tests.rs` |
| `crates/cli/src/main.rs` | 728 | **257** | complies; 15 subcommand handlers split along the existing `DatabaseAuthority` boundary into `commands/read.rs` (294) and `commands/write.rs` (193) |
| `crates/localcache/src/db/repository.rs` | 618 | 618 | **reasoned refusal** — ~28 free functions over `&Connection` sharing SQL-construction helpers; any split scatters coupled query building rather than isolating a concern |
| `crates/localcache/src/db/schema/classifier.rs` | 586 | 586 | **reasoned refusal** — one DDL-tokenising and validating pipeline over shared types; tests already external, so no measurement artefact to recover |
| `crates/localcache/src/cache/query.rs` | 463 | 463 | complies; removed from the register |

`engine.rs` remains above the 500 guidance and that is accepted deliberately. The
guidance exists to prompt the question, not to be satisfied at any cost; two real
seams were found and taken, and the remainder all reads or writes the same
`Connection` and fields.

### Recorded conflict of interest

N1's RFC would be authored and reviewed by the same high-capability model, with no
separation — the same structural conflict recorded against RFC 017 at M7 §6. The
owner is aware; how it is handled is an open decision.

## Phase 23 — Measured Performance and Consolidation (target: v0.21.1, then v0.22.0) 🚧

Approved by the owner on 2026-08-01. **The first phase scoped from measurement rather than
intuition** — N4's profile in Phase 22 overturned two of the three hypotheses it was built to
test, so Phase 23 starts from numbers.

### Exit criteria — defined before the work

Phase 23 is complete when all of the following hold:

1. Every performance finding in N4's profile is either fixed and re-measured, or explicitly
   deferred with a recorded reason. None is left unaddressed and unexplained.
2. Re-measurement uses the **same harness at the same three scales** (10k / 100k / 1M), so
   before/after numbers are comparable.
3. `cleanup_missing_files` has been measured on **real storage**, not tmpfs — the current
   figure is a floor and we know it.
4. Every deferred-register item is closed, re-registered with a reason, or scheduled.
5. Public documentation states the measured characteristics and the query pattern to avoid.
6. **Each release ships at its own breaking point, with CI green before the next milestone
   starts.**
7. No release action occurs without owner authorization.

Criterion 6 is Phase 22's cadence failure written as a gate rather than an intention: four
breaking points passed there with no release proposed, and thirteen commits accumulated
unverified.

### Version plan — non-breaking work ships first

Phase 22 sequenced its breaking milestone first and trapped three non-breaking milestones
behind it. Phase 23 inverts that.

| Release | Contents | Breaking? |
|---|---|---|
| **v0.21.1** | P0 — query documentation, the `ConnectionPool` batch fix, tooling hygiene | no |
| **v0.22.0** | P1 — JSON field query performance, only if it needs new public API | additive at most |

If P1 needs no new API there may be no v0.22.0, which is a good outcome rather than a
shortfall.

### Milestones

| Milestone | Scope | Authority | Depends on |
|---|---|---|---|
| **P0a — Query guidance ✅** | Document the leading-literal glob rule where users meet it — `docs/src/querying.md`, `path_glob` rustdoc, `QueryBuilder` docs. Only `performance.md` says it today | — | — |
| **P0b — `ConnectionPool` batch length ✅** | Fix `batch_get`/`batch_get_fresh`/`check_status_batch` returning one element on lock failure regardless of `paths.len()`; fold in the `namespace_copy`/`import_from` duplication | recorded findings | — |
| **P0c — Tooling hygiene ✅** | `TransientFetchError`; self-describing `follow-up`; pin the exhaustiveness doctest to `E0004` | recorded findings | — |
| **P0e — Async test deduplication ✅** | Collapse `pool_observe.rs`'s three runtime modules with a `macro_rules!` helper. **Not a proc-macro** — see below | — | — |
| **P0d — Release v0.21.1 ✅** | gates, evidence, publish | owner | P0a–P0c, P0e |
| **P1a — Real-storage measurement** | Re-run the scale profile with `TMPDIR` on real storage; add `preload`, concurrent access, bincode-at-scale, cold-open | — | — |
| **P1b — JSON query design** | **New RFC** | RFC required | P1a |
| **P1c — Implementation** | per the accepted RFC | that RFC | P1b |
| **P1d — Re-measure** | same harness, same scales; before/after table | — | P1c |
| **P1e — Release** | only if P1c changed public API | owner | P1a–P1d |

### P0a/b/c/e completion

Implemented and independently accepted at commit `cd209e1`, ten files. The glob
leading-literal rule now sits in `path_glob`'s rustdoc where a user writing the
pattern will see it, not only in `performance.md`. `ConnectionPool`'s three batch
methods return one result per requested path on lock failure instead of a single
element — a latent correctness bug where `paths.iter().zip(results)` silently
dropped every path but one. `namespace_copy` delegates to `import_from` rather
than repeating a byte-identical body. A dedicated `TransientFetchError` means the
fetcher declares transience instead of the retry wrapper inferring it from a broad
exception type. Advisory `follow-up` values are complete sentences emitted
verbatim. And `pool_observe.rs`'s triplicated panic test is generated from one
macro body.

375 tests pass, 162 script tests pass including under a toolchain-free `PATH`, the
full feature matrix and the declared-MSRV matrix are green, and the live advisory
gate is unchanged at `denied=0`.

**Two judgement calls were reported rather than absorbed, and both were right.**
`poisoned_mutex_recovers_on_subsequent_calls` was left un-deduplicated: Tokio's
version asserts a third subsequent call that async-std's and smol's do not, so
unifying it would have either dropped coverage or silently added untested
assertions. And the C1 boundary deliberately leaves 5xx status inspection outside
the new error type, because that branch reads a field from a *successful* return
rather than catching an exception — moving it into `live_fetch` would let each
implementation disagree about what a 5xx means.

### P0d progress — coming-version housekeeping complete

v0.21.1 is set across every live carrier at commit `f41ea4e`: `[workspace.package].version`,
the three gated install examples, the `CHANGELOG.md` heading (RC placeholder, date unset until
owner authorization), and the advisory gate's crates.io User-Agent — which required re-pinning
`check-advisories` in `scripts/release-tools.toml`.

Fourteen files still contain `0.21.0` and all fourteen are correct history: the shipped
changelog section, RFC 018/019 `Implemented (0.21.0)` statuses, the RFC index, Phase 22
narratives, two `docs/src` migration headings whose subject genuinely is the 0.20.x→0.21.0
upgrade, and the handoff records — including the P0d handoff itself, which quotes the values it
instructs changing. A blanket substitution would have rewritten all of them into falsehoods
**and still passed the version gate**.

Verified after commit: the full `source` gate exits 0 with all ten steps passing,
`version-contract: PASS (0.21.1)`, archive `localcache-v0.21.1.tar.gz` (uncompressed
`fc6c28a6a362fee09433dee3bedb590955e3e01f5cd93e65a9e2d88b14a13c4c`). CI run **30698532496** is
green on `f41ea4e`, 26/26 jobs — the first CI run covering all of Phase 23 P0.

Remaining for the release: the RC production run, the release decision, and the owner's
tag/publish.

### P0 complete — v0.21.1 released 2026-08-01

| | |
|---|---|
| Release commit | `a4b1f90` |
| Tag | `0.21.1`, GPG-signed, on the release commit |
| Release archive uncompressed-tar SHA-256 | `16cfddfa43ba0f5be32e33b02d384be387cefa2246032bf06d163936dc0f3b0a` |
| CI on the release commit | run 30701243893, 26/26 green — confirmed **before** tagging |
| Published | `localcache 0.21.1` and `localcache-cli 0.21.1` on crates.io |

Verified after publication: a fresh consumer declaring `localcache = "0.21.1"` and
`rust-version = "1.85"` resolves `libsqlite3-sys 0.37.0` and builds on rustc 1.85.0, so the
declared MSRV holds for the shipped release rather than only for this workspace.

**This is the first release under the corrected cadence rule.** P0's non-breaking work shipped
as a patch rather than waiting behind a breaking change — the inversion the v0.21.0 release
decision identified as Phase 22's structural error. The release decision was **Accept** with no
notes, the first without qualification since the cadence and contract findings were opened.

Two process points recorded from the milestone, neither affecting the artifact:

- The RC was first produced on an **unpushed** commit, because a ROADMAP record was committed
  between "push and confirm CI" and "produce the RC". Resolved by pushing and re-confirming
  before the decision. The working ordering is: make every commit, push, confirm CI, **then**
  produce the RC — stated as a sequence rather than a list item, because a list item gets
  overtaken.
- The § "Bundle retention" rules were not applied on their first outing; the bundle was 686 MB
  of which 664 MB was build output. Applied during review after verifying both `.crate` digests
  against the manifest: 686 MB → 23 MB, and the superseded v0.21.0 bundle removed.

**Phase 23 now moves to P1a** — re-measuring the scale profile on real storage, since the
current `cleanup_missing_files` figure was taken on tmpfs and is a floor rather than an estimate.

### Why P0e is a `macro_rules!` helper, not `#[async_test]`

The backlog item read "`#[async_test]` proc-macro wrapper for unified async test authoring
across runtime backends". Measured, the duplication it would remove is **one file**:
`pool_observe.rs`'s three runtime modules, 295 lines, of which only **two test functions**
plus a `block_on` helper are genuinely triplicated. The suite's other 29 async test
functions are single-runtime and would not benefit.

A proc-macro costs a new workspace crate — proc-macros cannot live in an existing one —
plus `syn`, `quote`, and `proc-macro2`: three more crates under advisory watch, each
required to hold at MSRV 1.85, and `syn` moves quickly. That is a permanent maintenance
commitment for a 295-line saving.

A `macro_rules!` helper in a test-support module generates the same three modules with
**no new crate and no new dependency**. It loses the attribute spelling — `async_test! { … }`
rather than `#[async_test]` — which is cosmetic for two tests.

**Owner decision, 2026-08-01: deduplication accepted, proc-macro not pursued.** If the
attribute form is ever wanted, it should be an unpublished crate (`publish = false`, path
dev-dependency) so it never becomes a third release artifact, and that must be verified
against `cargo publish` rather than assumed — published manifests do carry
dev-dependencies.

### The dominant finding, and why it is not simply "add an index"

`field_gt` + `order_by_field` + `limit 25` costs **4.4 s at 1M rows to return 25 rows**,
linearly. `dry_run()` shows the plan narrowing on `namespace=?` only, so every row's JSON is
decoded and sorted before `LIMIT` applies — a small limit saves nothing.

A SQLite expression index over `json_extract(payload, '$.field')` is the obvious answer, and
`create_path_index`/`drop_path_index`/`list_path_indexes` already establish the user-declared
index pattern with RFC 011's ownership validation and `QuotedIdentifier` boundary.

**But `json_extract` requires the payload to be literally JSON on disk**, and payloads may be
bincode-encoded, compressed, or AES-256-GCM encrypted. An expression index over an encrypted
BLOB is meaningless. The design questions are therefore which payload configurations can
support a field index, what happens when a user enables compression afterwards, whether the
index is user-declared or inferred, and what the API says when it cannot help.

**Documenting the ceiling is a legitimate outcome** if the design cost exceeds the benefit.
N4 exists so that call rests on evidence.

### RFC authorship and review

P1b's RFC is **authored by the high-capability model and reviewed by the owner** — owner
decision, 2026-08-01, the same arrangement that worked for RFC 018, where the owner's ruling
on R4 overruled the author and produced the strongest change in v0.21.0. Recorded here so it
is in the RFC when written, rather than appended at review time.

### Out of scope

- **Cross-process read-write shared cache** — still blocked on a use case from the owner;
  multi-reader/one-writer and symmetric multi-writer are different designs.
- **Performance work beyond N4's profile** — anything unmeasured waits for P1a.

## Future / Unscheduled

*(all items from the previous Future section shipped in v0.17.0)*

- **Performance tuning for very large namespaces (> 1M entries)** — deferred to
  Phase 23 pending N4's measured profile. Do not scope as tuning before then.
- **Cross-process shared-cache via named shared memory (beyond RFC 004 scope)** —
  deferred. RFC 004 delivered read-only shared memory; "beyond" means cross-process
  read-write, i.e. write coordination and lock contention layered directly on the
  data-integrity guarantees Phase 21 just restored, while poison handling is still
  deferred debt. **Blocked on a stated use case from the owner**, since
  multi-reader/one-writer and symmetric multi-writer are different designs.
- **`#[async_test]` proc-macro wrapper (deferred from RFC 005)** — deferred. A
  proc-macro needs its own crate, making a **third publishable workspace member**:
  a third release surface and a third thing a publish step can silently skip. v0.20.1
  shipped `localcache-cli` late for exactly that reason (a bare `cargo publish`
  honours `default-members`). Internal test ergonomics do not justify that risk yet;
  revisit once `cargo publish --workspace` has been proven on a release.
