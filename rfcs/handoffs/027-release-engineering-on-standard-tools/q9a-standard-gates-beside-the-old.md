# Q9a Handoff — Standard Gates Beside the Old Ones (RFC 027 R2, R6.1)

RFC: `rfcs/accepted/027-release-engineering-on-standard-tools.md` (accepted by the owner, 2026-10-29).
Slice: Phase 24 **Q9a**, the first of three (Q9a adds, Q9b deletes, Q9c adds `release.yaml`).
Order for the dev team: **Q9a → Q9b → Q9c → Q3d** (RFC 025 Part B, `rfcs/handoffs/025-…/q3d-opaque-database-error.md`).
Q9 goes first because its workflow changes can only be verified by a push, and from Q3d on, v0.22.0 work
stays in local commits until the release preparation.
Review-request format: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6.
Authorship: high-capability model. Authorized: the owner.

## 0. What this is

RFC 027 replaces the *mechanisms* of RFC 009 with standard tools and keeps its goals. The method is a
parity proof: the new gate and the old gate run on the **same trees** and must agree. **Q9a deletes
nothing and changes no required check.** If a step seems to need a deletion, or to change what an existing
gate checks, stop and file a design request.

The owner's rule for this work: *keep it simple, standard-oriented, clean and easy to understand; don't
make it complicated or messy.* A new gate that is as clever as the one it replaces has failed.

## 1. Deliverable 1 — `deny.toml` and the advisory parity (R2, "Dependency security")

Add `deny.toml` at the repository root:

- `[graph]` **`all-features = true`** (the optional dependencies — `async-std`, `smol`, `notify`,
  `bincode`, … — must be in the graph; the old gate reads all of `Cargo.lock`). Show, with a run, what
  happens with it off, and keep what matches the old gate.
- `[advisories]`: `version = 2`, `yanked = "deny"`, `unmaintained = "all"`, **`unsound = "all"`**
  (the default scope hid 10 of 11 unsound reports, review 031 § 2), and one `ignore` entry per standing
  disposition in `security/advisory-policy.json`: `RUSTSEC-2025-0052` (`async-std`) and `RUSTSEC-2025-0141`
  (`bincode`), each `reason` carrying that disposition's reason **and** its follow-up condition, verbatim.
- `[sources]`: `unknown-registry = "deny"`, `unknown-git = "deny"`.
- Run `cargo deny check advisories sources`. Pin the `cargo-deny` version you use (the installed one is
  0.20.2) and say whether it is the version CI will install.

**Parity runs** (the evidence is two trees, each through both gates):

| Tree | Old gate: `python3 scripts/release.py security --output-dir <new empty dir>` | New gate |
|---|---|---|
| `24544bc` (exported with `git archive` into a new empty directory under `.git-exclude/tmp/`) | exit 1, `denied=15` (11 unsound + 4 vulnerabilities; the ids are in `.git-exclude/reviewed/028-…`) | must exit 1 with **the same 15 advisory ids** |
| the current tip | exit 0, `denied=0`, `warnings=2` | must exit 0; the two ignores show as expected, nothing else |

Compare the **sets of advisory ids**, not the counts. Any difference is a finding: report it, do not
tune the configuration until it disappears.

Two more behaviours to establish and report, each with a run:
1. **A stale ignore.** Add an `ignore` for an advisory that is not in the graph. The old gate fails closed
   on a stale exception; `cargo deny` is expected to warn (`advisory-not-detected`). Find out whether the
   tool can make that warning fatal (read its documentation for lint-level flags; do not guess). If it can,
   use it in the command. If it cannot, say so plainly: RFC 027 records the loss.
2. **Offline behaviour.** State what the new gate does when the advisory database cannot be fetched, and
   whether it can silently pass. The old gate fails closed. Report the exit code of a run with the
   database unreachable (for example, an invalid `db-urls`/offline environment in a scratch copy), not an
   assumption.

## 2. Deliverable 2 — the feature matrix on `cargo hack` (R2, "Feature matrix")

Install `cargo-hack` (pinned, `cargo install cargo-hack --version X --locked`; report the version).
Define the gates as **literal commands** in `Makefile.toml` (new tasks only; the existing tasks stay) and
in a **new CI job** that runs those same commands:

- `cargo hack --each-feature` for the library, plus an all-features row, plus the RFC 009 rows for
  `async-std` and `smol` without Tokio, plus the CLI rows and the workspace doctest;
- clippy with `-D warnings` and tests per row, as the old matrix does.

**Parity table (required).** One row per row of `python3 scripts/feature_matrix.py --list` (17 rows
today). Columns: old row name, its package and feature set, the new command, and whether the feature set is
identical. Differences are findings. `cargo hack --each-feature` also includes a no-default-features and a
default row depending on flags; state exactly what it runs and reconcile it with `lib-no-features`.

The unassigned-feature check (`feature_matrix.py --check-coverage`) is replaced by **one assertion** that
the feature list in `crates/localcache/Cargo.toml` equals the list the new CI rows name. Make it a few
lines, show it failing when you add a dummy feature (revert afterwards), and place it where the old check
runs.

The new job lives in `.github/workflows/ci.yaml` **beside** the old matrix. It is **not** added to
`aggregate-ci`'s `needs` or to `release-gate` in Q9a: Q9b flips that once parity is proven. It must obey
RFC 009 R16 (read-only default permissions, every third-party action pinned to a full commit SHA with the
version comment, no `pull_request_target`, no secrets). If you add a pinned action, verify the SHA equals
the tagged release and say how; Dependabot (`.github/dependabot.yml`) keeps the pins current afterwards.

**Demonstrate a deliberately failing row**: add a compile error behind one feature, show the new job's
command failing for exactly that row (locally, with the log), and revert.

## 3. What Q9a does not do

No deletion of any script, test, policy file or workflow step. No change to `aggregate-ci`,
`release-gate`, `scripts/release.py`, `scripts/release-tools.toml`, or the `security`/`msrv` steps. No
release action. No change to `docs/src/` beyond a one-paragraph note in `docs/src/dependency_security.md`
**only if** the new gate is referred to there (Q9b rewrites that page). No product code. `CHANGELOG.md`
is untouched: nothing here is user-visible.

## 4. Gates and the request

- `cargo fmt --all --check`; `python3 scripts/source_integrity.py --require-tracked`; the existing script
  tests (they must pass **unchanged**); `python3 scripts/feature_matrix.py --check-coverage` and
  `--run-all` (the old gate must still pass); the new commands from § 1 and § 2.
- Because the workflow changes can only be verified by CI, **after the review approves it, commit and push
  this slice** (the owner allows dev-team pushes of approved work). The architect then confirms the
  new job runs green in CI on the pushed commit and that the old jobs are unchanged.
- File the request as `.git-exclude/review-request/NNN-dev-q9a-standard-gates-<date>.md` (next free
  number), describing the **uncommitted** tree: the two parity tables, the `deny.toml` runs with their
  exit codes (logs under `.git-exclude/tmp/`), the stale-ignore and offline findings, the deliberate
  failing row, and every judgement call. List every required step, including the ones that pass.
- Scratch belongs under `.git-exclude/tmp/`; use `cd` only with relative paths and verify with `pwd`; do
  not pipe a command whose exit code matters.
