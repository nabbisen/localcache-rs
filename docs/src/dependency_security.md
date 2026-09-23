# MSRV and Dependency Security

The workspace declares and tests Rust 1.85 as its minimum supported Rust
version (MSRV). A successful build on current stable Rust does not substitute
for this contract. Dependency changes must preserve all four locked MSRV rows:

```sh
cargo +1.85.0 check -p localcache --all-targets --all-features --locked
cargo +1.85.0 check -p localcache --all-targets \
  --no-default-features --features localcache/async-std --locked
cargo +1.85.0 check -p localcache --all-targets \
  --no-default-features --features localcache/smol --locked
cargo +1.85.0 check -p localcache-cli --all-targets --all-features --locked
```

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
- **Verification:** the four locked rows above in CI and the release gate, and
  after each publication a fresh consumer crate with `rust-version = "1.85"` builds
  the published version on the declared toolchain.
- **The previous minor line** gets security, data-loss, and corruption fixes for
  6 months after a raise, where the fix does not itself need the raise.

## Why `rusqlite` is pinned below its newest line

`localcache` requires `rusqlite ^0.39`, not the newer `0.40`. This is deliberate,
it is the constraint most often asked about, and it is worth understanding before
filing a request to change it.

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

- **`localcache 0.21.x` keeps `rusqlite ^0.39`** — for compatibility, not because
  of the toolchain. `rusqlite::Error` is part of this crate's public
  `LocalFileCacheError::Database` variant, and `links` (below) allows one SQLite
  line per dependency graph, so changing the line is a breaking change and ships
  only in a minor release.
- **`localcache 0.22.0` moves to `rusqlite 0.40.2`**, with the declared MSRV still
  1.85. The requirement names `0.40.2`, not a bare `0.40`, so that no lockfile can
  keep `rusqlite 0.40.0`/`0.40.1` and `libsqlite3-sys 0.38.0`/`0.38.1`, which need
  1.95.
- **Consumers who pin `rusqlite 0.40` directly** can use `localcache` from 0.22.0.

The gain is a newer bundled SQLite (3.51.3 → 3.53.2). The cost of staying on
`^0.39` until then is the older one: no advisory currently affects it, and the
dependency-security gate below scans `libsqlite3-sys` along with everything else,
so a future one would surface there.

### Why this cannot be worked around downstream

`libsqlite3-sys` declares `links = "sqlite3"`, and Cargo permits **exactly one**
package with a given `links` value in a dependency graph. Two `rusqlite` lines are
therefore not a tolerable duplicate — they are a hard resolution failure.

The practical consequence: a crate depending directly on `rusqlite 0.40` cannot
also depend on a `localcache` version requiring `^0.39`, and **no lockfile entry,
`--precise` pin, or feature flag at the consumer's end can resolve it**. If that
is your situation on `localcache 0.21.x` or earlier, the options are to move your
own `rusqlite` to 0.39, or to wait for `localcache 0.22.0`, which requires
`rusqlite 0.40.2`. You can also tell us — see below.

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
day) removed the Rust 1.95 requirement, so the second case's request (`rusqlite >= 0.40`) is being met in `localcache 0.22.0`,
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

`security/advisory-policy.json` is the sole checked-in disposition for RustSec
findings. `scripts/check_advisories.py` runs pinned cargo-audit 0.22.2, freezes
the exact RustSec revision, and independently fetches a fresh crates.io sparse
index record for every eligible package in `Cargo.lock`. Every registry record
must match the locked package name, version, and checksum. Missing, malformed,
duplicate, unavailable, or yanked records fail the gate.

Run the live gate with a new or empty evidence directory:

```sh
python3 scripts/check_advisories.py .git-exclude/advisory-evidence
```

The normal command has no date, policy, finding, ignore, or snapshot override.
It records the raw audit reports, RustSec Git identity, registry responses and
manifest digest, lockfile and policy digests, scan time, summary, and exit
status beneath the supplied output directory.

Vulnerabilities, unsoundness, yanked packages, unknown findings, malformed
input, and stale or expired policy entries are denied by default. A zero exit
status means every current finding exactly matches reviewed policy; it does
not mean the dependency graph has no warnings.

### Output labels

Every classified finding renders as exactly one of three labels — a
knowingly accepted vulnerability or unsound finding is never a bare `PASS`:

| Label | Meaning |
|---|---|
| `WARN` | `unmaintained`/`notice`, `action: "warn"` — visible, not denied |
| `EXCEPTION` | `vulnerability`/`unsound`, `action: "exception"` — a knowingly accepted defect, deliberately distinguished from `WARN` |
| `DENY` | no exact policy disposition, a stale entry, an expired one, or a yanked package |

The final `RESULT` line reports `findings`, `warnings`, `exceptions`, and
`denied` counts separately. The coverage line also reports how many locked
packages were excluded from advisory coverage because they have no crates.io
registry source (`path` or `git` dependencies) — see each excluded package's
name, version, and reason in the evidence bundle's registry manifest, under
`excluded`, so completeness does not require a manual `Cargo.lock`
cross-check.

### Standing dispositions vs. deferred fixes

Each policy entry matches exactly one finding: `(advisory ID, package,
version, kind)`. A finding with no matching entry is denied, and an entry
matching no current finding is denied as stale — so any version change, or a
new advisory of a different `kind` against the same package, re-raises the
finding immediately, on its own, with no calendar involved.

`unmaintained` and `notice` dispositions may rely on exactly that and omit
`expires` entirely (or set it `null`), becoming a **standing disposition**:
valid until the version, the kind, or the dependency graph changes, with no
date to renew. This applies because "unmaintained" is a **standing
condition** — either acceptable or not, and the answer does not change on a
particular date — and because a newly published `vulnerability` advisory
against the same package is a *different* policy key that no `unmaintained`
entry can cover, so it is denied the day it appears rather than waiting for
an expiry.

`vulnerability` and `unsound` dispositions still require `expires`, and it
must still post-date `approved`. Accepting one of these is a **deferred
fix** for a known defect, not a standing condition, and a deferral needs a
deadline that forces reconsideration — that is what the scheduled expiry is
for.

A standing disposition's `follow-up` field must state the **condition** that
would change the decision (for example, "reassess if a maintained fork gains
adoption, or if any vulnerability advisory is published against this
package") rather than a date — there is no date to point to. Gate output
for a standing disposition reads `standing disposition` and names that
condition directly, rather than printing a date that does not exist:

```text
WARN RUSTSEC-2025-0052/async-std/1.13.2/unmaintained: warn, standing disposition
(localcache maintainers) — reassess if a maintained fork of async-std gains
adoption, or any vulnerability or unsound advisory is published against
async-std at the locked version.
```

Two unmaintained-package findings are currently acknowledged as standing
dispositions:

- `RUSTSEC-2025-0052` for `async-std 1.13.2`, preserving the advertised
  optional runtime feature; and
- `RUSTSEC-2025-0141` for `bincode 2.0.1`, preserving the established legacy
  payload wire format.

Both are warnings rather than known vulnerabilities, remain visible on every
scan, and have no expiry date — see `security/advisory-policy.json` for each
entry's exact re-raise condition.
