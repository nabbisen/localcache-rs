# Q9c Fix Handoff — The Release Notes Must List the Two `.crate` Digests

RFC: `rfcs/accepted/027-release-engineering-on-standard-tools.md` (R4: the release notes list the archive digest and the
two `.crate` digests). Found at the first real run of `Release` (v0.22.0, run 41214060136).
Format of the review request: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6.
Authorship: high-capability model. Authorized: the owner (as part of RFC 027).

## 0. The defect, with its evidence

In the real run, the `Publish` job's step "Record the .crate digests" ran
`for f in target/package/*.crate; do printf '%s  %s\n' "$(sha256sum < "$f" | cut -d' ' -f1)" "$(basename "$f")"; done`
and wrote **`  *.crate`**: the glob matched nothing, `sha256sum` failed silently inside the command substitution, and the
loop printed one malformed line. The release notes therefore listed `*.crate` where the two digests belong. A dry run keeps the
packaged `.crate` files in `target/package`; a real `cargo publish` does not. The step exited 0, so nothing flagged it. The
architect repaired the published notes by hand with digests verified against crates.io (2026-10-31).

## 1. The change (`.github/workflows/release.yaml`, `Publish` job and `github-release`)

1. **The step fails closed.** It must exit non-zero unless it recorded exactly two digests, each 64 hexadecimal characters,
   for exactly `localcache` and `localcache-cli` at the released version. A step that can write garbage and succeed is the
   defect; fix that first.
2. **Where the digests come from.** For a real run (`dry_run` false): the bytes crates.io serves, since that is what users
   download: `https://static.crates.io/crates/<name>/<name>-<version>.crate`, fetched with `curl --fail --silent --show-error
   --location` and a bounded retry (the file can lag the publish by seconds; at most 10 tries, 6 s apart), then `sha256sum`.
   For a dry run: the `.crate` files in `target/package` (they exist there), so the dry run exercises the same recording code
   path's output format. Say in a comment why the two sources differ.
3. The artifact `crate-digests` and the `github-release` notes block keep their format (`<64 hex>  <name>-<version>.crate`,
   one per line). `github-release` must also refuse to create the release if the digests file does not have exactly two
   lines.
4. Nothing else changes: no version, no other job.

## 2. Demonstrations

The real step body, in a scratch clone: (a) a `target/package` with two fake `.crate` files and `dry_run=true` records two
correct digests; (b) an empty `target/package` and `dry_run=true` **fails** (this is the case that passed silently); (c) a real run
with a fake `curl` that serves two files records two correct digests; a fake `curl` that fails ten times then succeeds
passes; one that always fails **fails** the step; (d) the `github-release` guard rejects a digests file with one line, with
`*.crate`, and with a short hash. `actionlint` exit 0. Then, after the fix is on `main`, a dry run from `main` (green) is the
dev's evidence; the real path is first exercised by the v0.22.1 release, and the request says so.

## 3. Push mechanics

As in `q9c-fix-semver-tags.md` § 4 (a worktree from `origin/main`; push only after the architect's approval; the dry run from
`main` afterwards; never force-push; your unpublished v0.22.1 commits stay untouched, the architect rebases them).

## 4. Not yours

The v0.22.1 version bump and release preparation (a later handoff); editing the published v0.22.0 notes (done).
