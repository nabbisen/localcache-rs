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

## Phase 21 — Stabilization and Compatibility Recovery (target: v0.20.1) 🚧

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
| **M6 — Release controls, docs, and RC** | Aug 13–26 | Correct CI/Makefile feature matrices; enforce warning policy; reconcile archive and published-crate legal-file rules; refresh docs/RFC final prose; assemble fresh evidence | Stable and MSRV gates, tests, clippy, docs, package/archive smoke, legal-file content, and advisory gate all pass on the RC |
| **M7 — Independent review and release decision** | Aug 27–Sep 4 | Independent architecture re-review of the RC and extracted archive | Every blocker closed; reviewer verdict **Accept** or **Accept with notes**; owner authorizes release |

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
| **M6e — RC construction and evidence** *(RFC 017 migration, items 1–6 ✅)* | Implement the RFC 017 migration (uncompressed-tar content digest, per-host determinism, gate-derived RC eligibility, retire the container wrapper); joint workspace package verification; full gate run; archive and evidence bundle | **RFC 017**; RFC 009 R9, R14 | M6b–M6d | Two same-host builds produce an identical uncompressed-tar digest; RC eligibility derives from gates, not environment; complete evidence bundle satisfying R14 as amended |

M6a is withdrawn; its only residual work is reverting the uncommitted implementation. **M6e no longer depends on it.** M6d requires
only M6b and M6c: R10 constrains version housekeeping to precede the **final gates**, which is M6e's
RC construction, and nothing in it depends on M6a's legal files. M6e consumes all of them, and M7
begins only after M6e produces the evidence bundle.

**Currently delegable without any further owner decision:** M6d in full, and M6e's RFC 017 migration
(handoff § M6e items 1–6), which touches only the release scripts and CI. M6e's RC construction
(items 7–10) additionally requires M6a, which is blocked on the RFC 016 decision.

**RFC 017 (accepted 2026-07-28) supersedes RFC 009 R16 and retires the container producer.** M6c's
deferred canonical-producer items are therefore withdrawn rather than carried into M6e: there is no
container to execute and no compressed-byte identity to prove. In their place M6e implements RFC
017's migration. M6c's recorded completion stands on the CI-provenance work it actually delivered;
its scope line above no longer claims producer or platform-policy work, since RFC 017 removed both.

RFC 015 may be drafted near the end of M4, but its design review and acceptance
must use the dependency and supported-toolchain baseline delivered by RFC 014.
M5 implementation still requires RFC 015 to be durably Accepted. The residual
pre-RC corrections below must be complete by **Aug 12**, before M6 begins RC
construction. M6 performs coming-version housekeeping before it constructs or
reviews the RC; it does not defer housekeeping until after an actual release.

The M7 window includes independent-review availability and corrective-review
buffer. It is not a promised release date. An actual tag, publication, or
hosted release remains unset until M7 acceptance and explicit owner
authorization.

M1 completed on 2026-07-21 at implementation commit `e54cfe2` after focused
independent review and correction of its review record. CI archive construction,
portable noncanonical producer policy, externally attested RC eligibility,
post-smoke layout re-validation, failure-summary finalization, and direct
canonical-wrapper execution remain M6 work. M1 completion authorizes no release
action.

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
dependencies and records the accepted publish-time hazard explicitly. M6e's
remaining RC-construction items (7-10) require M6a. These closures authorize no
release action.

The virtual-workspace relocation at `fe9fe88` was accepted for continued
development on 2026-07-21. Publication remains blocked until M6 supplies and
verifies the root-authoritative `LICENSE` and `NOTICE` content in each generated
`.crate` artifact. These files must never be placed in member crate directories;
the repository-root files remain the sole authoritative copies.

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
| **[010](rfcs/accepted/010-transactional-payload-preserving-schema-migrations.md)** | Transactional, Payload-Preserving Schema Migrations | B-02 (closed by `95fd1a0`) | M2 | Implementation and fixture handoffs accepted |
| **[011](rfcs/accepted/011-safe-sqlite-identifier-boundary.md)** | Safe SQLite Identifier Boundary | B-03 (closed by `d4fe505`) | M2 | Hostile-input QA checklist accepted |
| **[012](rfcs/accepted/012-read-only-schema-and-mutation-contract.md)** | Read-only Schema and Mutation Contract | B-04 (closed by `6c14df3`) | M3 | API-boundary implementation matrix accepted; no handoff required |
| **[013](rfcs/accepted/013-panic-free-path-glob-and-cli-text-handling.md)** | Panic-free Path, Glob, and CLI Text Handling | B-05 (closed by `34fcc78`) and related path findings | M3 | Detailed RFC matrix; handoff only if delegated |
| **[014](rfcs/accepted/014-declared-msrv-and-dependency-security-policy.md)** | Declared MSRV and Dependency Security Policy | B-06, B-08 (closed by `b5e85da`) | M4 | Detailed RFC matrix; handoff only if delegated |
| **[015](rfcs/accepted/015-async-runtime-and-watcher-failure-safety.md)** | Async Runtime and Watcher Failure Safety | Runtime/watcher non-blocking findings | M5 (accepted at `772b3e5`) | Implementation and QA handoffs accepted |
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

## Future / Unscheduled

*(all items from the previous Future section shipped in v0.17.0)*

- Performance tuning for very large namespaces (> 1M entries)
- Cross-process shared-cache via named shared memory (beyond RFC 004 scope)
- `#[async_test]` proc-macro wrapper for unified async test authoring across
  runtime backends (deferred from RFC 005)
