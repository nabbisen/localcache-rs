# MSRV and Dependency Security

The workspace declares and tests Rust 1.85 as its minimum supported Rust
version (MSRV). A successful build on current stable Rust does not substitute
for this contract. Dependency changes must preserve all four locked MSRV rows:

```sh
cargo check -p localcache --all-targets --all-features --locked
cargo check -p localcache --all-targets --no-default-features --features async-std --locked
cargo check -p localcache --all-targets --no-default-features --features smol --locked
cargo check -p localcache-cli --all-targets --all-features --locked
```

They are one task, run on the declared toolchain: `rustup run 1.85.0 cargo make
gate-msrv`. CI runs the same task on Rust 1.85.0.

The separate async-std and smol rows are required because enabling all
features selects Tokio by runtime priority. `--all-targets` also keeps
benchmarks and development dependencies within the MSRV contract.

## MSRV policy

The declared MSRV changes only under a written rule
([RFC 023](https://github.com/nabbisen/localcache-rs/blob/main/rfcs/accepted/023-msrv-policy.md)):

- **Minor releases only** (0.21 → 0.22), never a patch, so `^0.21` never receives a
  raise through `cargo update`.
- **A named necessity, and no further.** Only a security fix that needs a newer
  toolchain, a runtime dependency whose only maintained line does, or a defect with
  no other fix. The new MSRV is the lowest version that meets it. "The MSRV is
  old" is not one, and a development-only dependency is held back instead.
- **A 12-month age floor** on the new MSRV, waived for security necessities.
- **Notice one release ahead** in `CHANGELOG.md` and on this page; a security
  raise is announced in the release that makes it, and says so.
- **Verification:** the four locked rows above (`gate-msrv`) in CI on every push,
  and after each publication a fresh consumer crate with `rust-version = "1.85"`
  builds the published version on the declared toolchain. A fresh-resolution check —
  the tracked tree without its lockfile, resolved by the declared toolchain's
  Cargo, then the same four rows — runs weekly and on request
  (`.github/workflows/msrv-fresh.yaml`); run it before a release, and a drift there
  blocks the release.
- **The previous minor line** gets security, data-loss, and corruption fixes for
  6 months after a raise, where the fix does not itself need the raise.

## Why the `rusqlite` requirement is `0.40.2`

`localcache` 0.22 requires `rusqlite 0.40.2`, not `0.40` and not the newer
`0.41`; `localcache` 0.21.x required `rusqlite ^0.39`. In Cargo's notation
`0.40.2` means at least that patch release and below 0.41, so it is a floor
inside the 0.40 line. Both choices were deliberate, they are the constraint most
often asked about, and they are worth understanding before filing a request to
change them.

### The chain

```text
rusqlite 0.39  ->  libsqlite3-sys 0.37.x   (bundles SQLite 3.51.3)
rusqlite 0.40  ->  libsqlite3-sys 0.38.x   (bundles SQLite 3.53.1 to 3.53.2)
```

Whether `rusqlite 0.40` builds on Rust 1.85 depends on the exact patch release,
not on the line. `libsqlite3-sys 0.38.0` and `0.38.1` use the standard library's
`cfg_select!` in their build script, which became stable in **Rust 1.95**;
`0.38.2` defines its own `cfg_select` macro instead. `rusqlite 0.40.0` and
`0.40.1` also use `cfg_select!` in their own source; `0.40.2` polyfills it and
requires `libsqlite3-sys ^0.38.2` (on every target except
`wasm32-unknown-unknown`, where it is `^0.38.1`).

Measured 2026-09-23 (RFC 023's evidence) and re-verified 2026-09-24, each crate
built alone with `features = ["bundled"]`:

| `libsqlite3-sys` | Published | 1.85.0 | 1.94.0 | 1.95.0 |
|---|---|---|---|---|
| 0.38.0 | 2026-05-26 | fails — `cannot find macro cfg_select` | fails — `cfg_select` is unstable | passes |
| 0.38.1 | 2026-06-06 | fails — same | fails — same | passes |
| **0.38.2** | 2026-08-08 | **passes** | passes | passes |

With `libsqlite3-sys 0.38.2` locked, `rusqlite =0.40.0` and `=0.40.1` still fail on
1.85.0 with the same error; `rusqlite =0.40.2` builds there.

### Where this leaves `localcache`

- **`localcache 0.21.x` kept `rusqlite ^0.39`** — for compatibility, not because
  of the toolchain. `rusqlite::Error` was part of this crate's public
  `LocalFileCacheError::Database` variant, and `links` (below) allows one SQLite
  line per dependency graph, so changing the line was a breaking change that
  could ship only in a minor release. v0.22.0 made the error opaque, so
  `rusqlite::Error` is no longer part of the public API; the `links` constraint
  remains.
- **`localcache 0.22.0` moved to `rusqlite 0.40.2`**, with the declared MSRV still
  1.85. The requirement names `0.40.2`, not a bare `0.40`, so that no lockfile can
  keep `rusqlite 0.40.0`/`0.40.1` and `libsqlite3-sys 0.38.0`/`0.38.1`, which need
  1.95.
- **`rusqlite 0.41` is not used.** It declares Rust 1.89 and requires
  `libsqlite3-sys ^0.39`, so adopting it would raise the MSRV, and RFC 023 allows
  that only for a named necessity; there is none. A crate that depends directly
  on `rusqlite 0.41` therefore cannot be combined with `localcache 0.22`
  (`links`, below).
- **Consumers who pin `rusqlite 0.40` directly** can use `localcache` from 0.22.0.

The gain was a newer bundled SQLite (3.51.3 → 3.53.2, checked on the built
crate: `rusqlite::version()` prints `3.53.2`). While `localcache` stayed on
`^0.39`, no advisory affected the older one, and the advisory check below scans
`libsqlite3-sys` along with everything else, so a later one would have surfaced
there.

### Why this cannot be worked around downstream

`libsqlite3-sys` declares `links = "sqlite3"`, and Cargo permits **exactly one**
package with a given `links` value in a dependency graph. Two `rusqlite` lines are
therefore not a tolerable duplicate — they are a hard resolution failure.

The practical consequence: a crate depending directly on a different `rusqlite`
line than `localcache` cannot be combined with it, and **no lockfile entry,
`--precise` pin, or feature flag at the consumer's end can resolve it**. With
`localcache 0.22` your own `rusqlite`, if you depend on it directly, must be
`0.40.2` or later in the 0.40 line. On `localcache 0.21.x` or earlier, which
required `^0.39`, a crate on `rusqlite 0.40` had to move its own `rusqlite` to
0.39 or wait for 0.22.0. You can also tell us — see below.

### The upstream cause

Neither `rusqlite` nor `libsqlite3-sys` declares a `rust-version` in its manifest.
Because of that, Cargo's MSRV-aware resolution cannot see a toolchain requirement
and cannot route around it.

The `rusqlite 0.40.x` and `libsqlite3-sys 0.38.x` series show why that matters:
the floor moved **within a patch series**, up (`rusqlite 0.40.0`/`0.40.1` and
`libsqlite3-sys 0.38.0`/`0.38.1`) and then back down (`rusqlite 0.40.2` and
`libsqlite3-sys 0.38.2`), invisibly to Cargo's MSRV-aware resolver. A consumer's
build could break, or be repaired, with no change to anything they declared. That is why RFC 023 adds a check of a
*fresh* resolution, rather than trusting the locked graph alone.

It is still so for `rusqlite 0.40.2`. Its release notes say "Lower MSRV to
1.88.0", but its manifest declares no `rust-version`, and the declared-MSRV rows
(`cargo make gate-msrv`) and the fresh-resolution check both pass on Rust 1.85.0
with it. What upstream states and what the build needs are not the same thing,
which is why the weekly fresh-resolution run stays.

We have not filed an upstream issue. Doing so would open a conversation this project
would need to sustain, and the constraint is documented here regardless. If you have hit
this and intend to report it upstream, that would help everyone — and we would link it
here.

### Recorded cases

Two dependent applications hit this within one week of each other, from opposite
directions. Kept here as a short reference, because the pattern recurs and the
right answer differed in each case.

| Date | Reported | Resolution |
|---|---|---|
| 2026-08-01 | A declared `rust-version = "1.85"` that the graph could not meet, because `rusqlite ^0.40` pulled `libsqlite3-sys 0.38.x`. | Fixed here: `rusqlite` constrained to `^0.39` in v0.20.1, making the declared 1.85 genuine. |
| 2026-08-01 | Blocked at `localcache 0.20.0`: the project pins `rusqlite 0.40` directly, so `^0.39` made every later version unresolvable. Requested `>=0.40`. | Declined — it would have raised this crate's MSRV to 1.95. The project moved its own `rusqlite` to 0.39 instead, having discovered its real floor was already 1.95 for the same reason. |

On 2026-08-08, `rusqlite 0.40.2` and `libsqlite3-sys 0.38.2` (both published that
day) removed the Rust 1.95 requirement, so the second case's request (`rusqlite >= 0.40`) is met in `localcache 0.22.0`,
without an MSRV change.

The second case is the more instructive one: the reporter believed their floor was
around 1.88, because Cargo only reports crates that *declare* `rust-version` — and
`libsqlite3-sys` declares none. The constraint had been invisible to them the whole
time.

The first case's fix was not retroactive: `0.19.1`, published before v0.20.1,
carried the same defect and was identified later by the same reporter. Neither
already-published version was repaired by the v0.20.1 fix — which is why both are
named below rather than treated as resolved.

### Affected published versions

**When they were published, `0.19.1` and `0.20.0` did not build on the baseline they
declare.** Both declare `rust-version = "1.85"` and require `rusqlite ^0.40`, which
then resolved `libsqlite3-sys 0.38.0`/`0.38.1` and `rusqlite 0.40.0`/`0.40.1` —
all of which need Rust 1.95. They are the only published versions that require
`rusqlite ^0.40`. Every other release requires an earlier line (`^0.32` up to
`0.13.0`, `^0.39` otherwise), so none of them is affected.

**Today, a fresh resolution builds on 1.85.** Resolving either version from
scratch now selects `rusqlite 0.40.2` and `libsqlite3-sys 0.38.2`, and builds on
Rust 1.85.0 (checked 2026-09-24 with a new crate declaring `rust-version = "1.85"`
and depending on `=0.19.1` and on `=0.20.0`). A build from an **existing lockfile**
that still holds `rusqlite 0.40.0`/`0.40.1` and `libsqlite3-sys 0.38.0`/`0.38.1`
still fails on 1.85. Update both together:

```sh
cargo update -p rusqlite
```

`cargo update -p libsqlite3-sys` alone is not enough: it leaves `rusqlite 0.40.0`
or `0.40.1` in place, and those need Rust 1.95 themselves. `rusqlite 0.40.2`
requires `libsqlite3-sys ^0.38.2`, so updating `rusqlite` moves both.

**Use `0.20.1` or greater**, and note that `^0.19` selects `0.19.1`, so move to
`0.20.1`+ rather than expecting a later `0.19.x`. The reason is no longer an MSRV
repair: later releases carry correctness fixes — see [`CHANGELOG.md`](https://github.com/nabbisen/localcache-rs/blob/main/CHANGELOG.md).

Neither `0.19.1` nor `0.20.0` is yanked. A fresh `localcache = "0.20"` resolves to
`0.20.1`, never `0.20.0`, so `0.20.0` is reachable only by an exact pin or an
existing lockfile; `0.19.1` is what `^0.19` selects, but its download volume is
indistinguishable from crawler traffic. Neither case justified a yank.

### If this blocks you

Tell us. A short note describing which side you are on, and what your own MSRV floor
actually is, is more useful than a patch — the two cases above needed opposite fixes,
and neither was the one first proposed.

## Advisory policy

Dependency security is one command, `cargo deny`, configured by `deny.toml` at the
repository root:

```sh
cargo make gate-deny
# the command it runs:
cargo deny --locked check -D advisory-not-detected advisories sources
```

**What it checks.** The RustSec advisory database is fetched fresh on every run,
and a failed fetch fails the run: there is no silent fallback to an old copy.
Against it, the gate denies

- vulnerabilities, unsound reports (`unsound = "all"`, so transitive dependencies
  too), unmaintained crates (`unmaintained = "all"`) and yanked versions, unless
  an `ignore` entry in `deny.toml` names the advisory;
- an `ignore` entry that no longer matches anything (`-D advisory-not-detected`),
  so a disposition cannot outlive its cause;
- a dependency from any registry other than crates.io, or from a Git repository
  (`[sources]`).

The graph it scans is the one that can be built: every feature on
(`all-features = true`), all targets, development dependencies included. A package
that sits in `Cargo.lock` only because of an optional dependency that no feature
of this workspace enables cannot be part of any build, and is not scanned.

**Where the dispositions live.** In `deny.toml`, under `[advisories] ignore`. Each
entry is the advisory id and a `reason` that says why it is accepted *and* the
condition that would change the decision, because an `ignore` has no expiry date:

```toml
{ id = "RUSTSEC-2025-0052", reason = "Preserve the advertised async-std runtime feature. Follow-up: Reassess if a maintained fork of async-std gains adoption, or if any vulnerability or unsound advisory is published against async-std at the locked version." },
```

Two unmaintained-crate findings are accepted this way today:

- `RUSTSEC-2025-0052` for `async-std`, preserving the advertised optional runtime
  feature; and
- `RUSTSEC-2025-0141` for `bincode`, preserving the established legacy payload
  wire format.

Both stay visible as notes on every run (`cargo deny -L info check advisories
sources`). A newly published vulnerability or unsound advisory against either
crate has a different id, so no existing entry covers it and the gate fails the
day it appears.

**To accept a finding:** add an `ignore` entry with its reason and follow-up
condition, and get the change reviewed like any other. **To retire one:** delete
the entry. If the advisory is still reported, the gate fails again; if the crate
is gone or fixed, leaving the entry in fails the gate instead (`advisory-not-detected`),
which is the prompt to delete it.

**When it runs.** In CI on every push and pull request (job "Standard gates
(dependency security)", required by the release gate), and every Monday in
`.github/workflows/advisories.yaml`, also on request. The weekly run is the
point: an advisory is published without any push, and a failure there means a new
advisory or yank has reached the locked graph. Dependabot alerts for Cargo
dependencies (a repository setting) back it up between runs.

**Pinned tool versions.** `cargo-deny` 0.20.2, `cargo-hack` 0.6.45 and
`cargo-make` 0.37.24, each installed with `cargo install <tool> --version <v>
--locked`. They are written in the header of `Makefile.toml` and in the "Cache
pinned gate tools" and "Install pinned gate tools" steps of
`.github/workflows/ci.yaml`, `advisories.yaml` and `msrv-fresh.yaml`.
Dependabot does not track tools installed this way, so raising a version means
editing all of those places together.
