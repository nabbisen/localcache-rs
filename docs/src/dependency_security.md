# MSRV and Dependency Security

The workspace declares and tests Rust 1.85 as its minimum supported Rust
version (MSRV). A successful build on current stable Rust does not substitute
for this contract. Dependency changes must preserve all four locked MSRV rows:

```sh
cargo check -p localcache-format -p localcache --all-targets --all-features --locked
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
  Cargo, then the same four rows — runs on request and at every release
  (`.github/workflows/msrv-fresh.yaml`, which also runs weekly; nothing relies on
  that), and a drift there blocks the release.
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
rusqlite 0.41  ->  libsqlite3-sys 0.39.x   (bundles SQLite 3.54.0 to 3.54.1)
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
  that only for a named necessity; none was known when 0.22 shipped. A security fix
  that is only in the newer SQLite has since been found (see "Known issue: the bundled
  SQLite" in [Security Model](./security.md)); the raise is made in a release, and this
  page is updated then. A crate that depends directly
  on `rusqlite 0.41` therefore cannot be combined with `localcache 0.22`
  (`links`, below).
- **Consumers who pin `rusqlite 0.40` directly** can use `localcache` from 0.22.0.

The gain was a newer bundled SQLite (3.51.3 → 3.53.2, checked on the built
crate: `rusqlite::version()` prints `3.53.2`). While `localcache` stayed on
`^0.39`, no RustSec advisory affected the older one. The advisory check below reads
the RustSec database: a vulnerability in the C source of the bundled SQLite appears
there only if RustSec lists it, and a CVE that it does not list is not reported by the
gate. One such CVE is described under "Known issue: the bundled SQLite" in
[Security Model](./security.md).

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

Neither `rusqlite 0.40.x` nor `libsqlite3-sys 0.38.x` declares a `rust-version` in its
manifest. Because of that, Cargo's MSRV-aware resolution cannot see a toolchain
requirement and cannot route around it. (The 0.41 line does declare one:
`rusqlite 0.41.1` says 1.89.0 and `libsqlite3-sys 0.39.1` says 1.88.0, checked in their
manifests on 2026-11-07.)

The `rusqlite 0.40.x` and `libsqlite3-sys 0.38.x` series show why that matters:
the floor moved **within a patch series**, up (`rusqlite 0.40.0`/`0.40.1` and
`libsqlite3-sys 0.38.0`/`0.38.1`) and then back down (`rusqlite 0.40.2` and
`libsqlite3-sys 0.38.2`), invisibly to Cargo's MSRV-aware resolver. A consumer's
build could break, or be repaired, with no change to anything they declared. That is why RFC 023 adds a check of a
*fresh* resolution, rather than trusting the locked graph alone.

That check, and the guarantee that `rust-version = "1.85"` builds, assume **MSRV-aware resolution** on the consumer's side.
Cargo does it by default for edition 2024 (resolver 3). On edition 2021 it must be requested, with
`CARGO_RESOLVER_INCOMPATIBLE_RUST_VERSIONS=fallback` or `resolver.incompatible-rust-versions = "fallback"` in
`.cargo/config.toml`; otherwise a fresh resolution takes the newest versions whatever they declare. For example, at
the v0.22.0 release, with the `watching` feature on Rust 1.85, resolver 2 chooses `notify-types 2.2.0` (needs Rust 1.88)
and the build fails, while MSRV-aware resolution chooses `2.1.0` and the build passes (checked with `localcache 0.22.0`).

It is still so for `rusqlite 0.40.2`. Its release notes say "Lower MSRV to
1.88.0", but its manifest declares no `rust-version`, and the declared-MSRV rows
(`cargo make gate-msrv`) and the fresh-resolution check both pass on Rust 1.85.0
with it. What upstream states and what the build needs are not the same thing,
which is why the fresh-resolution check exists, and why a release runs it again.

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

## How freshness is controlled

No schedule of any period is what this project depends on. Freshness matters at two moments, and each
has a control that does not need anyone to remember to look.

**When we release** (what users and deps.rs see). `cargo make gate-requirements`
(`scripts/check_requirements.py`) compares every dependency requirement that any workspace member declares
(normal, build and development, optional and target-specific ones included; workspace members excluded) with
the newest version on the crates.io index that is not yanked and not a pre-release. A dependency passes if its
requirement admits that version, or if `requirements-held.toml` has a row for it that says why it is held and
what ends the hold, and names the newest version it was written against. When a newer version is published the
row fails, so an upstream event, not a date, brings the decision back. A row for a requirement that now passes,
or that no workspace member declares any more, fails too: the release that ends a hold deletes its row. The gate
fails closed: an index that cannot be fetched or parsed, a requirement that names a pre-release or has a form the
script does not understand, a crate with no stable version. There is no cached copy and no fallback. The `Release`
workflow's `Gate` job runs it, so a release stops until every line is decided. It is deliberately not in `ci.yaml`:
an upstream publication between two pushes must not turn an unrelated push red.

**When an advisory is published against what we ship.** GitHub Dependabot alerts for Cargo dependencies are
**enabled** (the owner enabled them on 2026-11-07): GitHub tells the owner when an advisory is published against
the locked graph. Dependabot *security updates* stay off, because an automatic bump does not know the MSRV, the
`links` constraint or the exact `localcache-format` pin. The workflows `advisories.yaml` and `msrv-fresh.yaml` exist
and run weekly, and a release runs them again; nothing relies on their having run.

**Limits, stated.** Nothing detects staleness in a repository that is idle and silent; that costs nothing until a
release, which the gate stops. And the advisory sources list vulnerabilities **by crate**: RustSec (which
`cargo deny` reads) and GitHub's reviewed advisories (which Dependabot alerts read). A vulnerability in the C source
that a `-sys` crate bundles reaches them only when someone maps it to the crate. For CVE-2026-119375 in SQLite,
bundled by `libsqlite3-sys`, nobody had, as of 2026-11-07: OSV and GitHub publish the CVE itself, without a package,
and neither lists it against `libsqlite3-sys` or `rusqlite`. So neither the advisory gate nor Dependabot alerts would
have said anything about it; it is described under "Known issue: the bundled SQLite" in
[Security Model](./security.md). This says nothing about how often that happens in general, or about what the alerts
will do later.

## Requirements held back

A declared requirement that does not admit the newest stable version on crates.io is a decision,
and this is where each one is recorded, with the reason and what would end it. The record is the file
`requirements-held.toml` at the repository root; the table below is generated from it and checked against
it (`scripts/check_requirements.py`), so the two cannot differ. How the file is used is described under
"How freshness is controlled". The MSRV is 1.85 (RFC 023): it is raised only for a named necessity, and a
development tool never justifies a raise.

<!-- requirements-held:begin (generated from requirements-held.toml by scripts/check_requirements.py --write-book) -->

| Requirement | Newest seen | Why it is held | What ends the hold |
|---|---|---|---|
| `aes-gcm ^0.10.3` | `0.11.1` | aes-gcm 0.11 needs a dependency, ctutils, that needs Rust 1.87 or newer, and the ctutils releases that build on 1.85 carry RUSTSEC-2026-0385 (non-constant-time operations in a cryptographic layer). | The MSRV is at least 1.87. It is then adopted in its own patch release, with the proof that the ciphertext format is unchanged. |
| `criterion ^0.7` | `0.8.2` | criterion 0.8 needs Rust 1.86. It is a development-only dependency (the cache_bench benchmark), and a development tool never justifies an MSRV raise. | The MSRV reaches 1.86 for another reason (the rusqlite row), or the benchmark stops using criterion. |
| `opentelemetry ^0.32` | `0.33.1` | The opentelemetry feature is removed in v0.26.0 (announced in v0.25.1). A bump of a dependency that is about to leave the tree is not worth a release. | The removal of the opentelemetry feature lands. The row is deleted with the dependency. |
| `rusqlite ^0.40.2` | `0.41.1` | rusqlite 0.41 (bundled SQLite 3.54.1) declares Rust 1.89, and libsqlite3-sys 0.39 declares 1.88; the MSRV is 1.85. | An MSRV raise authorized under RFC 023 R3.1 for the security fix described under Known issue in docs/src/security.md. No version or date is promised here. |
| `sha2 ^0.10` | `0.11.1` | sha2 0.11 needs Rust 1.88. It is a development-only dependency (the digest checks of the committed fixtures), and a development tool never justifies an MSRV raise. | The MSRV reaches 1.88 for another reason (the rusqlite row), or the checks stop needing SHA-256 from a library. |
| `tracing-opentelemetry ^0.33` | `0.34.0` | The opentelemetry feature is removed in v0.26.0 (announced in v0.25.1). A bump of a dependency that is about to leave the tree is not worth a release. | The removal of the opentelemetry feature lands. The row is deleted with the dependency. |

<!-- requirements-held:end -->

## SQLite compile options we do not use

The bundled SQLite is built by `libsqlite3-sys` with several optional parts switched on for every user:
full-text search (FTS3/FTS4 and FTS5), R-Tree, STAT4, the DBSTAT table, SOUNDEX and support for loading
extensions. **This crate uses none of them**: it uses core SQL and the JSON functions, which are part of the
core since SQLite 3.38.

**A program that builds its own binary** can remove them. `libsqlite3-sys` has no Cargo feature for it, but its build
script appends the words of the environment variable `LIBSQLITE3_FLAGS` to the compiler command line, and a
`-U` word undefines an option:

```sh
LIBSQLITE3_FLAGS="-USQLITE_ENABLE_FTS3 -USQLITE_ENABLE_FTS3_PARENTHESIS -USQLITE_ENABLE_FTS5 -USQLITE_ENABLE_RTREE -USQLITE_ENABLE_STAT4 -USQLITE_SOUNDEX -USQLITE_ENABLE_DBSTAT_VTAB" cargo build --release
```

Measured on 2026-11-07 with `rusqlite 0.40.2`: with these words set, `fts3`, `fts5`, `rtree`, `dbstat` and `soundex`
are gone from a test program (`no such module`, `no such table`, `no such function`) and `json_extract` still works;
that test program's binary is 22% smaller (2,715,424 to 2,105,232 bytes); and this workspace's own test suite
(742 tests, all features) passes with the options removed.

What this does and does not do:

- It reaches **only the build where the variable is set**. A user who depends on `localcache` from crates.io gets the
  default build unless they set the variable in their own build.
- It does **nothing for CVE-2026-119375**, which is in the core of SQLite (see "Known issue: the bundled SQLite" in
  [Security Model](./security.md)). It reduces the exposure to future vulnerabilities in the parts it removes.
- Your own program may use FTS or R-Tree through `rusqlite` in the same binary: check before you remove them.

## The `zstd` features

From v0.22.0 the `compression` feature builds `zstd` with `default-features = false`,
which leaves out the decoders for pre-2016 frame formats (`legacy`), array-sized
buffers (`arrays`) and dictionary training (`zdict_builder`): `localcache` writes and
reads only standard frames, and the compatibility fixtures written by released versions
decode unchanged.

## Advisory policy

Dependency security is one command, `cargo deny`, configured by `deny.toml` at the
repository root:

```sh
cargo make gate-deny
# the command it runs:
cargo deny --locked check -D advisory-not-detected advisories sources
```

**What it checks.** The RustSec advisory database is fetched fresh on every run,
and a failed fetch fails the run: there is no silent fallback to an old copy. It checks
that database and nothing else: a vulnerability that is published only in the National
Vulnerability Database, for example in the C source of the SQLite that `libsqlite3-sys`
bundles, is not seen by it.
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

Two unmaintained-crate findings are accepted this way today, both behind the one
optional `async-std` runtime feature, which is removed in v0.26.0 (announced in v0.25.1);
both entries are deleted with it:

- `RUSTSEC-2025-0052` for `async-std`, preserving the advertised optional runtime
  feature.
- `RUSTSEC-2026-0441` for `pin-utils` (archived upstream on 2026-10-01, no
  vulnerability or unsound advisory), which only `async-std` depends on. The entry
  ends with the `async-std` entry: when `async-std` drops `pin-utils`, or the
  feature is removed, it is deleted.

It stays visible as a note on every run (`cargo deny -L info check advisories
sources`). A newly published vulnerability or unsound advisory against that crate
has a different id, so no existing entry covers it and the gate fails the day it
appears.

A second entry, `RUSTSEC-2025-0141` for `bincode`, preserved the legacy payload
wire format until v0.22.1. `bincode` is unmaintained and its decoder could not be
made to check a claimed length against the input (RFC 028), so the format's reader
and writer now live in this crate, checked against `bincode`'s own output (golden
vectors and a one-time differential run), `bincode` left the dependency tree and
the entry was deleted: a disposition does not outlive its cause.

**To accept a finding:** add an `ignore` entry with its reason and follow-up
condition, and get the change reviewed like any other. **To retire one:** delete
the entry. If the advisory is still reported, the gate fails again; if the crate
is gone or fixed, leaving the entry in fails the gate instead (`advisory-not-detected`),
which is the prompt to delete it.

**When it runs.** In CI on every push and pull request (job "Standard gates
(dependency security)", required by the release gate), and again at every release
(`.github/workflows/advisories.yaml` is called by `Release`). That workflow also runs
every Monday and on request; that run is a convenience, and nothing relies on it. An
advisory is published without any push: for a crate of the locked graph it reaches the
owner through Dependabot alerts, and the release runs this check again (see "How
freshness is controlled").

**Pinned tool versions.** `cargo-deny` 0.20.2, `cargo-hack` 0.6.45 and
`cargo-make` 0.37.24, each installed with `cargo install <tool> --version <v>
--locked`. They are written in the header of `Makefile.toml` and in the "Cache
pinned gate tools" and "Install pinned gate tools" steps of
`.github/workflows/ci.yaml`, `advisories.yaml` and `msrv-fresh.yaml`.
Dependabot does not track tools installed this way, so raising a version means
editing all of those places together.
