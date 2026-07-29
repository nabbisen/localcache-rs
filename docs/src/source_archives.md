# Project Source Archives

Project source archives are named:

```text
localcache-vX.Y.Z.tar.gz
```

Project files are stored directly at the archive root. There is no
`localcache-vX.Y.Z/` member prefix inside the tarball. Create a new empty
destination and extract into it:

```sh
mkdir localcache-vX.Y.Z
test -z "$(ls -A localcache-vX.Y.Z)"
tar -xzf localcache-vX.Y.Z.tar.gz -C localcache-vX.Y.Z
```

Never extract an archive over an existing checkout or populated directory.

Maintainer archive verification is implemented by `scripts/release.py`. Its
source context requires a clean committed revision, constructs the export from
that exact commit, validates structured tar headers and the complete export
manifest before extraction, and records the commit-to-SHA-256 binding. Its
artifact context contains no Git metadata and runs the applicable metadata,
package-scoped check, benchmark-compilation, and mdBook smoke gates with build
output outside the extracted source.

`cargo make archive` (`scripts/release.py source`) is the maintainer entry
point and runs on any supported host — there is no separate container
producer or canonical/noncanonical distinction. The archive's integrity
identifier is the SHA-256 of the *uncompressed* tar stream; two consecutive
constructions from the same clean commit on the same host must match it. The
compressed `.tar.gz` digest is recorded alongside it but is advisory only,
never asserted as reproducible across hosts. `rc_eligible` derives from a
clean committed tree, every required gate passing, and a complete evidence
bundle — not from which machine produced the archive.

The runner only constructs and verifies a review candidate. It does not tag,
push, publish crates, or create a hosted release.

`cargo make release` (`scripts/release.py release`) is the canonical release
entry point (RFC 009 R12). It runs `source`, `msrv`, `doc-package`, and
`security` in order as subprocesses, fails fast on the first failing gate,
and writes one consolidated evidence manifest. The `msrv` gate runs under the
declared MSRV toolchain explicitly, via `rustup run`, regardless of whichever
toolchain is otherwise ambient; the other three gates run under the ambient
toolchain (stable, by convention and in CI). This split is required, not
incidental: `cargo package --workspace --locked` (part of `doc-package`)
verifies each workspace member in an isolated build against the real
crates.io index rather than against its just-packaged sibling, so under a
cargo old enough for that distinction to matter, the CLI's `localcache`
dependency can resolve to an older published version instead of the one just
packaged — pulling in transitive dependencies incompatible with the declared
MSRV even though the workspace itself compiles fine under it. Packaging is
release tooling, not an MSRV assertion; proving the workspace compiles under
the declared MSRV is `msrv`'s `cargo check` matrix, run separately.

The bootstrap source-integrity check discovers the virtual workspace members
under `crates/` and covers their manifest-declared targets plus conventional
`src/lib.rs`, `src/main.rs`, and `build.rs` roots. In source context those
target files must also be Git-tracked. Auto-discovered nested targets are left
to Cargo metadata and compilation; archive completeness is instead enforced by
comparing every structured archive member with the full committed Git export
manifest.
