# Q9c Fix Handoff — `release.yaml` Accepts Any SemVer Tag (RFC 027 Amendment 1)

RFC: `rfcs/accepted/027-release-engineering-on-standard-tools.md`, **Amendment 1**. Raised by the owner (2026-10-31):
SemVer accepts `1.1.1-pre`, `1.1.1-beta-1`, `1.1.1-rc.1`; the gate's `^[0-9]+\.[0-9]+\.[0-9]+$` rejects them.
Format of the review request: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6.
Authorship: high-capability model. Authorized: the owner.

## 0. What this is

A small fix to `.github/workflows/release.yaml`, then a **push of that one commit to `main`** (the workflow must be
on `main` to be dispatched). It is the only thing that may be pushed before v0.22.0 is released.

## 1. The change

1. **Gate, step "Check the tag against the workspace version":** delete the `grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'`
   check. The rule is: a tag run requires the tag to be **annotated** and to **equal the workspace version
   exactly** (string equality). Cargo guarantees the workspace version is valid SemVer, and equality is what rejects a
   `v` prefix (`v0.22.0` never equals `0.22.0`). Keep one clear message for that case: if the tag starts with `v`,
   say "Tags carry no v prefix" before the generic mismatch message.
2. **`github-release`:** when the version has a pre-release part (it contains `-`), pass `--prerelease` to
   `gh release create`. Nothing else about the step changes.
3. **Comments and the file header:** wherever `release.yaml` says the tag is `X.Y.Z`, say "a SemVer version
   (pre-releases included)".
4. **A comment that is no longer true.** The file header says `publish` is "the only job that can obtain a registry
   token (the crates.io trusted-publisher records name that Environment)". The owner decided the records do not name the
   environment (RFC 027 Amendment 1). Reword: `publish` is the only job that **requests** a registry token and the only
   one that runs in the `release` Environment, where the owner approves. No other comment says otherwise (`grep`).
5. Nothing else. The Environment's tag rule is `*.*.*` (the owner's setup); `check_versions.py` already accepts any
   version string; do not touch other workflows or scripts.

## 2. Demonstrations (the real step bodies, in scratch clones, as in the Q9c request)

With the workspace version set to each case in a scratch copy: tag `0.22.0` vs `0.22.0` passes; tag `0.22.0-rc.1` vs
workspace `0.22.0-rc.1` passes; `1.1.1-pre` and `1.1.1-beta-1` the same; tag `0.22.0-rc.1` vs workspace `0.22.0` fails
(mismatch); tag `v0.22.0` fails with the `v` message; a lightweight tag fails; a branch run is unchanged. The
`github-release` step run with a fake `gh`: a pre-release version gets `--prerelease`, a final one does not. `actionlint`
exit 0. Everything else of the Q9c demonstrations that this edit touches is re-run, not reused.

## 3. Push mechanics (read carefully)

Your local branch holds unpushed v0.22.1 work and the architect's documentation commits, which must **not** be pushed
yet. So: create a worktree from `origin/main` (`git worktree add <dir under .git-exclude/tmp/> origin/main`), make the
change there, run the gates that apply (`actionlint`, the step demonstrations, `python3 scripts/check_versions.py`,
`mdbook build docs`), and after the architect's approval commit and push **from the worktree** as a fast-forward of
`main`. Then, in your main checkout, `git rebase origin/main` (your and the architect's unpushed commits sit on top; the
rebase is of unpushed commits only; report any conflict instead of resolving it silently). Remove the worktree.
Confirm CI is green on the pushed commit and report the run id. Never force-push.

## 4. Not yours

The Environment, the crates.io records, the dry run (owner/architect); the version, the tag, the date.
