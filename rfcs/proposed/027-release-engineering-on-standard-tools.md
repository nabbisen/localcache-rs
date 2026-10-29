# RFC 027 — Release Engineering on Standard Tools

| Field | Value |
|---|---|
| Status | Proposed |
| Feature | *(release engineering; no Cargo feature)* |
| Touches | `scripts/`, `.github/workflows/`, `Makefile.toml`, `security/`, `deny.toml` (new), `docs/src/source_archives.md`, `docs/src/dependency_security.md`, `README.md` |
| Amends | [RFC 009](../done/009-reproducible-source-archives-and-release-gates.md) (R3, R5, R6, R7, R12, R13, R14, R15: the mechanisms; their goals stand) and [RFC 017](../done/017-content-reproducible-archives-without-a-container-producer.md) |
| Finding | Owner request, 2026-10-29: is RFC 009 still reasonable under the project philosophy? Architect review `.git-exclude/reviewed/031-architect-rfc009-release-engineering-review-2026-10-29.md` |
| Milestone | Phase 24 Q9 |
| Breaking | **No.** No library, CLI, schema or wire-format change. The archive filename and root layout (RFC 009 R4) are kept |
| Authorship | High-capability model; **reviewed by the owner** (arrangement of 2026-08-01). The author also wrote RFC 009, and says so |

## Summary

Keep RFC 009's goals and replace the machinery that serves them. A release gate should be a short list of
standard tools run by CI, not a 4,000-line Python framework with 3,400 lines of tests. Publishing should
use crates.io **Trusted Publishing** behind a GitHub Environment that the owner approves, instead of a
long-lived token on a workstation.

## Motivation

The owner's standing rules are "finally clean, safe and secure, robust and sophisticated design" and
"keep it simple, standard-oriented, clean and easy to understand; don't make it complicated or messy".
Measured on 2026-10-29:

- production Rust is about 7,700 lines; the release machinery (`scripts/*.py` 3,957, their tests 3,391,
  workflows 514, `Makefile.toml` 273) is about 8,100 lines;
- cutting RCs found defects in the machinery itself (RC-1 to RC-4, N3, and a stale User-Agent), and one
  RC cycle produces a 685 MB bundle that retention rules then prune;
- the archive validator (RFC 009 R5) checks an archive built a minute earlier from the same repository,
  so the attacker it defends against cannot exist in this pipeline;
- the `release-tools.toml` hash pins protect scripts that live in the repository they pin, so they add no
  security;
- a standard tool already reproduces the 1,059-line advisory gate: on 2026-10-29 `cargo deny` with
  `unsound = "all"` reported the same 15 advisories on `24544bc` and exit 0 on the fixed tree. Its
  default scope hid 10 of the 11 unsound reports, which is why the configuration below is deliberate;
- publishing uses a long-lived token in `~/.cargo/credentials.toml`, and the owner's authorization exists
  only as chat text.

## Goals

- Every RFC 009 goal that still protects the project, with less code and tools a contributor already knows.
- A release that is **owner-authorized, auditable and free of long-lived secrets**.
- The v0.22.0 release is cut once, on the simpler pipeline.

## Non-goals

- Changing what the gates check (the matrix rows, the MSRV rows, the policy for advisories).
- Artifact signing beyond GitHub attestations, `cargo-dist`, binary releases of the CLI.
- Replacing the version-consistency checks (RFC 009 R10, R11), which are project-specific.

## Requirements

### R1 — What stays exactly as RFC 009 has it

R1 (manifest completeness), R2 (archive completeness), R4 (archive name and root layout), R8 (declared
MSRV, with the RFC 023 fresh-resolution check), R9 (rustdoc, mdBook, `cargo package --workspace --locked`),
R10 and R11 (coming-version housekeeping and consistency), and **all of R16** (the CI trust boundary:
read-only permissions, no `pull_request_target`, every third-party action pinned to a commit SHA, no
publish secrets in build jobs).

### R2 — The gates are standard commands in one place

`Makefile.toml` (or the CI workflow, if the owner prefers; see the open question) holds each gate as the
literal command, and CI runs those same commands. That satisfies RFC 009 R12 ("one executable source of
truth") without a runner.

- **Feature matrix (R7):** `cargo hack --each-feature` for the library and an all-features row, plus the
  RFC 009 rows for `async-std` and `smol` without Tokio. The unassigned-feature check that
  `feature_matrix.py --check-coverage` provides is kept as a one-line assertion that the feature list in
  `Cargo.toml` equals the list the CI rows name.
- **Dependency security (R13):** `cargo deny check` with a checked-in `deny.toml`:
  `advisories` with `version = 2`, `yanked = "deny"`, `unmaintained = "all"`, **`unsound = "all"`**, and
  an `ignore` entry per standing disposition whose `reason` carries today's follow-up condition;
  `sources` with `unknown-registry = "deny"` and `unknown-git = "deny"`. `security/advisory-policy.json`
  is deleted. Dependabot alerts (a repository setting) back it up between pushes.
- **Version consistency (R10, R11):** the existing checks, extracted to one short script (about 100
  lines).

### R3 — The archive is `git archive` of the tag

The project source archive is `git archive --format=tar.gz`, from the release tag, named
`localcache-vX.Y.Z.tar.gz`, with files at the archive root (RFC 009 R4). The structured tar parser, the
export-manifest comparison and the hostile-archive fixtures (RFC 009 R5) are deleted: nothing untrusted
produces or supplies the file. The extracted-archive smoke gate (R6) stays as **one CI job**: extract the
archive into an empty directory, run `cargo metadata --locked`, `cargo check --workspace --all-targets
--locked` and `mdbook build docs`, with build output outside the source. `cargo package --workspace
--locked` already builds each crate from its packaged files.

### R4 — Provenance comes from the CI run

An evidence bundle, a toolchain-identity table and hash-pinned scripts are replaced by what the platform
already binds to a commit: the Actions run, its logs, and a GitHub **artifact attestation** for the release
archive. The release notes list the archive SHA-256 and the two `.crate` digests. `scripts/release.py`,
`scripts/release_archive.py`, `scripts/check_advisories.py`, `scripts/feature_matrix.py`, their tests, and
`scripts/release-tools.toml` are deleted. `scripts/source_integrity.py` is kept only if the manifest-target
check it performs is not already covered by `cargo metadata --locked` failing on a missing target; the
implementation shows which.

### R5 — Publishing through Trusted Publishing, approved by the owner

A new `.github/workflows/release.yaml`:

- **Trigger:** `workflow_dispatch` with the version as input, run from a signed tag the owner has pushed.
- **Gate job:** read-only. It checks that the tag is on `main`, that the tag equals the workspace version,
  and that CI is green on that commit, then builds the archive and attests it.
- **Publish job:** the only job with `id-token: write`. It runs in a GitHub **Environment** named
  `release` whose **required reviewer is the owner**, so the owner's authorization is an explicit,
  recorded approval (RFC 009 R15's intent). It exchanges the OIDC identity for a short-lived crates.io
  token with the SHA-pinned `rust-lang/crates-io-auth-action`, then runs
  `cargo publish --workspace --locked`.
- **crates.io side:** a trusted-publisher record for each crate, `localcache` and `localcache-cli`, naming
  this repository, the workflow file `release.yaml`, and the environment `release`. crates.io stores
  exactly those fields.
- **No long-lived publish token** remains in the repository, in CI, or (after the first success) on a
  workstation. The tag stays a local, GPG-signed action by the owner.

RFC 009 R15 is amended from "CI never publishes" to "nothing publishes without an explicit, recorded
owner approval, and no long-lived credential exists in CI".

### R6 — Migration

1. **Q9a (no deletions):** add `deny.toml`, `cargo hack` rows and `dependabot.yml` next to the old gates.
   On the same trees (`24544bc` and the current tip) the old and new gates must agree, as the 2026-10-29
   parity test did. Differences are findings.
2. **Q9b:** delete what R2 to R4 replace, in separate commits per tool, each with its parity result.
3. **Q9c:** add `release.yaml`, configure the Environment and the crates.io records, and document the
   release procedure in `docs/src/source_archives.md`.
4. **First use at the v0.22.0 release.** A real publish cannot be rehearsed, so the owner keeps the
   existing token until the first workflow publish succeeds, then revokes it.

## Test plan

- Parity: old and new advisory gates give the same denied set on `24544bc` and the same pass on the tip.
- Parity: the new feature rows equal the old matrix rows (names and feature sets).
- A deliberately failing row, a missing target, and a stale install example each fail the new CI.
- `release.yaml`: a dispatch from a tag not on `main`, or with a mismatched version, fails before the
  publish job. An unapproved Environment never reaches `id-token`.
- The workflow passes the RFC 009 R16 checks (read-only default, pinned SHAs, no `pull_request_target`).

## Security considerations

Net effect: less code to audit; no long-lived registry token; publish authority limited to one workflow,
one environment and one approver; CI trust boundary unchanged. Removed controls were reviewed one by one
in the architect review: the tar validator and the script hash pins defended against actors the pipeline
does not contain. `cargo deny` replaces fetching sparse-index records because Cargo verifies lockfile
checksums when it downloads a crate. **Not preserved:** a condition-based follow-up as structured data
(`reason` is free text) and the "stale ignore entry is denied" behaviour; `cargo deny` warns on an
unmatched ignore (`advisory-not-detected`). The implementation makes that warning fatal if the tool allows
it, or records the loss.

## Compatibility

No change to any crate. The archive keeps its name and root layout. Contributors lose `scripts/release.py`
and gain standard commands.

## Alternatives considered

- **Keep RFC 009 as is.** It works and is green. It is also the largest component of the repository, and
  it is where the last four RC defects were.
- **`release-plz` or `cargo-release`.** They automate version bumps, changelogs and tags in ways that
  conflict with the owner's authorization flow and the project's CHANGELOG discipline.
- **Keep a long-lived token in a CI secret.** Rejected: Trusted Publishing gives the same automation
  without a stored credential.

## Rollback

Q9a adds files only. Q9b is reverted by restoring the deleted scripts from Git. Q9c is withdrawn by not
using `release.yaml`; the manual `cargo publish --workspace --locked` path never goes away while the token
exists.

## Decisions requested of the owner

1. **Adopt Trusted Publishing** with an owner-approved `release` Environment (R5), or keep local publishing.
   Recommended: adopt.
2. **Is a per-release `localcache-vX.Y.Z.tar.gz` still a deliverable?** The README promises it. Recommended:
   keep it, as a plain `git archive` attached to a GitHub Release by `release.yaml`, with an attestation.

## Open questions

1. Whether the gate commands live in `Makefile.toml` (needs `cargo-make`, installed in CI by pinned
   `cargo install`) or directly in the workflow steps. Recommended: `Makefile.toml`, so the local
   command equals the CI command.
2. The crates.io trusted-publisher setup page and the current form of the auth action are confirmed
   against the crates.io documentation before Q9c, not assumed.
