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

## Why `rusqlite` is pinned below its newest line

`localcache` requires `rusqlite ^0.39`, not the newer `0.40`. This is deliberate,
it is the constraint most often asked about, and it is worth understanding before
filing a request to change it.

### The chain

```text
rusqlite 0.39  ->  libsqlite3-sys 0.37.x   (bundles SQLite 3.51.3)
rusqlite 0.40  ->  libsqlite3-sys 0.38.x   (bundles SQLite 3.53.2)
```

`libsqlite3-sys 0.38.x` uses `cfg_select!` in its build script, which requires
**Rust 1.95**. We bisected it against this workspace:

| Toolchain | `rusqlite 0.40` |
|---|---|
| 1.85.0 | fails — `cannot find macro cfg_select` |
| 1.94 | fails — same |
| **1.95.0** | passes |

So moving to `rusqlite 0.40` would raise this crate's MSRV from 1.85 to exactly
1.95 — currently within three releases of stable. We hold `^0.39` to keep the
declared 1.85 contract real.

The cost of that choice is the older bundled SQLite (3.51.3 rather than 3.53.2).
No advisory currently affects it, and the dependency-security gate below scans
`libsqlite3-sys` along with everything else, so a future one would surface there.

### Why this cannot be worked around downstream

`libsqlite3-sys` declares `links = "sqlite3"`, and Cargo permits **exactly one**
package with a given `links` value in a dependency graph. Two `rusqlite` lines are
therefore not a tolerable duplicate — they are a hard resolution failure.

The practical consequence: a crate depending directly on `rusqlite 0.40` cannot
also depend on a `localcache` version requiring `^0.39`, and **no lockfile entry,
`--precise` pin, or feature flag at the consumer's end can resolve it**. If that
is your situation, the options are to move your own `rusqlite` to 0.39, or to tell
us — see below.

### The upstream cause

Neither `rusqlite` nor `libsqlite3-sys` declares a `rust-version` in its manifest.
Because of that, Cargo's MSRV-aware resolution cannot see the 1.95 requirement and
cannot route around it.

**If `libsqlite3-sys 0.38.x` declared `rust-version = "1.95"`, this whole conflict
would disappear**: resolution would hand `0.37.x` to consumers with a lower floor
and `0.38.x` to everyone else, automatically, and no choice would fall to this
crate at all.

Until then the constraint stands, and this section exists because it is likely to
stand for a while.

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

The second case is the more instructive one: the reporter believed their floor was
around 1.88, because Cargo only reports crates that *declare* `rust-version` — and
`libsqlite3-sys` declares none. The constraint had been invisible to them the whole
time.

The first case's fix was not retroactive: `0.19.1`, published before v0.20.1,
carried the same defect and was identified later by the same reporter. Neither
already-published version was repaired by the v0.20.1 fix — which is why both are
named below rather than treated as resolved.

### Affected published versions

**`0.19.1` and `0.20.0`** are broken under the constraint above: both declare
`rust-version = "1.85"` and require `rusqlite ^0.40`, which resolves
`libsqlite3-sys 0.38.x` and its Rust 1.95 `cfg_select!` macro — neither builds on
the baseline it declares. `0.19.0` and earlier (`rusqlite ^0.39`) and `0.20.1`
onward (constrained back to `^0.39`) are unaffected.

**Use `0.20.1` or greater.** If you are pinned to `localcache = "0.19"`, note that
it resolves to `0.19.1` — move to `0.20.1`+ rather than expecting a working
`0.19.x`.

Neither `0.19.1` nor `0.20.0` is yanked. A fresh `localcache = "0.20"` resolves to
`0.20.1`, never `0.20.0`, so the broken version is reachable only by an exact pin or
an existing lockfile; `0.19.1` is what `^0.19` selects, but its download volume is
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
