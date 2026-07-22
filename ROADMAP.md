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

- Schedule baseline: **2026-07-17**, Asia/Tokyo.
- One primary implementer; independent architecture review is a separate gate.
- Dates are targets, not permission to bypass an exit gate.
- Non-trivial work is designed and approved in an RFC before implementation.
- Because design review and implementation are separate roles, an approved RFC
  must have a durable repository-visible Accepted state before delegation;
  ignored review records alone do not authorize implementation.
- The release version is provisionally v0.20.1 and must be confirmed before the
  release-candidate milestone under the project's version-immutability policy.

### Milestone schedule

| Milestone | Target window | Scope | Exit gate |
|---|---|---|---|
| **M0 — Plan and design** | Jul 17–22 | Approve this schedule; resolve archive-layout and canonical-producer authority; adopt a durable Accepted RFC state; draft RFCs 009–015 | Roadmap accepted; RFC review order agreed; owner decisions recorded; no implementation starts without an Accepted RFC |
| **M1 — Buildable source and archive ✅** | Jul 23–27 | Author or remove the declared benchmark coherently; create source-context and artifact-context runners; make the source archive self-buildable and safely verifiable | Current checkout and extracted archive pass their applicable RFC-defined smoke gates; exact export manifest and malicious archive fixtures pass |
| **M2 — Data integrity and SQL safety ✅** | Jul 28–Aug 5 | Preserve v1 payloads through v1-to-v5 migration; make migrations atomic; constrain and safely handle SQLite identifiers | Historical fixture and rollback tests pass; hostile identifier tests pass; focused security review accepted |
| **M3 — Mutation boundaries and input safety** | Aug 6–14 | Enforce read-only schema/mutation rules; prevent watcher privilege bypass; make glob/path/CLI handling Unicode-safe and non-panicking; align deleted-path behavior | Negative read-only and Unicode/property tests pass; public behavior matches approved RFCs |
| **M4 — MSRV and supply-chain recovery** | Aug 15–21 | Select a Rust-1.85-compatible SQLite stack or approve a new MSRV; update vulnerable dependencies; define advisory deny/warn/exception policy | Full declared-MSRV build succeeds; security policy gate is green or has approved, expiring exceptions |
| **M5 — Async and maintainability hardening** | Aug 22–28 | Remove unnecessary unsafe generic casts; unify runtime panic/poison handling; surface watcher setup failures; perform only risk-reducing module splits | Runtime-backend tests and mutex-panic tests pass; no unexplained unsafe remains; focused review accepted |
| **M6 — Release controls, docs, and RC** | Aug 29–Sep 5 | Correct CI/Makefile feature matrices; enforce warning policy; reconcile archive and published-crate legal-file rules; refresh docs/RFC final prose; assemble fresh evidence | Stable and MSRV gates, tests, clippy, docs, package/archive smoke, legal-file content, and advisory gate all pass on the RC |
| **M7 — Independent review and release decision** | Sep 8–12 | Independent architecture re-review of the RC and extracted archive | Every blocker closed; reviewer verdict **Accept** or **Accept with notes**; owner authorizes release |

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

The virtual-workspace relocation at `fe9fe88` was accepted for continued
development on 2026-07-21. Publication remains blocked until M6 supplies and
verifies the root-authoritative `LICENSE` and `NOTICE` content in each generated
`.crate` artifact. These files must never be placed in member crate directories;
the repository-root files remain the sole authoritative copies.

### RFC design queue

RFC numbers are provisional until each file is created and indexed according
to RFC 000.

| RFC | Working title | Primary review findings | Planned implementation milestone | Handoff expectation |
|---|---|---|---|---|
| **009** | Reproducible Source Archives and Release Gates | B-01, B-07 | M1, completed in M6 | Required implementation and QA handoff after acceptance |
| **[010](rfcs/accepted/010-transactional-payload-preserving-schema-migrations.md)** | Transactional, Payload-Preserving Schema Migrations | B-02 (closed by `95fd1a0`) | M2 | Implementation and fixture handoffs accepted |
| **[011](rfcs/accepted/011-safe-sqlite-identifier-boundary.md)** | Safe SQLite Identifier Boundary | B-03 (closed by `d4fe505`) | M2 | Hostile-input QA checklist accepted |
| **[012](rfcs/accepted/012-read-only-schema-and-mutation-contract.md)** | Read-only Schema and Mutation Contract | B-04 | M3 | Accepted API-boundary implementation matrix |
| **013** | Panic-free Path, Glob, and CLI Text Handling | B-05 and related path findings | M3 | Optional property-test handoff |
| **014** | MSRV and Dependency Security Policy | B-06, B-08 | M4 | Recommended dependency-verification handoff |
| **015** | Async Runtime and Watcher Failure Safety | Runtime/watcher non-blocking findings | M5 | Recommended runtime test-matrix handoff |

An implementation handoff is created only when the approved RFC still needs
non-obvious sequencing, fixture provenance, cross-runtime validation, or a
multi-developer task split. Handoffs remain companion documents under
`rfcs/handoffs/` and inherit their RFC's lifecycle state.

### Review and commit points

- **Design review 1:** roadmap and milestone acceptance (this change).
- **Design review 2:** each RFC independently; RFC 009 first, then RFCs 010
  and 011, then the remaining queue.
- **Design acceptance:** after an independent acceptance recommendation and
  explicit owner approval, move the RFC into the repository's Accepted state
  before implementation or handoff delegation.
- **Implementation review 1:** M1 buildable-source and extracted-archive proof.
- **Implementation review 2:** M2 migration-integrity and SQL-safety proof.
- **Implementation review 3:** M4 declared-MSRV and advisory-policy proof.
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
