# RFC 017 — Content-Reproducible Archives Without a Container Producer

| Field | Value |
|---|---|
| Status | Implemented (0.20.1) |
| Feature | *(release engineering; no Cargo feature)* |
| Touches | `rfcs/done/009-…` R16 and related clauses, `scripts/release.py`, `scripts/release_archive.py`, `scripts/release-tools.toml`, `scripts/canonical-producer.sh`, `.github/workflows/ci.yaml` |
| Amends | **RFC 009** — see § Clauses amended |
| Finding | Owner challenge, 2026-07-28: is the container producer necessary, useful, effective? |
| Milestone | Phase 21 M6c |

## Summary

Replace RFC 009's container-based **compressed-byte** reproducibility contract with
**content reproducibility**: the archive's integrity identifier becomes the SHA-256 of the
*uncompressed* tar stream, determinism is required per host rather than per pinned image, and
RC eligibility depends on gates passing rather than on which machine ran them. The existing
export-manifest and Git-blob-identity controls — which are what actually verify the archive against
the commit — are preserved unchanged.

Net effect: the same detection power for *"do these archive contents match the reviewed commit"*,
without Docker, without an image digest to maintain, and without the requirement that has blocked two
milestones. RFC 009 R5's security controls are untouched.

## Motivation

### The container pins two variables, not the ones it appears to

RFC 009 R16 lists a broad contract (OS identity, `git`/archive/compressor versions, locale, timezone,
member ordering, modes, uid/gid, mtime, gzip headers). Tracing the implementation, most of that is
already normalized **in code**, independent of any environment:

- `release_archive.compress_tar` uses `gzip.GzipFile(filename="", mtime=0, compresslevel=9)` — no
  filename, no wall-clock stamp;
- `build_git_tar` uses `git -c tar.umask=0022 archive --format=tar` — normalized modes;
- `validate_tar` requires `uid=0, gid=0, uname=root, gname=root` and rejects anything else;
- member mtime is derived from the commit timestamp;
- member ordering comes from git's tree order, deterministic for a fixed commit.

The archive path invokes **no system `tar` and no system `gzip`** — it uses Python's stdlib `gzip`/
`zlib`, and its only subprocess is `git`. So the residual variables the container actually holds
still are exactly two: **`git archive`'s output across git versions**, and **zlib's compressed output
across zlib versions**. That is precisely why `[canonical-base-components]` pins
`libz.so.1.2.13` and `/usr/lib/python3.11/gzip.py`.

Both are eliminated more cheaply: hashing the uncompressed tar removes zlib from the identity
entirely, and `git archive` output for a fixed commit under a fixed umask is stable across a wide git
range — with the version recorded in evidence so any future difference is explainable rather than
mysterious.

### Byte identity is not the control that protects this project

The authoritative content control is already implemented and container-independent:
`expected_manifest()` derives every member from `git ls-tree`, and `validate_tar()` recomputes each
regular file's **Git blob SHA-1** and compares it, member by member, before extraction. That verifies
*content against the commit*. Compressed-byte identity adds only *"the same bytes came out twice on
the same machine."*

Byte-level reproducibility earns its cost when independent parties rebuild and compare digests. This
project has one maintainer, and the source archive is delivered to the owner; crates.io receives
`cargo package` output, which the container never touches. There is no second builder to compare
against.

It is also worth stating plainly: the container was never a security boundary. It pins tool versions;
it does not protect against a compromised build host, which would simply run the container.

### It has not worked

The canonical producer has **never been executed**. M1 accepted a checksum-verified OCI filesystem
under `bwrap` because Docker was unavailable; M6c is now blocked on the same requirement. Three
milestones, zero executions. A control that never runs supplies no assurance — only a recurring
blocker.

It carries maintenance cost besides. The pinned digest was resolved from a mutable tag on
2026-07-17; base images are rebuilt and superseded digests eventually become unpullable, and R16
requires design review to change it. That is a standing obligation with no offsetting benefit at this
project's scale.

## Clauses amended

This RFC replaces the following in `rfcs/done/009-reproducible-source-archives-and-release-gates.md`:

| Location | Current | Becomes |
|---|---|---|
| Non-goals, lines 81–84 | byte-identity disclaimed except in the canonical producer | byte-identity disclaimed entirely; content identity required |
| R14, line 464 | evidence records "canonical producer environment identity" | records **host and toolchain identity** (platform, `git`, Python, zlib, locale, timezone) |
| R14, line 472 | "archive filename, byte size, and SHA-256 digest" | adds the **uncompressed-tar SHA-256** as the integrity identifier; compressed digest retained as advisory |
| R16, lines 495–543 | canonical producer: image digest, platform, base components, tool manifest, "only an archive … produced by the canonical environment may become the release candidate" | § Requirements below |
| Design, lines 648–653 | two consecutive constructions in the canonical producer must match SHA-256 | two consecutive constructions **on the same host** must match the **uncompressed-tar** SHA-256 |
| R9, line 370 and Design line 669 | "the canonical producer's pinned Cargo version" | the recorded Cargo version of the run |
| Acceptance criteria, line 932 | "the canonical producer creates reproducible bytes" | the run produces a reproducible content manifest and uncompressed-tar digest |

RFC 009 **R5 is not amended.** Structured header validation, the exact export-manifest comparison,
Git blob identity, link/special-entry rejection, and validate-before-extract all remain exactly as
accepted. Nothing in this RFC weakens an archive-safety control.

## Requirements

### R1 — Content identity replaces compressed-byte identity

The archive's integrity identifier is the **SHA-256 of the uncompressed tar stream**. Evidence
records it as the primary digest, alongside the compressed `.tar.gz` size and digest, which remain
recorded but **advisory**.

Rationale: the uncompressed stream is what the export manifest describes and what extraction
consumes. Compression is a transport encoding, and making it part of the identity imports zlib's
version behaviour into the contract for no verification gain.

### R2 — Per-host determinism, gated

Two consecutive constructions from the same clean commit **on the same host** must produce an
identical uncompressed-tar SHA-256. This is a blocking gate and is achievable on any supported
platform without special provisioning.

Compressed-byte identity across hosts is explicitly **not** required and must not be asserted in
evidence or release notes.

### R3 — RC eligibility depends on gates, not on environment

`rc_eligible` becomes true when, and only when:

1. the tree is clean and the archive was built from a committed revision;
2. every required gate in the release run passed; and
3. the evidence bundle satisfies R14 with no skipped required step.

The producing environment is **recorded**, not gated. Remove the canonical/noncanonical producer
classes, `RFC009_PRODUCER_IMAGE`, `RFC009_RC_ELIGIBLE`, `[producer].image`,
`[canonical-base-components]`, and the `[supported-platforms]`/`[supported-host-tools]` split
introduced for the noncanonical path.

This also retires the M6c item-5 problem — there is no environmental claim left to attest
externally, so the env-var attestation and its residual weakness both disappear.

### R4 — Toolchain identity is recorded, and drift is explainable

Evidence records, per run: platform triple, `git --version`, Python version, zlib version, locale,
timezone, stable and declared-MSRV `rustc`/`cargo` versions, mdBook version, and security-tool
version. R14's existing per-command logs and exit statuses are unchanged.

If a future run's uncompressed-tar digest differs from a prior run's for the same commit, the
recorded `git` version is the first thing to compare. The goal is an explainable difference, not an
impossible one.

### R5 — Retire the container wrapper

Delete `scripts/canonical-producer.sh` and its `[implementations]` pin. Retain a single release entry
point (`scripts/release.py release`) that runs on any supported host.

If a container is ever wanted again for convenience, it may be reintroduced as an *optional*
execution wrapper that changes no gate semantics — not as a precondition for RC eligibility.

## What is preserved

- Every RFC 009 R5 archive-safety control, unchanged.
- The exact export-manifest comparison and Git blob identity verification — the real content control.
- Clean-committed-source construction (R3), archive-root layout (R4), the extracted-artifact smoke
  gate (R6), the feature matrix (R7), the MSRV gate (R8), package/doc gates (R9), version consistency
  (R10/R11), one canonical gate implementation (R12), the security step (R13), evidence provenance
  (R14 as amended), and no-implicit-release-action (R15).

## What is lost, honestly

1. **Third-party byte-level verifiability of the `.tar.gz`.** If an adopter later wants to rebuild
   the compressed artifact and match digests exactly, that becomes unavailable. Content
   verifiability against the commit remains, and is stronger for the purpose.
2. **A pinned Rust version for archive production.** Archive construction does not compile anything,
   so this affects only the gates run alongside it — which already record their toolchain versions,
   and which RFC 014 R8 pins for MSRV independently.
3. **The "one blessed machine" story.** Replaced by "any clean-commit run with complete evidence,"
   which is weaker as provenance and stronger as a practice that actually executes.

If reproducible-byte provenance becomes a genuine requirement — for example if the tarball is ever
published as a trust artifact — the right answer then is artifact signing, which RFC 009 already
places out of scope and which supersedes byte-comparison anyway.

## Migration

1. `release_archive.py` — compute and return the uncompressed-tar digest; keep `compress_tar` for
   producing the deliverable.
2. `release.py` — record the uncompressed digest as primary; drop `rc_eligibility()`'s environment
   check and replace it with the R3 conditions; drop `verify_tool_manifest`'s canonical branch and
   the platform/host-tool tables; add the R4 toolchain-identity capture.
3. `release-tools.toml` — remove `[producer].image`, `[canonical-base-components]`,
   `[supported-platforms]`, `[supported-host-tools]`, and the `canonical-producer` implementation
   pin. Keep `[implementations]` for the gate scripts.
4. Delete `scripts/canonical-producer.sh`.
5. `.github/workflows/ci.yaml` — the `archive` job drops `--noncanonical` and becomes the ordinary
   archive gate.
6. Update the M6 handoff and QA checklist: M6c items 1, 2, 5, and 8 are replaced by R1–R4 checks.

## Test plan

- Two consecutive constructions from one clean commit on the same host produce an identical
  uncompressed-tar SHA-256.
- The compressed digest is recorded and labelled advisory; no gate compares it across hosts.
- `rc_eligible` is false when the tree is dirty, when any required gate failed, and when a required
  evidence step was skipped — each demonstrated separately.
- `rc_eligible` is true for a clean commit with all gates green and complete evidence, with **no**
  environment variable involved.
- Evidence contains every R4 identity field; a missing field fails the bundle.
- All RFC 009 R5 archive tests continue to pass unchanged, including the hostile-fixture set.
- Removing `canonical-producer.sh` leaves no dangling reference in `Makefile.toml`, CI,
  `release-tools.toml`, or the handoffs.

## Alternatives considered

**Keep R16 as accepted and obtain Docker.** Rejected as the default: it restores a blocker that has
already cost two milestones, to pin two variables that R1 removes from the identity outright, on a
project with no second builder to compare against.

**Keep the canonical producer but make byte identity advisory.** Tempting, and strictly better than
today. Rejected because it retains the image digest, the base-component pins, the producer-class
split, and the design-review obligation on digest changes — all of the maintenance with none of the
gating value.

**Sign the archive instead.** Out of scope for RFC 009 by its own text, and the right answer only if
the tarball becomes a published trust artifact. Recorded as the future direction if that changes.

**Compare normalized content manifests instead of any digest.** Already required by R5 and retained;
R1's uncompressed digest is a cheap single-value summary of the same thing, useful for evidence and
for cross-run comparison.

## Owner decision required

Accepting this RFC supersedes **owner decision #3 of 2026-07-17** (canonical Rust 1.97.1 linux/amd64
producer with byte identity required there). That decision is preserved in RFC 009's record and
should be marked superseded by this RFC rather than removed, matching how R4 recorded its supersession
of the v0.19.0 versioned-parent convention.

## Open questions

None. The single decision is the supersession above.

---

**Authorship note.** This RFC was drafted by the reviewer who raised the finding, at the owner's
request. It therefore requires independent design review by someone other than its author before
acceptance, per RFC 000's five-folder variant.
