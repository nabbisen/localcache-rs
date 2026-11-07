# Q25 — Dependency freshness controlled by a release gate, with no timer

Authorized by the owner on 2026-11-07 ("Yes. Authorized."), after the owner rejected a monthly review and then a weekly one: **no schedule of any period may be what the project depends on.** Review 101 and request 075 (Q21) are the background: the architect had checked the lockfile, not the declared requirements, and called the dependencies clean when deps.rs showed otherwise.
Precedent for a gate script with tests: `scripts/check_versions.py`, `scripts/tests/`, `gate-versions` in `Makefile.toml`.
Format of the review request: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6.
Authorship: high-capability model.

## 0. The design in one paragraph

Freshness matters at two moments: when we release (what users and deps.rs see) and when an advisory is published against what we ship. The first becomes a **hard gate in the release path**; the second is **pushed to the owner** by GitHub Dependabot alerts (a repository setting, already enabled by the owner on 2026-11-07; you do not touch it). The gate compares every *declared* requirement of every workspace member with the newest stable version on crates.io. A dependency passes if its requirement admits the newest version, or if it has a row in a held-back file that says why, what ends the hold, and the newest version the row was written against. A newer version appearing makes the row fail, so an upstream event, not a date, brings it back for a decision. The weekly `advisories` and `msrv-fresh` workflows stay (the release reruns them), but **nothing in the project may be described as relying on them**.

## 1. Step 0 — evidence, scratch only, before any commit

Start now; it needs nothing from the v0.25.1 stack.

1. **Does a feed other than RustSec list CVE-2026-119375 for `libsqlite3-sys`?** Query, read-only, OSV.dev (`https://api.osv.dev/v1/query` for `libsqlite3-sys` at `0.38.2` and at `0.39.1`) and GitHub's advisory database (`gh api` on the advisories endpoint by CVE id and by ecosystem `rust`). Send nothing but the package name, version or CVE id. Report the raw answers, the aliases they carry, and the date each entry was published. This decides what the book may say about the gap (§ 4); it is not an input to the gate.
2. **A prototype of the check** against the current tree (§ 2), run with no commit: list every dependency it reports and compare with deps.rs for `localcache 0.25.0` and with the three held rows of `docs/src/dependency_security.md` (`rusqlite`, `criterion`, `sha2`). Report every difference.
3. **Choose how to compare a requirement with a version, with reasons.** (a) A pinned `cargo-outdated`-style tool: report whether one exists that is maintained, installs with `--locked` on the MSRV-independent CI toolchain, and exits non-zero on staleness; (b) a script in the style of `check_versions.py` (standard library only) over `cargo metadata` and the crates.io sparse index, with a small requirement matcher for the operators the tree actually uses, **failing closed on any operator or form it does not understand**. The architect's expectation is (b); report if (a) is better.

## 2. What the gate does

* **Inputs.** `cargo metadata --locked --format-version 1` for every workspace member: every dependency of every kind (normal, build, development), optional and target-specific included, with the requirement as declared (workspace-inherited ones resolved). Direct dependencies only. The lockfile is not the subject: a user resolves fresh.
* **Latest.** The newest version on the crates.io sparse index (`https://index.crates.io/…`) that is not yanked and not a pre-release. A declared requirement that names a pre-release fails closed. Use a generic `User-Agent` naming the project; no personal address.
* **Pass** when the requirement admits the newest version. **Fail** when it does not and the dependency has no row, when a row's `latest` is not the newest version now, when a row exists for a dependency that now passes (delete it, as `advisory-not-detected` does), and always when the index cannot be fetched or parsed: **no silent fallback, no cached copy** (the same rule as the advisory gate).
* **Output.** One line per failure: name, declared requirement, newest version, and the exact next step (update the requirement, or add or correct a row).
* **The held-back file.** One tracked file at the repository root, `requirements-held.toml`, TOML, schema checked, unknown keys fail. Each entry: `crate`, `requirement` (as declared), `latest` (the version seen), `why`, `ends` (the condition, in the style of the `deny.toml` follow-ups). It replaces the hand-kept table in `docs/src/dependency_security.md` as the single source of truth: that page's table is checked against the file by the same script (names, requirements, latest), or generated from it, whichever you can make fail closed with less code. State the choice.
* **Tests** in `scripts/tests/`, with a fixture index and fixture metadata and no network: stale and unlisted (fails); listed with the current `latest` (passes); listed with an older `latest` (fails); listed but now satisfied (fails); index unreachable (fails); a requirement operator the matcher does not know (fails); a pre-release requirement (fails); a yanked newest version is skipped (passes against the next).

## 3. Where it runs

* `cargo make gate-requirements` in `Makefile.toml`, documented in its header list.
* The `Release` workflow's gate, **required** for the release to proceed, and the book's release checklist (`docs/src/source_archives.md`).
* The architect names it in the gate list of every handoff that touches a dependency or a version.
* **Not** in `ci.yaml` (an upstream publication between two pushes must not turn an unrelated push red) and **not** on a schedule. No new workflow.

## 4. The book

* `docs/src/dependency_security.md`: a section "How freshness is controlled" with the design of § 0 in the project's plain style; the limits stated: nothing detects staleness in a repository that is idle and silent, and that costs nothing until a release, which the gate stops; and what the advisory sources do and do not cover, **as step 0 item 1 found it** (do not write more than the evidence supports).
* Rewrite the sentences that make a schedule the control: `dependency_security.md` ("The weekly run is the point", "which is why the weekly fresh-resolution run stays", the MSRV verification bullet), `docs/src/security.md` ("every push and every week"), `docs/src/source_archives.md` (the `Advisories` / `MSRV fresh resolution` row), `deny.toml` line 3, the comments of `advisories.yaml` and `msrv-fresh.yaml`. They may say the workflows exist and run weekly; they must not say the project depends on them. RFC 023's text is a record of a decision: leave it.
* Dependabot alerts for Cargo dependencies: the pages already call them "a repository setting". Say what they are for (GitHub tells the owner when an advisory is published against the locked graph), and say that they **are enabled** (the owner enabled Dependabot alerts on 2026-11-07; Dependabot security updates stay off, with the reason: an automatic bump does not know the MSRV, the `links` constraint or the exact `localcache-format` pin).
* The existing `.github/dependabot.yml` (GitHub Actions only, weekly pull requests) is not changed here. Mention it nowhere as a freshness control for crates.

## 5. Order and the push

Develop on a local branch off `main`; **do not push or merge before v0.25.1 is released**, so the v0.25.1 candidate and its release dry run are not disturbed. After the release, rebase, run all twelve gates and `gate-requirements`, and report. Nothing here changes a version, the CHANGELOG's released sections, library code or the MSRV. The CHANGELOG `[Unreleased]` section gets one *Internal* line when this lands in v0.26.0's stack.

## 6. The request

One review request (step 0 may be reported first as its own request, so the architect can answer item 1 and item 3 before the implementation): the evidence of § 1 with raw answers; the choice of § 1.3; the commits; the fixture-test names with a negative control that fails when the check is broken; the output of the gate on the current tree; the gates.

## 7. Not yours

The Dependabot settings, any change to workflow permissions, the date, tags and publication, `ROADMAP.md`, `docs/src/roadmap.md`, any removal of a feature (v0.26.0 is separate).
