# RFC 023 — MSRV Policy: When and How the Floor May Rise

| Field | Value |
|---|---|
| Status | Accepted (owner, 2026-09-23), with all three requested decisions as recommended: a 12-month age floor (R4); the 6-month, security- and corruption-only, on-demand previous line (R7); and `rusqlite 0.40.2` in v0.22.0, announced in v0.21.5 (R9) |
| Feature | *(workspace and release policy; no Cargo feature)* |
| Touches | `Cargo.toml` (`rust-version`, `rusqlite` requirement: under R9 only), `scripts/release.py` (`msrv` context), `.github/workflows/ci.yaml`, `docs/src/dependency_security.md`, `README.md`, `CHANGELOG.md`, `ROADMAP.md` |
| Finding | Phase 24 plan, milestone Q1 (owner, 2026-09-23): "the MSRV [is] raised only carefully, because this crate's consumers are application projects" |
| Milestone | Phase 24 Q1 |
| Breaking | **No.** This RFC changes no code and no MSRV. R9 schedules one breaking dependency change, the `rusqlite` line, for v0.22.0, which is already a breaking release; it is **not** an MSRV change |
| Authorship | High-capability model; **reviewed by the owner** (arrangement of 2026-08-01) |
| Extends | [RFC 014](../done/014-declared-msrv-and-dependency-security-policy.md), which fixed the declared MSRV at 1.85 but defined no rule for changing it |
| Handoffs | [`../handoffs/023-msrv-policy/`](../handoffs/023-msrv-policy/implementation-handoff.md): implementation handoff and QA checklist |

## Summary

localcache declares Rust 1.85 and has never had a rule for raising it. This RFC writes that rule
before any raise is discussed:
- a raise ships **only in a minor release**;
- it needs a **named necessity**, and goes to the **lowest** version that meets it;
- the new floor must be **at least 12 months old** on the release date, unless the necessity is a
  security fix;
- it is **announced one release ahead**;
- it is **verified three ways**, one of them new: a scheduled check that resolves dependencies
  fresh and builds them on the declared MSRV. That check catches the failure that broke
  `0.19.1` and `0.20.0`.

Applying the policy to today's facts gives an answer that differs from the one the roadmap
expected:
- **The MSRV stays 1.85.** Nothing a user depends on needs more.
- **`rusqlite 0.40` no longer requires Rust 1.95.** `libsqlite3-sys 0.38.2` (2026-08-08)
  replaced its use of the standard-library `cfg_select!` with a local polyfill. localcache on
  `rusqlite 0.40.2` passes all four declared-MSRV rows on 1.85.0, the full suite (459 tests),
  and clippy, with **no source change**.
- **Adopting 0.40 is still breaking**, for two reasons that have nothing to do with the
  toolchain: `rusqlite::Error` is part of the public error type, and `links = "sqlite3"` makes
  the `rusqlite` line a constraint every consumer shares. R9 therefore schedules it for v0.22.0,
  announced in v0.21.5.
- **`docs/src/dependency_security.md` is now wrong on two counts,** and it is published. R10
  corrects it without waiting for a release.

## Motivation

### The contract has no change procedure

RFC 014 R1 made "Rust 1.85 remains the contract" enforceable: four locked rows, run on the exact
declared toolchain, in CI and in the release gate. It deliberately did not say when 1.85 may
change. Its only alternative on the subject, "Raise MSRV to Rust 1.95", was rejected for a
corrective patch because "the owner has not authorized that compatibility change". Phase 24 exit
criterion 4 now forbids any change "without an approved MSRV policy (Q1) and explicit owner
authorization, and never in a patch release". This RFC is that policy.

### Who the consumers are

The crates.io reverse-dependency list, read on 2026-09-23:

| Consumer | Version (date) | Requires | Declares `rust-version` |
|---|---|---|---|
| `orbok`, `orbok-cache`, `orbok-workers` | 0.26.1 (2026-09-22) | `localcache ^0.21` | **1.91** |
| `arama-cache` | 0.36.2 (2026-07-09) | `localcache ^0.20` | **1.90** |
| `localcache-cli` (this workspace) | 0.21.4 | `^0` | 1.85 |

Both external consumers are applications, and both sit 11–12 months behind current stable
(1.98). An application can usually raise its toolchain more easily than a library can. But these
two already chose their floors, and a library that moves past them forces their hand.

### The failure this project has already had

`0.19.1` and `0.20.0` declared `rust-version = "1.85"` and did not build on it. A dependency with
no `rust-version` of its own (`libsqlite3-sys 0.38.x`) required 1.95, and the locked MSRV rows
did not exist yet. One consumer later found that its own real floor had been 1.95 all along,
because "Cargo only reports crates that *declare* `rust-version`"
(`docs/src/dependency_security.md`, "Recorded cases").

The locked rows RFC 014 added now prevent that for **this repository's lockfile**. They cannot
prevent it for a **consumer's fresh resolution**. A consumer resolves the newest
semver-compatible versions, and when a dependency declares no `rust-version`, Cargo's MSRV-aware
resolver cannot steer around it. The exact hazard is still live:
- neither `rusqlite` nor `libsqlite3-sys` declares a `rust-version` in any release through
  `0.40.2` / `0.38.2`;
- 0.38.0/0.38.1 → 0.38.2 shows the floor moving **within a patch series**: from 1.95 down to 1.85 or
  lower. A move in the other direction would be just as silent.

### Current dependency pressure — measured

`cargo update --dry-run --verbose` on `529c1d5`, with `resolver = "3"`, so MSRV-aware fallback
applies:

| Held back | Available | Why |
|---|---|---|
| `criterion 0.7.0` (dev-only) | 0.8.2 | **requires Rust 1.86**. The only MSRV-caused hold in the graph |
| `rusqlite 0.39.0` | 0.40.2 | our `^0.39` requirement (see below) |
| `aes-gcm 0.10`, `sha2 0.10`, `bincode 2`, `zstd 0.13`, `base64 0.22`, `opentelemetry 0.32`, `tracing-opentelemetry 0.33` | newer majors | semver-incompatible. Every newer major declares ≤ 1.85 (`aes-gcm 0.11.1` and `sha2 0.11.0`: 1.85; `bincode 3.0.0`: 1.85.0; the others lower) |

`clap 4.6.7`, the CLI's newest release, declares exactly **1.85**. It is the dependency nearest
the floor. The next `clap` minor may cross it, and the resolver fallback will then hold the CLI
at the last compatible release, as it does `criterion` now.

**Conclusion: nothing a user depends on needs more than 1.85 today.**

### The `rusqlite 0.40` question, re-measured

ROADMAP Q1 asked for "the `rusqlite 0.40` / Rust 1.95 question" to be evaluated against this
policy. The premise has changed:

| Evidence (2026-09-23) | Result |
|---|---|
| `libsqlite3-sys 0.38.0` and `0.38.1` `build.rs` | use the standard-library `cfg_select!` (Rust 1.95), with no polyfill |
| `libsqlite3-sys 0.38.2` `build.rs` (2026-08-08) | defines its own `macro_rules! cfg_select`, commented "Just to keep MSRV low" |
| `rusqlite 0.40.2` → `libsqlite3-sys` requirement | `^0.38.2` on every non-wasm target (`^0.38.1` on `wasm32-unknown-unknown`) |
| Minimal `rusqlite =0.40.2` (bundled, limits) | builds on **1.85.0**, 1.94.0, 1.95.0 |
| localcache with `rusqlite = "0.40.2"`: the four declared-MSRV rows on 1.85.0 | **all pass**, no source change |
| The same tree: `cargo test --workspace --all-features --locked` (stable) | **459 passed, 0 failed**, including the compatibility fixtures |
| The same tree: `cargo clippy --workspace --all-targets --all-features -D warnings` | clean |
| A fresh consumer of the published `localcache =0.20.0`, `rust-version = "1.85"`, on 1.85.0 | **builds**: it now resolves `rusqlite 0.40.2` / `libsqlite3-sys 0.38.2` |
| Bundled SQLite | `libsqlite3-sys 0.37.0`: **3.51.3**; 0.38.2: **3.53.2** |

The logs and the one-line manifest patch are under `.git-exclude/tmp/rfc023-evidence/`.
The minimal-crate bisection is under `.git-exclude/tmp/rfc023-sqlite-floor/`.

So the question is no longer "raise the MSRV to get `rusqlite 0.40`". It is "adopt a breaking
dependency line at the same MSRV", and that is a compatibility question (R8, R9), not an MSRV
one.

## Goals

1. One written rule for when, how far, and in which release the declared MSRV may rise.
2. No consumer's `cargo update` within a localcache minor line ever breaks their build on the
   MSRV we declared.
3. Detect a floor raised by an **upstream** crate before a consumer does, including crates that
   declare no `rust-version`.
4. Settle the `rusqlite 0.40` question on today's evidence, and correct the published
   documentation that no longer matches it.

## Non-goals

- **Raising the MSRV.** This RFC approves none.
- **A time-based "track stable minus N" schedule.** A raise needs a necessity (R3), never age
  alone.
- **Changing the four locked MSRV rows or the release gate's structure.** RFC 014 R8 and RFC 009
  stand. R6 adds a check; it removes none.
- **Changing `LocalFileCacheError`.** Whether `rusqlite::Error` should stay public is Q3's
  question (RFC 025). R8 records the input, and decides nothing there.
- **Filing an upstream issue** about `rust-version` in `rusqlite`/`libsqlite3-sys`. The
  project's position in `docs/src/dependency_security.md` stands.

## Terminology

- **Declared MSRV**: `[workspace.package].rust-version`, as in RFC 014.
- **Raise**: any change that makes the declared MSRV higher.
- **Necessity**: a reason from R3's closed list.
- **Release date of a Rust version**: the date its `x.y.0` was published on the stable channel.
- **Fresh resolution**: dependency resolution with no existing lockfile, as a new consumer
  experiences it.
- **Drift**: a fresh resolution that no longer builds on the declared MSRV, although the locked
  graph still does.

## Requirements

### R1 — The contract is unchanged in scope

The declared MSRV covers every package, every target, and every supported feature combination:
the four locked rows of RFC 014 R8, including development dependencies and benchmarks. A
dependency that needs a newer toolchain is held at a compatible version, replaced, or removed.
**It never justifies a raise on its own if it is development-only** (R3). This is how
`criterion 0.7` is held today.

### R2 — A raise ships only in a minor release

A raise ships only in a release that increments the leftmost non-zero version component. In the
current 0.x series, that is the **minor** (0.21 → 0.22). It never ships in a patch release.

Cargo treats 0.x minors as incompatible, so a consumer requiring `^0.21` never receives the raise
through `cargo update`. This is the same rule as "newly rejected input ships only in a minor
release" (Phase 24, authorized 2026-09-23), and for the same reason.

### R3 — A raise needs a named necessity, and goes no further than it

A raise is proposed only for one of these, named in the proposal with evidence:

1. **Security.** A RustSec advisory of kind `vulnerability` or `unsound`, or a CVE in bundled
   SQLite. The fix is available only in a dependency version that needs a newer toolchain, and
   there is no compatible fix.
2. **A runtime dependency's maintained line.** A non-development dependency's only maintained or
   correct release line needs a newer toolchain. An example: a correctness defect this project is
   affected by, fixed only there.
3. **A defect this project cannot fix otherwise.** A language or standard-library feature that
   removes a reproduced defect, or removes `unsafe` code that no stable alternative at the current
   MSRV can replace. Convenience, brevity, and "idiomatic" do not qualify.

The new MSRV is the **lowest** Rust version that satisfies the necessity. It is never the highest
version R4 would allow.

"The MSRV is old" is not a necessity. Nor is "stable has a nicer API", or a development tool's
requirement.

### R4 — Age floor: 12 months

On the release date, the new MSRV must have been released at least **12 months** earlier.

- For a **security** necessity (R3.1), the age floor does not apply. The raise still ships only in
  a minor release (R2). In 0.x a minor release can be cut whenever it is needed, so R2 does not
  delay a security fix.
- For any other necessity that needs a version younger than 12 months, the necessity **waits**
  until the version is old enough. Meanwhile the dependency is held (R1), and the hold is
  documented, as `rusqlite ^0.39` was from v0.20.1 to now.

Today the rule permits at most **1.90** (released 2025-09-18). That is at or below both external
consumers' declared floors, 1.90 and 1.91.

### R5 — Notice one release ahead

A non-security raise is announced in the `CHANGELOG.md` of the **release before** the one that
makes it. The announcement goes in its release summary, as the v0.22.0 input rejections were
announced in v0.21.4. It names:
- the new MSRV;
- the necessity;
- the release that will carry it.

`docs/src/dependency_security.md` carries the same notice from that release on.

A security raise (R3.1) is announced in the release that makes it, and says so explicitly.

### R6 — Verification: three checks, one of them new

1. **Locked rows (existing, RFC 014 R8).** The four rows on the exact declared toolchain with
   `--locked`, in CI and in the `msrv` release gate. Unchanged.
2. **Fresh-resolution drift check (new).** In a temporary copy of the workspace, with no
   `Cargo.lock`:
   - resolve with the **declared-MSRV** toolchain's Cargo, so that `resolver = "3"`'s MSRV-aware
     fallback applies exactly as it does for a consumer;
   - then run the four rows without `--locked`.
   
   It runs in two places:
   - **weekly on a schedule in CI**, where it fails visibly and the architect triages it;
   - **in the `msrv` release gate**, where drift blocks the release: that release would declare
     an MSRV that a new consumer cannot meet.
   
   The remedy for drift is never "ignore". It is one of:
   - constrain the offending requirement, as `^0.39` did;
   - or, if R3 is met, raise under this policy.
3. **Post-publication fresh consumer (existing practice, now required).** After every
   publication:
   - create a new crate with `rust-version` set to the declared MSRV;
   - depend on exactly the published version, with every feature enabled;
   - resolve it fresh, and build it on the declared toolchain.

   Report the resolved `libsqlite3-sys` version. The result is recorded in the release decision's
   outcome. v0.21.1–v0.21.4 ran this informally.

### R7 — The previous minor line after a raise

When a raise ships in 0.N+1, the previous line 0.N receives **security fixes** (R3.1 kinds) and
**data-loss or corruption fixes** for **6 months** from the 0.N+1 release date. This applies only
where the fix does not itself need a raise.

- The line's branch (`release/0.N`) is created only when the first such fix is needed.
- Each backport is an ordinary patch release, with the full gate set at 0.N's own declared MSRV.
- No other change goes to the old line.

If a security fix cannot be made at 0.N's MSRV (the R3.1 case itself), 0.N's documentation
says so plainly and names the fixed minor.

### R8 — `rusqlite` is a coordinated dependency

Two properties make the `rusqlite` line a compatibility boundary, independent of the MSRV:
- `LocalFileCacheError::Database(#[from] rusqlite::Error)` makes `rusqlite::Error` part of the
  public API;
- `libsqlite3-sys`'s `links = "sqlite3"` allows exactly one SQLite line per graph, so every
  consumer must share ours.

Therefore:
1. Changing the `rusqlite` minor line is a **breaking change**. It ships only in a minor release,
   with R5's notice.
2. The requirement names the **lowest patch such that every graph satisfying it meets the
   declared MSRV**, including a consumer's existing lockfile, not only a fresh resolution. For
   0.40 that is `"0.40.2"`:
   - a bare `"0.40"` leaves a lockfile holding `rusqlite` 0.40.1 with `libsqlite3-sys 0.38.1`
     valid, and that needs 1.95;
   - `"0.40.2"` forces `libsqlite3-sys` ≥ 0.38.2, so Cargo must update such a lockfile.
3. **Input to RFC 025 (Q3), not a decision here.** If `LocalFileCacheError` stopped exposing
   `rusqlite::Error` directly, future `rusqlite` bumps would stop being API-breaking. The `links`
   coordination would remain, so a bump would still warrant notice. Q3 weighs this against its
   own error-taxonomy goals.

### R9 — Application: adopt `rusqlite 0.40.2` in v0.22.0

Evaluated against R1–R8 on the evidence in the Motivation:

- **MSRV: unchanged at 1.85.** No R3 necessity exists.
- **`rusqlite = "0.40.2"` in v0.22.0.** It is not an MSRV change, so R3 and R4 do not apply. It
  is breaking under R8, and v0.22.0 is already the Phase 24 breaking release.
  - **Gain:** bundled SQLite 3.51.3 → 3.53.2. It also unblocks consumers who pin `rusqlite 0.40`
    directly. One of the two recorded cases asked for exactly that, and was declined when it
    would have cost Rust 1.95.
  - **Cost:** a consumer on `rusqlite 0.39` must move to 0.40 when it adopts localcache 0.22.
    One consumer moved to 0.39 to match this crate. The v0.21.5 notice (R5) gives it a release of
    warning.
- **Notice:** the v0.21.5 `CHANGELOG.md` summary announces it, together with v0.22.0's other
  breaking changes.
- **Verification:** the implementing slice shows:
  - all four locked rows on 1.85.0;
  - the full suite;
  - the compatibility fixtures;
  - the R6.2 drift check;
  - the SQLite version recorded in the CHANGELOG.

### R10 — Correct the published documentation now

`docs/src/dependency_security.md` is deployed to the project's Pages site from `main`, and it
now misleads:
- it says `rusqlite 0.40` "would raise this crate's MSRV from 1.85 to exactly 1.95". That is true
  only of `libsqlite3-sys` 0.38.0–0.38.1;
- it names `0.19.1` and `0.20.0` as broken on their declared baseline. A fresh resolution of
  either now selects `libsqlite3-sys 0.38.2` and builds on 1.85. They still fail from a
  **lockfile** holding `libsqlite3-sys` 0.38.0 or 0.38.1, and
  `cargo update -p libsqlite3-sys` repairs that.

Correct both, and keep the recorded cases as history. Add a short "MSRV policy" section
summarizing R2–R7. The change touches only `docs/src/`, so it ships to Pages on push without a
crate release, and it should not wait for v0.21.5. The owner's principle that public material
must not mislead users applies to the book as much as to the API.

## Detailed design

### Where each rule is enforced

| Rule | Enforced by |
|---|---|
| R1 scope | the existing four locked rows (`scripts/release.py msrv`, CI `msrv` job) |
| R2 minor only | architect review of the release plan; `version-contract` already rejects an unplanned version. A raise in a patch is a review rejection |
| R3, R4 necessity and age | the proposal (an RFC or RFC amendment) must state the necessity, the lowest satisfying version, and its release date. Owner authorization (exit criterion 4) |
| R5 notice | the release-preparation handoff of the release before the raise |
| R6.2 drift | new: a `--fresh` mode of the `msrv` context and a scheduled CI job (see below) |
| R6.3 fresh consumer | the release decision's outcome section |
| R7 old line | created on demand; recorded in ROADMAP when it exists |

### The drift check

A sketch for the implementing slice. The slice owns the exact interface.

```text
scripts/release.py msrv --fresh --output-dir <dir>
  1. copy the tracked tree (no Cargo.lock, no target/) to a temporary directory
  2. rustup run <declared toolchain> cargo generate-lockfile
  3. record the resolved versions of every package that declares no rust-version
  4. run the four RFC 014 R8 rows without --locked
  5. PASS / FAIL with the rows' logs as evidence; never touch the repository's Cargo.lock
```

In CI it runs as a separate `msrv-fresh` job on a weekly `schedule:` (and `workflow_dispatch`),
**not** on every push. A push-time failure caused by an upstream publication would block
unrelated work. It still runs in the release gate. Step 3 exists so that a failure names its
likely cause. The crates that declare no `rust-version` are exactly the ones the resolver cannot
protect a consumer from.

### What the check would have caught

While `libsqlite3-sys 0.38.1` was the newest release (2026-06-06 to 2026-08-08), a weekly R6.2
run on any `rusqlite ^0.40` tree would have failed within seven days, naming `libsqlite3-sys`
among the undeclared packages. `0.19.1` and `0.20.0` shipped with that class of failure.

## Test plan

- **Drift check, positive.** The current tree passes `--fresh` on 1.85.0.
- **Drift check, negative.** A fixture workspace pinning `libsqlite3-sys = "=0.38.1"` (with
  `bundled`) fails `--fresh` on 1.85.0 at its `cfg_select!`, and its evidence names
  `libsqlite3-sys` as undeclared. The pin must be on `libsqlite3-sys` itself: `rusqlite =0.40.1`
  requires `^0.38.1`, which a fresh resolution satisfies with 0.38.2 and then passes. This is
  the failing-before test: it reproduces the historical failure deterministically, whatever
  upstream publishes next.
- **The repository lockfile is untouched** by a `--fresh` run: its hash is the same before and
  after.
- **Script tests** run normally and under the restricted `PATH`, as for every `scripts/` change.
- **R9** evidence as listed in R9.

## Security considerations

- The age floor never delays a security fix (R4). R3.1 has no floor, and R2's minor-only rule
  costs nothing in 0.x.
- Holding a dependency for MSRV reasons can hold back fixes. The advisory gate (RFC 014 R4) scans
  held versions too, so a held version that becomes vulnerable fails the gate, and R3.1 then
  applies.
- The 0.39 line's bundled SQLite 3.51.3 has no advisory in the current scan. R9 moves to 3.53.2
  in v0.22.0 regardless.
- The drift check resolves from crates.io in CI. It adds no credential and no write access.

## Compatibility

This RFC changes no code, API, schema, wire format, or MSRV. R9's `rusqlite` change is breaking,
and it is scheduled for v0.22.0, which is already breaking. R10 changes documentation only.

## Alternatives considered

### A 6-month age floor (the roadmap's first proposal)

It would permit up to 1.94 today, and 1.95 from 2026-10-16. That is above both external
consumers' declared floors (1.90, 1.91), so a raise to the permitted maximum would force both to
move. It only matters when R3 is met, and R3.1 already covers the urgent case. For the non-urgent
cases, the owner asked for caution, and 12 months costs nothing measurable today (the Motivation
shows no pending necessity). **Rejected in favour of 12**, but it is a legitimate choice, and it is
the first decision this RFC asks of the owner.

### Track "stable minus N releases" automatically

This is common in the ecosystem, but it raises the floor on a calendar with no user benefit. It
would have moved localcache past 1.90 already, for nothing. **Rejected** by R3.

### No policy: decide each raise case by case

That is what the project has done, and it produced a declared MSRV that did not build
(`0.19.1`, `0.20.0`). A written rule is what makes "carefully" checkable. **Rejected.**

### Adopt `rusqlite 0.40.2` in v0.21.5

It is technically possible at 1.85, but it changes the public `rusqlite::Error` type and the
shared `links` line within `^0.21`. A consumer pinning `rusqlite 0.39` would fail to resolve
after a routine `cargo update`. **Rejected** by R8.1. It is the same principle as the
newly-rejected-input rule.

### Maintain no previous line after a raise

This is simpler, and applications can usually raise their toolchain. But a security fix is the
one change an application must be able to take without also taking a toolchain change. A
6-month, security- and corruption-only, on-demand line bounds the cost. **Rejected in favour of
R7**, and this is the second decision this RFC asks of the owner.

## Rollback

The policy is amended by an RFC amendment, like any accepted RFC. Before v0.22.0 ships, R9 is
withdrawn by deleting its slice from the plan and its notice from the v0.21.5 summary. After
v0.22.0, returning to `rusqlite 0.39` would itself be a breaking change, and it needs its own
decision.

## Decisions requested of the owner

**Decided 2026-09-23: all three accepted as recommended.**

1. **The age floor: 12 months (recommended) or 6.** R4.
2. **The previous-line commitment: 6 months, security and corruption fixes only, on demand
   (recommended), or none.** R7.
3. **Schedule `rusqlite 0.40.2` for v0.22.0, announced in v0.21.5.** R9.

The resulting slices, which the architect schedules and the owner authorizes with this RFC:

| Slice | Content | Ships |
|---|---|---|
| **Q1a** | R10: the `docs/src/dependency_security.md` correction and the policy summary | Pages, on push after review. No crate release |
| **Q1b** | R6.2: `msrv --fresh`, its tests, the `msrv-fresh` scheduled CI job, and its release-gate wiring | v0.21.5 (tooling; not breaking) |
| **Q1c** | R9: `rusqlite = "0.40.2"`, notice in v0.21.5 | v0.22.0 |

## Open questions

None. The three decisions above are settled.
