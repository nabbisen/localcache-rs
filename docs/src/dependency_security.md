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

Two unmaintained-package warnings are temporarily acknowledged through
2026-10-21:

- `RUSTSEC-2025-0052` for `async-std 1.13.2`, preserving the advertised
  optional runtime feature; and
- `RUSTSEC-2025-0141` for `bincode 2.0.1`, preserving the established legacy
  payload wire format.

They are warnings rather than known vulnerabilities. They remain visible on
every scan and fail on the expiry date. Replacement, removal, or renewal
requires a separate compatibility decision with current evidence before the
deadline.
