# Roadmap

See the live
[ROADMAP.md](https://github.com/nabbisen/localcache-rs/blob/main/ROADMAP.md)
on GitHub for the full backlog with implementation notes.

## Completed phases

| Phase | Version | Theme |
|---|---|---|
| 1 | 0.1 | Foundation — SQLite, bincode, BLAKE3 |
| 2 | 0.2 | Ergonomics — namespaces, batch ops, TTL |
| 3 | 0.3 | Performance — partial hash, streaming |
| 4 | 0.4 | Async & ecosystem — `AsyncCacheEngine`, zstd |
| 5 | 0.5 | Polish — JSON codec, `max_entries`, glob scan |
| 6 | 0.6 | Security — AES-256-GCM, true LRU |
| 7 | 0.7 | Operational — builder API, `cache_stats` |
| 8 | 0.8 | Workspace — CLI tool, `on_evict` |
| 9 | 0.9 | Portability — export / import |
| 10 | 0.10 | Queries — `QueryBuilder`, `contains`, `keys` |
| 11 | 0.11 | Query sorting — multi-column, `offset`, indexes |
| 12 | 0.12 | Release readiness — benchmarks, `ConnectionPool` (renamed `SyncCacheEngine` in 0.21.5) |
| 13 | 0.13 | Observability — `tracing`, `explain()`, DX |
| 14 | 0.14 | File watching — `CacheWatcher`, `preload()` |
| 15 | 0.15 | Production hardening — `metrics`, debounce, namespaces |
| 16 | 0.16 | Documentation overhaul — 18-chapter mdBook |
| 17 | 0.17 | RFC backlog — watching dirs, index hints, OTel, shared cache, async-std/smol |
| 18 | 0.18 | Directory-scoped query predicates — `path_in_dir`, `path_glob` |
| 19 | 0.19 | Read-only pool + compatibility guarantees — `ReadPool<T>`, golden fixture |
| 20 | 0.20 | Nanosecond mtime precision — schema v5 and overwrite regression coverage |
| 21 | 0.20.1 | Stabilization — eight release blockers closed, reproducible source archives, release gates |
| 22 | 0.21.0 | Consolidation and measurement — truthful error taxonomy, first at-scale profile |
| 23 | 0.21.1–0.21.3 | Measured performance — maintenance-delete batching, query execution redesign |

## Phase 24 — Correctness, Contracts, and API Consistency (in progress)

**Authorized by the owner 2026-09-23**, immediately after Phase 23 closed. RFC 022 (accepted the
same day, later amended) drove the first sub-phase, **v0.21.4, released 2026-09-23**: a patch release fixing six
reproduced correctness defects found and reproduced during Phase 24's review — key rotation, query
`offset`, `max_entries` eviction, partial-hash change detection, `AsyncCacheEngine` batch results,
watcher journal-mode inheritance — plus release-tooling repair and a documentation/records
reconciliation pass (this page included). **No MSRV change, and no behaviour newly rejected as an
error, shipped in this patch** — see [Error Handling](./errors.md) and the compatibility notes
throughout this book for what each fix actually changes.

Three further releases are planned: v0.21.5 (API consistency — additive names and deprecations
only, no removals — plus two TTL correctness fixes), v0.22.0 (the first breaking release since
v0.21.0: completing the error taxonomy, validating configuration that is silently accepted today,
and removing what v0.21.5 deprecates), and v0.23.0 (a true least-recently-used eviction policy on a
schema change). See the live
[ROADMAP.md](https://github.com/nabbisen/localcache-rs/blob/main/ROADMAP.md) for the milestone
table and RFC mapping.

## Phase 23 — Measured Performance and Consolidation ✅

**Released as v0.21.1, v0.21.2, and v0.21.3.** The first phase scoped from measurement rather than
intuition: Phase 22's profile (N4) overturned two of its own three hypotheses, so Phase 23 started
from numbers instead. Three non-breaking patches shipped, each at its own breaking point with CI
green before the next started:

- **v0.21.1** — query documentation, a `ConnectionPool` (now `SyncCacheEngine`) batch-result fix, tooling hygiene.
- **v0.21.2** — maintenance-delete batching (`cleanup_missing_files`/`cleanup_expired` page their
  scan and batch each page's deletes in one transaction).
- **v0.21.3** — query execution redesign: one streaming query replaces the old per-row `SELECT`
  pair, and payloads are decoded only for rows surviving `offset`/`limit`.

Twice, the initially recorded remedy targeted a minority of the measured cost — batching the
maintenance `stat` calls was 11.7% of the cost, and the query-execution work first estimated the
same way was corrected by measuring before committing to a design. See
[Performance and Capacity](./performance.md) for the current numbers.

## Phase 22 — Consolidation and Measurement ✅

**Released 2026-08-01 as v0.21.0.** A consolidation release paying down what Phase 21
deferred, plus the first real measurement of how the cache behaves at scale.

**Breaking:** `LocalFileCacheError` is now `#[non_exhaustive]`, so exhaustive `match`
expressions need a `_` arm. Lock poisoning returns a dedicated `Poisoned` variant, and
JSON codec failures return `Serialization` — both previously `UnsupportedFeature`. A
`ReadPool` whose connection is poisoned now reports it instead of silently handing back
state another thread abandoned mid-panic. No schema, payload wire format, SQL, or method
signature changed; existing databases open unchanged. See
[Error Handling](./errors.md) for the migration note.

Also in this release: standing dispositions for unmaintained dependencies, seven
release-tooling fixes, identifier-quoting hardening, module splits along existing seams,
and an unsound advisory in a transitive dependency resolved by taking the upstream patch.

**The measurement work produced the most useful result.** Point lookups turn out to be
**O(1)** from ten thousand entries to a million (~10 µs throughout), and a `path_glob`
with a leading literal is equally flat. The real cost is a JSON field query with a sort —
just over **2 seconds per million entries** (reduced by Phase 23's query-execution work
from the original ~3.9 s), because no index can serve an `ORDER BY` on a JSON field. See
[Performance and Capacity](./performance.md) for the full, current profile and what to do
about it.

## Phase 21 — Stabilization and Compatibility Recovery ✅

**Released 2026-07-30 as v0.20.1.** Phase 21 closed the findings from the
2026-07-17 independent architecture review, which had returned **No-Go** on
v0.20.0. The M7 re-review returned **Accept with notes**, changing the
recommendation to **Go**, and both `localcache 0.20.1` and
`localcache-cli 0.20.1` are published.

All eight of the phase's RFCs — 009, 010, 011, 012, 013, 014, 015, and 017 — are
implemented and now live under `rfcs/done/`. RFC 016 was withdrawn; its
Apache-2.0 premise was false, and the repository-root `LICENSE` and `NOTICE`
remain the sole copies.

| Milestone | Target | Outcome |
|---|---|---|
| M0 ✅ | Completed Jul 17 | Roadmap and RFC 009–015 design queue established |
| M1 ✅ | Completed Jul 21 | Current source and extracted release archive are buildable |
| M2 ✅ | Completed Jul 22 | Historical migrations preserve payloads and SQLite identifiers are safe |
| M3 ✅ | Completed Jul 23 | Read-only boundaries and Unicode/path handling are non-bypassable and non-panicking |
| M4 ✅ | Completed Jul 28 | Declared MSRV and dependency-security policy are verified |
| M5 ✅ | Completed Jul 28 | Async/watcher failure handling and highest-risk maintainability debt are addressed |
| M6 ✅ | Completed Jul 30 | CI, documentation, release gates, and fresh RC evidence agree |
| M7 ✅ | Completed Jul 30 | Independent architecture review and owner release decision |

Two bounded residual corrections—partial-hash `explain` comparison and the
CLI import overwrite contract—are complete with regression evidence. They do not
broaden the async/watcher RFC or create separate review gates.

M1 completed on 2026-07-21 at implementation commit `e54cfe2` after focused
independent review and correction of its review record. Archive verification in
CI and the remaining release-control hardening are explicitly deferred to M6;
M1 completion is not release authorization.

RFC 010 implementation was independently accepted on 2026-07-21 at commit
`95fd1a0`, closing B-02. RFC 011 implementation was independently accepted on
2026-07-22 at commit `d4fe505`, closing B-03 and completing M2. Both RFCs stayed
under `rfcs/accepted/` until the implementation shipped; that milestone closure
was not release authorization.

RFC 012 implementation was independently accepted on 2026-07-22 at commit
`6c14df3`. RFC 013 implementation was independently accepted on 2026-07-23 at
commit `34fcc78`, completing M3. Both stayed under `rfcs/accepted/` until the
implementation shipped; that milestone closure was not release authorization.

RFC 014 implementation was independently accepted on 2026-07-28 at commit
`b5e85da`, closing B-06 and B-08 and completing M4. RFC 015 implementation
was independently accepted at commit `772b3e5`, completing M5. Both stayed
under `rfcs/accepted/`; neither closure authorized release work.

M6b (canonical gate consolidation) was independently accepted at commit
`11a8bc8`, closing **B-07** — the last of the eight original blocking
findings. **All eight are now closed.** This closure authorizes no release
action.

M6c (CI provenance) was independently accepted at commit `d86fda7`. Its two
canonical-producer proof items were withdrawn rather than carried forward:
[RFC 017](https://github.com/nabbisen/localcache-rs/blob/main/rfcs/done/017-content-reproducible-archives-without-a-container-producer.md)
(accepted 2026-07-28) supersedes RFC 009's canonical-producer requirement (R16)
entirely, replacing compressed-byte reproducibility in a pinned Docker image
with content reproducibility: an uncompressed-tar digest, per-host
determinism, and RC eligibility that derives from gates passing rather than
from which machine ran them. This closure authorizes no release action.

M6a was resolved as withdrawn rather than implemented. RFC 016 required per-crate
`LICENSE`/`NOTICE` copies on the stated grounds that Apache-2.0 mandates them inside each published
crate; that reading was wrong, since those conditions bind redistributors rather than the copyright
holder. The repository-root files remain the sole copies and no per-crate copies ship.

M6d (coming-version housekeeping) and M6e's RFC 017 migration were
independently accepted at commits `7aaa5bf`, `84fb7f2`, and `77f8b84`. v0.20.1
is confirmed across both packages, the changelog, and every install example and
is enforced by a version-reference gate. The archive's integrity identifier is
now the uncompressed-tar digest, and release-candidate eligibility derives from
a clean tree, passing gates, and complete evidence rather than from which
machine ran them. These closures authorize no release action.

M6e's release-candidate tooling and two reviewer corrections were independently
accepted at commits `3ceb08d`, `257ac0a`, and `95f7d5d`. CI aggregation now
fails closed when a required job is missing, and `release` is the canonical
entry point that runs the source, MSRV, documentation/package, and security
gates in one invocation. Only the MSRV gate runs under the declared minimum
toolchain; `cargo package` runs under stable by design, because cargo 1.85
cannot see a just-packaged workspace sibling and would resolve the published
`localcache` instead.

**M6 is complete.** The release candidate is commit `3005ac2`, whose project
source archive has uncompressed-tar SHA-256 `46ac66b0…`, verified by a green CI
run on that exact commit — all jobs, including the full feature matrix, the
declared MSRV, and the advisory gate.

Closing M6e took three release-candidate re-cuts. The first push of the phase
revealed that CI had never run any Phase 21 commit, and two
environment-dependent defects surfaced once it did: a unit test that required
`mdbook`/`cargo`/`rustc` in a job that installs none of them, and a
`cargo metadata` parse that broke when a cold cargo cache made cargo write
progress to stderr. Both were invisible on a maintainer host with every tool
installed and a warm cache. A restricted `PATH` and an empty `CARGO_HOME` are now
standing verification requirements for release-tooling changes.

These closures authorize no release action; M7 (the independent architecture review and owner
release decision) is what did — see above.

The virtual-workspace relocation at `fe9fe88` was accepted for continued
development. Its recorded legal-file publication blocker never existed and was
withdrawn with RFC 016; the repository-root `LICENSE` and `NOTICE` are
sufficient and remain the sole copies, never placed in member crate
directories.

The detailed scope, RFC mapping, dependencies, and exit gates are maintained in
the repository-root
[ROADMAP.md](https://github.com/nabbisen/localcache-rs/blob/main/ROADMAP.md).
Dates are targets; no milestone is complete until its exit gate passes.

## Future / unscheduled

- **Cross-process shared-cache via named shared memory (beyond RFC 004 scope)** — deferred,
  blocked on a stated use case from the owner. RFC 004 delivered read-only shared memory;
  cross-process read-write is a different design (multi-reader/one-writer and symmetric
  multi-writer are not the same problem).

Two items previously listed here are resolved: performance tuning for very large namespaces was
measured and largely addressed in Phase 23 (see [Performance and Capacity](./performance.md)), and
the `#[async_test]` proc-macro wrapper was evaluated and **not pursued** — a `macro_rules!` helper
removed the only real test duplication without a new crate or dependency.
