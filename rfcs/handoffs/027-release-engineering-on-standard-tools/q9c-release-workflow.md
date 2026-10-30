# Q9c Handoff — `release.yaml`: Owner-Approved Trusted Publishing (RFC 027 R5, R3, R4)

RFC: `rfcs/accepted/027-release-engineering-on-standard-tools.md` (accepted by the owner, 2026-10-29).
Slice: Phase 24 **Q9c**, the last of three. Order: Q9a, Q9b, Q9b-3 (all done, CI green on `983c62d`) → **Q9c** → Q3d.
Review-request format: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6.
Authorship: high-capability model. Authorized: the owner.

## 0. What this is, and what I verified

One new workflow, `.github/workflows/release.yaml`, that publishes both crates through crates.io Trusted
Publishing, after an explicit, recorded approval by the owner in a GitHub Environment. No long-lived registry
token is used by it. A real publish cannot be rehearsed, so the workflow has a **`dry_run` mode that exercises
the whole trust chain without publishing** (§ 3), and the owner runs it before v0.22.0.

The RFC's open question 2 asked that crates.io's behaviour be confirmed, not assumed. The architect read the
crates.io source (`rust-lang/crates.io`, `src/controllers/trustpub/tokens/exchange/mod.rs`) and the action:

| Fact | Source |
|---|---|
| The token lives **30 minutes**. | `ACCESS_TOKEN_LIFETIME = 30 minutes` |
| One exchange returns **one token for every crate whose trusted-publisher record matches** the repository owner, repository name, workflow file name and environment. Two crates, both configured for `release.yaml` + `release`, are published with **one** token by `cargo publish --workspace`. | `repo_configs … crate_ids` |
| A record with an `environment` matches only a job that runs in that environment. A job **without** the environment cannot obtain a token even if it has `id-token: write`. | the environment filter in the same file |
| Triggers `pull_request` and `workflow_run` are refused. `workflow_dispatch` is fine. | same file |
| A trusted-publisher record can be created only after a crate's first manual publish. Both crates exist. | RFC 3691; crates.io blog 2025-07-11 |
| The action's `post` step revokes the token when the job ends. Latest release `v1.0.6` (2026-10-21), commit `3268eb413133e2960fa4ae0e963ca76380c54cd9`. | `rust-lang/crates-io-auth-action` |
| Attestation: `actions/attest` `v4.2.2`, commit `1e69f48acb82d1966a394da916b4c1698aa569d6`; needs `id-token: write` and `attestations: write`. | its README |

**Verify both SHAs yourself** against the tagged release (`gh api repos/<owner>/<repo>/git/ref/tags/<tag>`)
and say how. Dependabot's `github-actions` ecosystem keeps them current afterwards.

A consequence for the RFC's wording ("the publish job is the only job with `id-token: write`"): the
attestation job also needs `id-token: write`. That does not weaken the control, because **the crates.io record
requires the `release` Environment**; only the job in that Environment can get a registry token. RFC 027 R5 is
amended accordingly (architect).

## 1. Owner actions (the dev team cannot do these; the architect lists them in the request)

1. **GitHub, Settings → Environments → New environment `release`:**
   - Required reviewers: the owner (`nabbisen`). "Prevent self-review": **off** (the owner both triggers and
     approves; with it on, nothing could ever be approved).
   - Deployment branches and tags: *Selected*, add tag rule `[0-9]*.[0-9]*.[0-9]*` (no `v`). For the
     rehearsal in § 3 also add branch `main`; remove it afterwards.
   - No secrets, no variables.
2. **crates.io, for each of `localcache` and `localcache-cli`, Settings → Trusted Publishing → Add (GitHub):**
   repository owner `nabbisen`, repository `localcache-rs`, workflow `release.yaml`, environment `release`.
3. **Keep the existing API token** in `~/.cargo/credentials.toml` until the first real workflow publish
   succeeds (v0.22.0). Then revoke it on crates.io (Account settings → API tokens) and delete it from the
   file. If crates.io offers a setting to refuse token-based publishing for a crate, enable it at that moment.

## 2. The workflow

`.github/workflows/release.yaml`, trigger **`workflow_dispatch` only**, run on a tag the owner has pushed
(**signed, no `v` prefix**: `0.22.0`). Input: `dry_run` (boolean, **default `true`**). No version input: the tag
is the version. Top-level `permissions: contents: read`; `concurrency: { group: release, cancel-in-progress:
false }`; `env: CARGO_TERM_COLOR: always`. Same R16 posture as `ci.yaml` (every action pinned to a full SHA
with a version comment; no `pull_request_target`; no secret). **Never interpolate `${{ … }}` of an input,
`github.ref_name` or any other external text into a script; pass it through `env:`.**

Jobs, in order:

| Job | needs | Permissions | Environment | What it does |
|---|---|---|---|---|
| `gate` | — | `contents: read`, `actions: read` | none | checks (below); output `version` |
| `advisories` | gate | `contents: read` | none | `uses: ./.github/workflows/advisories.yaml` (add `workflow_call:` to that file) |
| `msrv-fresh` | gate | `contents: read` | none | `uses: ./.github/workflows/msrv-fresh.yaml` (add `workflow_call:` to that file) |
| `archive` | gate, advisories, msrv-fresh | `contents: read`, `id-token: write`, `attestations: write` | none | `git archive` + digest + attestation (below) |
| `publish` | archive | `contents: read`, `id-token: write` | **`release`** | the only job that can obtain a registry token |
| `github-release` | publish | `contents: write` | none | only when `dry_run` is false |

**Why `advisories` and `msrv-fresh` are called here:** they are not part of `ci.yaml`; the advisory database
and a fresh resolution change without any push, so a release must see them at the moment of release.

**`gate`** (checkout with `fetch-depth: 0`; each check a separate, named step; fail closed):

1. `dry_run == false` requires `github.ref_type == 'tag'`. A branch is accepted **only** for a dry run and
   only if it is `main` (the rehearsal, § 3). The version is then the workspace version.
2. For a tag: matches `^[0-9]+\.[0-9]+\.[0-9]+$` (so `v0.22.0` fails), is an **annotated** tag object
   (`git cat-file -t`), and **equals the workspace version** in the root `Cargo.toml`. The signature is the
   owner's local act and is not re-checked here; say so in the page (§ 4).
3. The commit is reachable from `origin/main` (`git merge-base --is-ancestor "$GITHUB_SHA" origin/main`).
4. CI is green on that exact commit: `gh run list --workflow ci.yaml --commit "$GITHUB_SHA"` has at least one
   run and its newest run is `completed` with conclusion `success` (`GH_TOKEN: ${{ github.token }}`).
5. `python3 scripts/check_versions.py --expect "$VERSION"`.

**`archive`:** `git archive --format=tar.gz` of `HEAD` into `localcache-v$VERSION.tar.gz` (the `v` in the file
name only, files at the root); `sha256sum`; `actions/upload-artifact` (existing pin) of the file;
`actions/attest` with `subject-path`. It does not re-run `gate-archive`: CI ran it on this commit (gate step 4).

**`publish`:** checkout; the stable toolchain (existing pin); `rust-lang/crates-io-auth-action` with `id: auth`;
then `cargo publish --workspace --locked` (add `--dry-run` when `dry_run`) with `CARGO_REGISTRY_TOKEN: ${{
steps.auth.outputs.token }}` in that step's `env:` only. **Do not pipe it**: redirect to a file, remember the
status, print the file, exit with the status. Afterwards `sha256sum target/package/*.crate` into a file and
upload it as an artifact (existing pin). A failure after the first crate was uploaded leaves a partial
release; the page must say what the owner does (§ 4).

**`github-release`:** `if: ${{ !inputs.dry_run }}`; download the artifacts; the notes are the version's
`CHANGELOG.md` section plus the archive SHA-256 and the two `.crate` digests; `gh release create "$TAG"
--verify-tag --title "$TAG" --notes-file notes.md localcache-v$VERSION.tar.gz` (the `gh` CLI is on the runner;
no third-party release action). The release is named by the tag, without `v`.

## 3. Rehearsal (the substitute for testing a real publish)

After the owner has done § 1 (with branch `main` allowed), the owner dispatches `release.yaml` **from `main`
with `dry_run = true`** and approves the `release` Environment. Success means: `gate` (branch path), both
reusable workflows, `archive` and the attestation, the **OIDC exchange with crates.io for both crates** (the
auth step succeeds), and `cargo publish --workspace --locked --dry-run` all pass, and nothing is published.
The architect reviews the run. Any trust-chain mistake (wrong workflow name, environment, owner) shows up here
and not at the release.

A second rehearsal is **not** needed on the release tag: `dry_run=false` on the tag runs the same jobs plus
`github-release`.

## 4. The page and the gates

- **`docs/src/source_archives.md`, "Publishing"**, rewritten for what exists: the owner pushes a signed tag
  (`git tag -s 0.22.0`), dispatches `release.yaml` from it (first with `dry_run`, then without), approves the
  `release` Environment after reading the `gate` and `archive` results; what each job does; the archive
  digest and attestation (`gh attestation verify <file> --repo nabbisen/localcache-rs`); what to do after a
  partial publish (`localcache` uploaded, `localcache-cli` failed: do **not** re-run; check crates.io, then
  publish the missing crate by hand with the old token or by a new patch release); that the signature is not
  verified by CI. Remove "until Q9c" wording.
- No new public documentation outside that page and the workflow comments. `CHANGELOG.md` untouched.
- Gates: `cargo fmt --all --check`; `python3 -m unittest discover -s scripts/tests`; the existing `cargo
  make gate-*` tasks; `mdbook build docs`; an `actionlint` run on all workflows if you can install it
  pinned (report version and output; if you cannot, say so).

## 5. Demonstrations required in the request

Run the **real step bodies** extracted from the YAML (as the aggregator was demonstrated), in scratch clones,
with `GITHUB_*` variables set, and show exit codes:

1. a tag not on `main` fails gate check 3;
2. `v0.22.0` fails check 2; a tag different from the workspace version fails check 2;
3. a branch other than `main`, or any branch with `dry_run=false`, fails check 1;
4. a lightweight (unannotated) tag fails check 2;
5. a failing/absent CI run fails check 4 (use a SHA with no run);
6. a stale install example fails check 5;
7. `grep -n 'id-token' .github/workflows/*.yaml` lists exactly the `archive` and `publish` jobs of
   `release.yaml`; `grep -n 'environment:'` shows `publish` only; every `uses:` in `release.yaml` is a full SHA;
   no `pull_request_target`; no `secrets.` anywhere.
8. the registry token appears only in the `env:` of the publish step.

"An unapproved Environment never reaches `id-token`" is GitHub's behaviour, not ours; state that you rely on
it and that the rehearsal shows the approval prompt.

## 6. Push, then rehearsal

Scope: two small edits (`workflow_call:` triggers), the new workflow, the page, and the RFC note (architect).
One review request, `.git-exclude/review-request/NNN-dev-q9c-release-workflow-<date>.md`, describing the
**uncommitted** tree. After approval: commit and push (CI must stay green: it does not run `release.yaml`;
`advisories.yaml` and `msrv-fresh.yaml` keep their schedules). The architect then asks the owner for § 1 and
reviews the rehearsal. **Q3d starts after Q9c's review, not after the rehearsal** (the rehearsal needs only the
owner and the architect).

## 7. Not yours

The Environment and crates.io records (owner); any change to `ci.yaml` or a gate; the version, tags, a real
publish; product code; `CHANGELOG.md`; `ROADMAP.md`. Never put a token, e-mail address or personal data in a
file or a request. Scratch under `.git-exclude/tmp/`; `cd` only with relative paths after `pwd`.
