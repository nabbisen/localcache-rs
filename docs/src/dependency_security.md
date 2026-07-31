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
