# RFC 009 Implementation Handoff

## 1. Summary

Implement the M1 source/archive recovery slice of
[RFC 009](../../done/009-reproducible-source-archives-and-release-gates.md).
Extend the same architecture at M6; do not create a second release runner.

RFC 009 is Accepted. Its requirements are authoritative. This handoff sequences
the work but does not add or override design decisions.

M1 ends at a focused implementation review point. It does not tag, publish,
push, or create a release.

## 2. Scope followed

### M1 scope

1. Author the meaningful Criterion benchmark required by the existing
   `cache_bench` target.
2. Add source-integrity preflight that remains runnable when Cargo manifest
   targets are incomplete.
3. Implement the shared gate definitions, source-context orchestrator, and
   Git-free artifact-context verifier.
4. Implement deterministic committed-source archive construction.
5. Implement structured archive validation, exact export-manifest comparison,
   and malicious archive fixtures.
6. Verify the checkout and fresh extraction with the RFC's M1 smoke gates.
7. Produce evidence tied to the exact commit and archive digest.

### Deferred to M6

- complete stable package-scoped feature and doctest matrices;
- RFC 014 MSRV and dependency-security policy integration;
- joint workspace package verification;
- final CI aggregation and least-privilege enforcement;
- coming-version housekeeping;
- full RC archive/evidence generation; and
- independent release-candidate architecture review.

M1 may create reusable foundations for these items but must not claim their M6
gates complete.

## 3. Files changed

This acceptance transition created:

- this implementation handoff; and
- `acceptance-qa-checklist.md`.

Expected M1 implementation areas, subject to the accepted RFC, are:

- `benches/cache_bench.rs`;
- a canonical runner under `scripts/` or a bootstrap-safe Rust `xtask`;
- structured archive parser/validator code and hostile fixtures;
- a checked-in producer-tool manifest;
- thin `Makefile.toml` and CI entry points where needed for M1; and
- release/archive documentation affected by the owner-approved root layout.

The implementer must keep the actual file set small and reviewable. If the
chosen runner structure conflicts with RFC requirements, pause and revise the
RFC before implementation continues.

## 4. Design decisions and assumptions

- Archive contents are at archive root. User guidance extracts only into a
  newly created empty directory.
- The benchmark is meaningful and retained; compilation blocks release,
  measurements do not.
- The canonical producer is the exact Rust 1.97.1 Linux/amd64 platform digest
  recorded in R16. Mutable tags are informational only.
- Source context owns Git cleanliness, commit identity, archive construction,
  digest binding, and orchestration.
- Artifact context requires no `.git/`, builds no nested archive, and receives
  expected version/layout from its parent.
- Archive parsing is structured. Human-formatted tar listings are not a
  security parser.
- Logical members are regular files/directories. The sole control-record
  exception is the commit-bound global PAX record defined by R5.
- The exact export manifest covers normalized path, type, and executable mode.
- Evidence and generated artifacts stay outside the project source archive.
- Every release action remains owner-only and outside the runner.

Assumption: one primary implementer owns M1. Independent review is a separate
exit gate.

## 5. Tests and gates run

At handoff creation, observed design/lifecycle evidence includes:

- final architecture verdict: Accept;
- `git diff --check` for the final narrow RFC revision: passed;
- `cargo fmt --all --check`: passed;
- `mdbook build docs`: passed;
- canonical `git archive` commit binding: matched the reviewed commit; and
- the tracked tree was clean before this acceptance transition.

No M1 implementation gate is claimed as passing. Normal Cargo all-target gates
remain blocked until `benches/cache_bench.rs` is authored. This blocks M1 exit
but does not invalidate the accepted design.

## 6. Generated artifacts

The acceptance change generates only repository documentation:

- `implementation-handoff.md`
- `acceptance-qa-checklist.md`

M1 must later generate its source archive and evidence outside the source tree.
Those future artifacts are not present or claimed here.

## 7. Known limitations

- The canonical image has been registry-verified but not executed locally.
- The producer-tool manifest and auxiliary integrity hashes do not yet exist.
- The archive validator and hostile fixtures do not yet exist.
- The benchmark source is absent.
- The original architecture-review findings B-01 and B-07 are not
  implementation-closed merely because RFC 009 is Accepted.
- RFC 014 is not yet available, so M6 security/MSRV integration remains
  pending.

## 8. Recommended next step

After the owner commits this Accepted transition and handoff, begin M1 with the
smallest bootstrap slice:

1. author the meaningful benchmark;
2. add source-integrity preflight independent of Cargo compilation; and
3. request an implementation review before expanding into archive production
   and extraction.

Use the companion QA checklist throughout M1. Stop and return to RFC review if
implementation reveals a conflict with the accepted contract.
