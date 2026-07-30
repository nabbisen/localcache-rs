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
| 12 | 0.12 | Release readiness — benchmarks, `ConnectionPool` |
| 13 | 0.13 | Observability — `tracing`, `explain()`, DX |
| 14 | 0.14 | File watching — `CacheWatcher`, `preload()` |
| 15 | 0.15 | Production hardening — `metrics`, debounce, namespaces |
| 16 | 0.16 | Documentation overhaul — 18-chapter mdBook |
| 17 | 0.17 | RFC backlog — watching dirs, index hints, OTel, shared cache, async-std/smol |
| 18 | 0.18 | Directory-scoped query predicates — `path_in_dir`, `path_glob` |
| 19 | 0.19 | Read-only pool + compatibility guarantees — `ReadPool<T>`, golden fixture |
| 20 | 0.20 | Nanosecond mtime precision — schema v5 and overwrite regression coverage |

## Active stabilization schedule

Phase 21 targets a corrective v0.20.1 release. It closes the findings from
the 2026-07-17 independent architecture review before new feature work resumes.

| Milestone | Target | Outcome |
|---|---|---|
| M0 ✅ | Completed Jul 17 | Roadmap and RFC 009–015 design queue established |
| M1 ✅ | Completed Jul 21 | Current source and extracted release archive are buildable |
| M2 ✅ | Completed Jul 22 | Historical migrations preserve payloads and SQLite identifiers are safe |
| M3 ✅ | Completed Jul 23 | Read-only boundaries and Unicode/path handling are non-bypassable and non-panicking |
| M4 ✅ | Completed Jul 28 | Declared MSRV and dependency-security policy are verified |
| M5 ✅ | Completed Jul 28 | Async/watcher failure handling and highest-risk maintainability debt are addressed |
| M6 ✅ | Completed Jul 30 | CI, documentation, release gates, and fresh RC evidence agree |
| M7 | Next; only milestone remaining | Independent architecture review and owner release decision |

Two bounded residual corrections—partial-hash `explain` comparison and the
CLI import overwrite contract—are complete with regression evidence. They do not
broaden the async/watcher RFC or create separate review gates.

M1 completed on 2026-07-21 at implementation commit `e54cfe2` after focused
independent review and correction of its review record. Archive verification in
CI and the remaining release-control hardening are explicitly deferred to M6;
M1 completion is not release authorization.

RFC 010 implementation was independently accepted on 2026-07-21 at commit
`95fd1a0`, closing B-02. RFC 011 implementation was independently accepted on
2026-07-22 at commit `d4fe505`, closing B-03 and completing M2. Both RFCs remain
under `rfcs/accepted/` until their implementation ships; this milestone closure
is not release authorization.

RFC 012 implementation was independently accepted on 2026-07-22 at commit
`6c14df3`. RFC 013 implementation was independently accepted on 2026-07-23 at
commit `34fcc78`, completing M3. Both remain under `rfcs/accepted/` until their
implementation ships; this milestone closure is not release authorization.

RFC 014 implementation was independently accepted on 2026-07-28 at commit
`b5e85da`, closing B-06 and B-08 and completing M4. RFC 015 implementation
was independently accepted at commit `772b3e5`, completing M5. Both remain
under `rfcs/accepted/`; neither closure authorizes release work.

M6b (canonical gate consolidation) was independently accepted at commit
`11a8bc8`, closing **B-07** — the last of the eight original blocking
findings. **All eight are now closed.** This closure authorizes no release
action.

M6c (CI provenance) was independently accepted at commit `d86fda7`. Its two
canonical-producer proof items were withdrawn rather than carried forward:
[RFC 017](https://github.com/nabbisen/localcache-rs/blob/main/rfcs/accepted/017-content-reproducible-archives-without-a-container-producer.md)
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

M7 is the only remaining milestone. These closures authorize no release action.

The virtual-workspace relocation at `fe9fe88` was accepted for continued
development. Its recorded legal-file publication blocker never existed and was
withdrawn with RFC 016; the repository-root `LICENSE` and `NOTICE` are
sufficient and remain the sole copies, never placed in member crate
directories.

The detailed scope, RFC mapping, dependencies, and exit gates are maintained in
the repository-root
[ROADMAP.md](https://github.com/nabbisen/localcache-rs/blob/main/ROADMAP.md).
Dates are targets; no milestone is complete until its exit gate passes.

## Future directions

- Performance tuning for very large namespaces (> 1M entries)
- Cross-process shared-cache via named shared memory (beyond RFC 004 scope)
- `#[async_test]` proc-macro wrapper for unified async test authoring across
  runtime backends (deferred from RFC 005)
