# Q9c Fix Handoff — `release.yaml` Accepts Any SemVer Tag, and Says What It Does

RFC: `rfcs/accepted/027-release-engineering-on-standard-tools.md` (R5 and its 2026-10-31 revision).
Raised by the owner (2026-10-31): SemVer accepts `1.1.1-pre`, `1.1.1-beta-1`, `1.1.1-rc.1`; the gate's
`^[0-9]+\.[0-9]+\.[0-9]+$` rejects them. While checking, the architect found that the file's comments describe an approval
step that does not exist in the configured repository.
Format of the review request: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6.
Authorship: high-capability model. Authorized: the owner.

## 0. What this is

A small fix to `.github/workflows/release.yaml`, pushed to `main` **before v0.22.0 is released**, so that the first
pre-release the project makes does not meet a gate that refuses it. It is the only thing that may be pushed before the
release other than the architect's documentation.

## 1. The facts the file must state (verified by the architect on 2026-10-31)

- A real run starts only when the owner starts it with `dry_run` off. **There is no approval pause**: the `release`
  Environment has deployment rules (branch `main`, tags `[0-9]*.[0-9]*.[0-9]*`) and no required reviewer. A dry run
  (the default) ran end to end on 2026-10-31 (run 41208010709) with no pause.
- The crates.io trusted-publisher records name the repository and `release.yaml`; they do not name an environment.
- `publish` is the only job that asks crates.io for a token, and it runs in the Environment `release`.

## 2. The change

1. **Gate, step "Check the tag against the workspace version":** delete the `grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'`
   check. The rule: a tag run requires the tag to be **annotated** and to **equal the workspace version exactly**
   (string equality). Cargo guarantees the workspace version is valid SemVer, and equality is what rejects a `v`
   prefix. Keep one clear message for that case: if the tag starts with `v`, say "Tags carry no v prefix" before the
   generic mismatch message.
2. **`github-release`:** when the version has a pre-release part (it contains `-`), pass `--prerelease` to
   `gh release create`. Nothing else about the step changes.
3. **Comments:** correct the file header and any other comment so they say only what §1 says. Concretely: lines 3 to 5
   ("after an explicit, recorded approval by the project owner in the `release` Environment") and lines 14 to 17
   ("the only job that can obtain a registry token (the crates.io trusted-publisher records name that Environment)") are
   wrong; the tag is "a SemVer version (pre-releases included)", not "`0.22.0`". Run
   `grep -n -i "approv\|environment\|X\.Y\.Z" .github/workflows/release.yaml` before and after and report both.
4. **Nothing else.** Do not touch other workflows, scripts or the book (the architect owns the book text and has
   already rewritten it to match §1).

## 3. Demonstrations (the real step bodies, in scratch clones, as in the Q9c request)

With the workspace version set to each case in a scratch copy: tag `0.22.0` vs `0.22.0` passes; `0.22.0-rc.1` vs
`0.22.0-rc.1` passes; `1.1.1-pre` and `1.1.1-beta-1` the same; tag `0.22.0-rc.1` vs workspace `0.22.0` fails
(mismatch); tag `v0.22.0` fails with the `v` message; a lightweight tag fails; a branch run is unchanged. The
`github-release` step run with a fake `gh`: a pre-release version gets `--prerelease`, a final one does not. `actionlint`
exit 0. Re-run everything of the Q9c demonstrations that this edit touches.

## 4. Push mechanics (read carefully)

Your local branch holds unpublished v0.22.1 work, which must **not** be pushed yet. So: create a worktree from
`origin/main` (`git worktree add <dir under .git-exclude/tmp/> origin/main`), make the change there, run `actionlint`, the
step demonstrations and `mdbook build docs`, and after the architect's approval commit and push **from the worktree** as a
fast-forward of `main`. Then **dispatch a dry run from `main`** (`gh workflow run release.yaml --ref main -f dry_run=true`)
and report the run id and the result of each job. Remove the worktree. Never force-push. The rebase of your unpublished
commits onto the new `main` is the architect's, done after the release.

## 5. Not yours

The version, the tag, the date, the real run.
