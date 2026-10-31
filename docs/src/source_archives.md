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
which the project owner starts by hand and approves. No registry token is stored
in the repository or in CI: the workflow exchanges a short-lived GitHub identity
for a crates.io token (Trusted Publishing).

### Steps for the owner

1. Push a signed, annotated tag named exactly like the version, without a `v`:
   `git tag -s 0.22.0` and `git push origin 0.22.0`.
2. In *Actions*, run **Release** from that tag, first with `dry_run` on (the
   default), then with it off.
3. Read the `Gate` and `Source archive` results, then approve the `release`
   Environment. Check that the commit shown in the approval prompt is the one you
   tagged: anyone who can push a tag of this form can start the workflow from it,
   and your approval is the control. Nothing reaches crates.io before it.

### What each job does

| Job | What it does |
|---|---|
| `Gate` | Fails unless: the run is from a tag (a branch is accepted only for a dry run from `main`); the tag is annotated and equals the workspace version exactly (any SemVer version, pre-releases such as `1.1.1-rc.1` included; a `v` prefix can never equal it); the commit is reachable from `main`; the newest `ci.yaml` run on the commit is `completed` with `success`; and `scripts/check_versions.py --expect <version>` passes |
| `Advisories`, `MSRV fresh resolution` | The weekly checks, run again at the moment of release, because an advisory or a new upstream release appears without any push |
| `Source archive` | `git archive` of the commit as `localcache-vX.Y.Z.tar.gz`, its SHA-256, and a build-provenance attestation |
| `Publish` | The only job in the `release` Environment, so the only one that can obtain a registry token. It runs `cargo publish --workspace --locked` (with `--dry-run` for a dry run) and records the `.crate` digests |
| `GitHub release` | Only when `dry_run` is off: a release named by the tag, with the version's `CHANGELOG.md` section, the archive, and the archive and `.crate` SHA-256 digests |

The tag's signature is made on the owner's machine and is **not** verified by
CI. To check an archive from a release:

```sh
gh attestation verify localcache-vX.Y.Z.tar.gz --repo nabbisen/localcache-rs
```

### Rehearsal

A dry run may also be started from `main`. It runs the whole chain (the gate on
its branch path, both reusable workflows, the archive and its attestation, the
identity exchange with crates.io for both crates, and `cargo publish --dry-run`)
and publishes nothing, so a mistake in the trust setup shows up before a release.
The rehearsal needs branch `main` to be allowed on the `release` Environment;
remove it when the rehearsal is done, so that afterwards only tags can deploy.

### If a publish stops half way

`cargo publish --workspace` uploads the crates in dependency order. If
`localcache` was uploaded and `localcache-cli` failed, **do not run the workflow
again**: the first crate's version already exists. Check crates.io, then publish
the missing crate by hand with the owner's own token, or release a new patch
version.

### One-time setup (owner)

- GitHub Environment `release`: the owner as required reviewer, with "prevent
  self-review" off (the owner both starts and approves), deployments limited to
  tags matching `*.*.*`. GitHub's name patterns are plain wildcards (no character classes), so
  this is only a coarse filter (it also admits SemVer pre-releases such as `1.1.1-rc.1`); the `Gate` job requires the tag to equal the workspace version exactly, which a `v` prefix never does.
- On crates.io, for each of `localcache` and `localcache-cli`, a trusted
  publisher: repository `nabbisen/localcache-rs`, workflow `release.yaml`,
  environment `release`. **Set the environment**, although crates.io marks it optional:
  without it, any job of any run of `release.yaml` (including one from a branch someone
  with write access pushed) that can request an identity token can obtain a registry
  token with no approval. With it, only a job that GitHub has let into the protected
  `release` environment can.
