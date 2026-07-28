# RFC 016 — Published Crate Legal-File Completeness

| Field | Value |
|---|---|
| Status | Accepted |
| Feature | *(release engineering; no Cargo feature)* |
| Touches | `crates/localcache/`, `crates/cli/`, `scripts/release.py`, `.github/workflows/ci.yaml`, release documentation |
| Finding | Virtual-workspace relocation review (2026-07-21), §3 R1 — publication blocker |
| Milestone | Phase 21 M6 |

## Summary

Every published `.crate` artifact must contain `LICENSE` and `NOTICE`, byte-identical to the
repository-root originals, and the release gate must verify that **inside the generated archive**
rather than on disk. Today neither crate ships either file, which blocks publication.

This RFC changes no Rust API, schema, payload format, feature, or dependency. It requires one owner
decision (§ Owner decision required) because the recommended resolution qualifies an earlier
owner ruling.

## Motivation

Observed on the generated artifacts:

- `localcache-0.20.0.crate` — contains `README.md`; **no `LICENSE`, no `NOTICE`**.
- `localcache-cli-0.20.0.crate` — **no `LICENSE`, no `NOTICE`, no `README`**.

This is a regression introduced by the virtual-workspace relocation (`fe9fe88`). Before it, the
repository root *was* the `localcache` package directory, both files sat inside it, and
`exclude = ["benches/", "target/", ".github/", "docs/"]` did not exclude them — so they were
packaged. After relocation they sit above `crates/localcache/`, outside every package root. The CLI
never shipped them; that part is pre-existing rather than a regression, but is equally incomplete.

Apache-2.0 §4(a) requires giving recipients a copy of the License, and §4(d) requires reproducing
the attribution notices from a `NOTICE` file **when the work has one — this project does**. The
`license = "Apache-2.0"` SPDX field is metadata describing the terms; it is not the terms.

## Why this needs design rather than a one-line fix

Four constraints interact, and three plausible fixes each violate one:

1. **Cargo cannot reach above a package root.** `include` and `exclude` are resolved relative to the
   package directory; `../../LICENSE` is not expressible. Nothing outside `crates/<pkg>/` can be
   packaged.
2. **Symlinks break the source-archive gate.** RFC 009 R5 permits only regular files and
   directories; `scripts/release_archive.py` rejects link entries outright. A tracked symlink at
   `crates/localcache/LICENSE` would appear in `git archive` output as a link member and fail
   validation before extraction.
3. **Package-time copying conflicts with RFC 009 R9**, which requires packaging without
   `--allow-dirty`. Materialising untracked files immediately before `cargo package` produces
   exactly the dirty state R9 forbids.
4. **An existing owner ruling** (2026-07-21, workspace relocation): root `LICENSE` and `NOTICE` are
   the sole authoritative copies, with no per-crate copies.

## Owner decision required

The recommended resolution (R1–R5 below) **qualifies constraint 4**. The owner must accept or
redirect before implementation.

The ruling's purpose was to prevent divergent legal text, and its wording addressed *repository
layout*. The packaging consequence was not in view when it was made. The recommendation preserves
the ruling's intent — one authoritative source, no drift — by making the crate-level files
**verified mirrors** rather than independent copies: a gate fails the release if any mirror differs
from the root file by a single byte. Under that constraint "authoritative" remains true in the sense
that matters, and drift becomes impossible rather than merely discouraged.

Alternatives are recorded in § Alternatives considered. If the owner prefers to keep the ruling
literal, the only remaining option that satisfies Apache-2.0 is amending RFC 009 R5 to carve out a
narrow symlink exception, which trades a legal-completeness fix for a security-surface change in the
archive validator; this RFC does not recommend it.

## Requirements

### R1 — Both published crates carry both files

`crates/localcache/LICENSE`, `crates/localcache/NOTICE`, `crates/cli/LICENSE`, and
`crates/cli/NOTICE` exist as tracked regular files, so `cargo package` includes them without any
manifest change. Neither package's `exclude` may filter them.

The CLI additionally gains a `README.md`, or its `readme` field is pointed at a packaged file, so
its crates.io page is not blank. If no CLI-specific README is wanted, inheriting the workspace
README is acceptable; what is not acceptable is a `readme` key naming a file absent from the
package.

### R2 — Mirrors are byte-identical, enforced by a gate

A release-gate step compares each crate-level `LICENSE` and `NOTICE` against the repository-root
file of the same name and fails on any difference. The comparison is over raw bytes, not normalised
text. This gate runs in the source context, before packaging.

### R3 — Root remains the authoritative source

The repository-root `LICENSE` and `NOTICE` remain the files a human edits. Crate-level copies are
derived artifacts that happen to be tracked. This is stated in the release documentation and in a
short comment or `README` note under each crate directory, so a future contributor edits the root
file and lets the gate catch the mirrors.

### R4 — The gate verifies content **inside** the generated `.crate`

Presence on disk is insufficient — the defect this RFC closes was invisible precisely because the
files existed at the repository root while being absent from the artifact. After
`cargo package --workspace --locked`, the gate must open each produced `.crate`, confirm `LICENSE`
and `NOTICE` are present as members, and confirm their bytes match the root originals.

This check is the acceptance criterion for the whole RFC. A change that adds the files but does not
verify them inside the artifact does not satisfy it.

### R5 — The source archive contract is unchanged

Because the mirrors are regular files, RFC 009 R5's link prohibition, exact export manifest, and
member-type checks continue to hold unmodified. The archive simply gains four members. No RFC 009
requirement is amended by this RFC, and `scripts/release_archive.py` needs no change beyond whatever
the export manifest naturally reflects.

### R6 — Licence metadata is unchanged

`license = "Apache-2.0"` stays as the SPDX expression in `[workspace.package]`, inherited by both
members. Do **not** switch to `license-file`: the SPDX identifier is machine-readable and correct,
and `license-file` would suppress it on crates.io.

## Test plan

- `cargo package --workspace --locked` produces both crates; each contains `LICENSE` and `NOTICE`.
- The bytes of each packaged `LICENSE`/`NOTICE` equal the repository-root file.
- Mutating one crate-level mirror by a single byte fails the R2 drift gate.
- Deleting one crate-level mirror fails the R2 gate and, independently, the R4 in-artifact check.
- The R4 check fails when a file is present at the repository root but absent from the `.crate` —
  the exact shape of the original defect, exercised deliberately.
- The source archive still validates: no link members, export manifest matches, extraction succeeds.
- `cargo package` emits no new warning; the pre-existing benchmark-exclusion warning is unchanged.
- The CLI crate's `readme` field, if set, names a file present in the package.

## Security considerations

None material. The change adds four tracked text files and one comparison step. It removes no
validation, and R5 explicitly declines to weaken RFC 009's link prohibition — which is the reason
the symlink alternative is rejected rather than adopted.

## Compatibility

No Rust API, schema, payload, feature, dependency, or MSRV change. Published crates gain files; they
lose nothing. No version bump is required beyond the coming v0.20.1 already planned for M6.

## Alternatives considered

### Amend RFC 009 R5 to permit exactly two symlink pairs

Rejected. It keeps a single physical copy, but requires amending an Accepted RFC's security
requirement, adding a link exception with its own validation surface (exact names, target must
resolve within the archive root, exactly two permitted pairs) to the one component whose job is
rejecting hostile archive members. Symlinks also degrade on Windows checkouts. Trading a security
boundary for a file-duplication preference is the wrong exchange.

### Copy the files in immediately before packaging, then remove them

Rejected. RFC 009 R9 requires packaging without `--allow-dirty`, and this produces precisely the
dirty worktree that forbids. It also makes the artifact's contents depend on an unversioned script
step rather than on tracked repository state.

### Publish with only the SPDX metadata field

Rejected. Apache-2.0 §4(a) and §4(d) are obligations on the distributor, and this project has a
`NOTICE` file that §4(d) makes non-optional. Metadata is not the licence text.

### Point `license-file` at a crate-level copy

Rejected as a substitute for R1, though harmless alongside it. `license-file` replaces the SPDX
expression on crates.io with a file reference, losing machine-readable licence identification for no
gain once R1 already places the file in the package.

## Rollback

Before publication, rollback is deletion of the four mirrors and the gate step. After publication,
nothing to roll back — the files are additive and no consumer can be harmed by their presence.

## Open questions

One, recorded above: whether the owner accepts qualifying the 2026-07-21 no-per-crate-copies ruling
in favour of verified mirrors, or prefers the symlink amendment this RFC recommends against. No
other design axis is open.
