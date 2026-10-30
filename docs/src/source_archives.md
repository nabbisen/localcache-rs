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

## How an archive is checked

The archive is `git archive` of a committed revision, and nothing else: the
members are the tracked files. `cargo make gate-archive` builds and checks it,
and CI runs the same task on every push (job "Project archive"):

1. It builds `localcache-v<version>.tar.gz` from `HEAD`, in a temporary
   directory outside the repository. The `v` is in the file name only; the
   version is the workspace version.
2. It checks the archive is complete: the sorted member list equals
   `git ls-files`, and no member is `.git`, `.git-exclude`, `target` or
   `docs/book`.
3. It extracts the archive into a new empty directory and, from there, runs
   `cargo metadata --locked`, `cargo check --workspace --all-targets --locked`
   and `mdbook build docs`, with the build output outside the extracted source.
4. It prints the archive's SHA-256.

Nothing untrusted produces or supplies the file, so there is no archive parser
and no export manifest. The task needs a committed tree, because the archive is
of `HEAD`; it creates no tag and publishes nothing.

## Source integrity

`gate-archive` builds from the commit, so it cannot see a file that was never
committed: an untracked `build.rs` or `src/main.rs` is simply absent from the
archive, and the archive builds without it. The source-integrity check
(`cargo make source-integrity`, CI job "Source integrity") covers that gap
without invoking Cargo. It discovers the virtual workspace members under
`crates/` and covers their manifest-declared targets plus the conventional
`src/lib.rs`, `src/main.rs` and `build.rs` roots; in source context those
target files must also be Git-tracked. Auto-discovered nested targets (for
example a file under `tests/`) are left to Cargo metadata and compilation.

## Publishing

A release is published by the project owner with
`cargo publish --workspace --locked`, after CI is green on the release commit.
The documentation here describes the checks, not the publication.
