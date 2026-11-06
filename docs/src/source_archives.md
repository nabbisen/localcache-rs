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

## Uncommitted files

The archive is of `HEAD`, so a file that was never committed is not in it.
`cargo package --workspace --locked` (in `cargo make gate-docs`) refuses a
working tree that has uncommitted or untracked files, so run it before pushing.

## Publishing

A release is published by the `Release` workflow (`.github/workflows/release.yaml`),
which the project owner starts by hand. No registry token is stored in the
repository or in CI: the workflow exchanges a short-lived GitHub identity for a
crates.io token (Trusted Publishing). GitHub records who started each run.

### Steps for the owner

1. Push a signed, annotated tag named exactly like the version in `Cargo.toml`,
   without a `v`: `git tag -s 0.22.0` and `git push origin 0.22.0`.
2. In *Actions*, run **Release** from that tag with `dry_run` on (the default).
   Read the `Gate` and `Source archive` results.
3. Run **Release** again from the same tag with `dry_run` off. This publishes the
   three crates (`localcache-format`, `localcache`, `localcache-cli`) and creates the GitHub release. Nothing asks for confirmation after
   this: starting the run with `dry_run` off is the authorization. (If a required
   reviewer is added to the `release` Environment in the repository settings, the
   run waits for that reviewer before the `Publish` job.)

### What each job does

| Job | What it does |
|---|---|
| `Gate` | Fails unless: the run is from a tag (a branch is accepted only for a dry run from `main`); the tag is annotated and equals the workspace version exactly (a `v` prefix can never equal it); the commit is reachable from `main`; the newest `ci.yaml` run on the commit is `completed` with `success`; and `scripts/check_versions.py --expect <version>` passes (it also checks that the exact pin of `localcache-format` in `Cargo.toml` is that version) |
| `Advisories`, `MSRV fresh resolution` | The weekly checks, run again at the moment of release, because an advisory or a new upstream release appears without any push |
| `Source archive` | `git archive` of the commit as `localcache-vX.Y.Z.tar.gz`, its SHA-256, and a build-provenance attestation |
| `Publish` | The only job that asks crates.io for a registry token; it runs in the `release` Environment. It runs `cargo publish --workspace --locked` (with `--dry-run` for a dry run) and records the three `.crate` digests, in the order `localcache-format`, `localcache`, `localcache-cli` |
| `GitHub release` | Only when `dry_run` is off: a release named by the tag, with the version's `CHANGELOG.md` section, the archive, and the archive and three `.crate` SHA-256 digests |

The tag's signature is made on the owner's machine and is **not** verified by
CI. To check an archive from a release:

```sh
gh attestation verify localcache-vX.Y.Z.tar.gz --repo nabbisen/localcache-rs
```

### Dry run from `main`

A dry run may also be started from `main`. It runs the whole chain (the gate on
its branch path, both reusable workflows, the archive and its attestation, the
identity exchange with crates.io for the three crates, and `cargo publish --dry-run`)
and publishes nothing, so a mistake in the setup shows up before a release.

### If a publish stops half way

`cargo publish --workspace` uploads the crates in dependency order
(`localcache-format`, `localcache`, `localcache-cli`). If an earlier crate was
uploaded and a later one failed, **do not run the workflow again**: the first
crate's version already exists. Check crates.io, then publish the missing crates
by hand with the owner's crates.io credentials, or release a new patch version.

The same rule applies if the `Publish` job fails *after* all three crates were
uploaded, for example while recording their digests: the crates are live, the
GitHub release was not created, and running the workflow again would fail at the
upload. Create the GitHub release by hand, with the archive from the `Source
archive` job and the three `.crate` digests as crates.io serves them.

### One-time setup (owner)

- GitHub Environment `release` (the name must match the workflow). Its deployment
  rules allow the branch `main` (dry runs) and version tags (`[0-9]*.[0-9]*.[0-9]*`).
  It needs no secrets or variables, and a required reviewer is optional.
- On crates.io, for each of `localcache-format`, `localcache` and `localcache-cli`, a trusted publisher
  with repository `nabbisen/localcache-rs` and workflow `release.yaml`. The
  environment field is not set.
- **A crate name that does not exist yet cannot be published by the workflow.**
  crates.io refuses a Trusted Publishing token the creation of a crate
  (`403 Forbidden: Trusted Publishing tokens do not support creating new crates.
  Publish the crate manually, first`), and a trusted publisher can be added only
  to a crate that exists. The first version of a new crate is therefore published
  once, by the owner, from a clean checkout of the signed tag with an API token
  that has the `publish-new` scope (`cargo publish --workspace --locked`, its output
  written to a file), and the trusted publisher is added afterwards. This happened
  once, for `localcache-format` 0.25.0; every later release goes through the
  workflow.
