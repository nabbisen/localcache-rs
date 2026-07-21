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

`scripts/canonical-producer.sh` is the maintainer entry point for an
RC-eligible archive. It pulls the accepted linux/amd64 image by immutable
platform digest, verifies the observed platform, verifies the official pinned
mdBook artifact, and invokes the source runner inside a read-only container.
`cargo make archive-equivalence` is available for supported-host behavioral
and normalized-content checks, but its evidence is explicitly marked
noncanonical and cannot become a release candidate.

The runner only constructs and verifies a review candidate. It does not tag,
push, publish crates, or create a hosted release.

The bootstrap source-integrity check discovers the virtual workspace members
under `crates/` and covers their manifest-declared targets plus conventional
`src/lib.rs`, `src/main.rs`, and `build.rs` roots. In source context those
target files must also be Git-tracked. Auto-discovered nested targets are left
to Cargo metadata and compilation; archive completeness is instead enforced by
comparing every structured archive member with the full committed Git export
manifest.
