# RFC 014 — Declared MSRV and Dependency Security Policy

| Field | Value |
|---|---|
| Status | Implemented (0.20.1) |
| Feature | *(workspace dependency and release policy; no production feature flag)* |
| Touches | workspace manifests and lockfile, declared-MSRV CI, advisory policy/checker and tests, dependency/security documentation, RFC 009 release-gate integration point |
| Finding | Architect review B-06 and B-08 |
| Milestone | Phase 21 M4 |

## Summary

Keep Rust 1.85 as localcache's declared minimum supported Rust version and
restore a dependency graph that actually compiles on that toolchain. Select
the evidence-backed `rusqlite 0.39` / `libsqlite3-sys 0.37` SQLite line and
the Rust-1.85-compatible `criterion 0.7` benchmark line, then update the
fixable `crossbeam-epoch` and `anyhow` advisories without broad unrelated
dependency churn.

Add a small checked-in advisory policy and checker. Vulnerabilities,
unsoundness, yanked packages, unknown informational findings, malformed policy,
and expired acknowledgements fail closed. The two current unmaintained
dependencies—`async-std` and `bincode`—remain temporarily because removing
either in a corrective patch would break an advertised runtime feature or the
payload wire-format contract. Their exact advisories and locked versions are
acknowledged only through 2026-10-21, produce visible warnings, and require a
new decision before that date.

The implementation replaces the misleading stable-based CI job with the exact
declared-MSRV matrix already specified by RFC 009 and adds the advisory-policy
gate. RFC 009 M6 remains responsible for folding these gates into the one
canonical release runner and evidence bundle.

This RFC changes no Rust API, database schema, payload encoding, feature name,
or release version. Accepted status authorizes M4 implementation under this
design; it does not authorize release housekeeping or release action.

## Motivation

The workspace declares `rust-version = "1.85"`, but its locked graph cannot
compile on Rust 1.85:

- `rusqlite 0.40.1` selects `libsqlite3-sys 0.38.1`, whose build script uses
  the standard-library `cfg_select!` macro introduced after Rust 1.85;
- a live package-scoped Rust 1.85 check fails at that macro; and
- the complete all-target check stops even earlier because `criterion 0.8.2`
  declares Rust 1.86.

The prior `rusqlite 0.39.0` / `libsqlite3-sys 0.37.0` graph is present in
repository history and contains no `cfg_select!` use. Official crate metadata
does not declare an MSRV for that SQLite pair, so only the complete localcache
gate—not metadata inference—can prove compatibility. `criterion 0.7.0`
declares Rust 1.80 and retains the benchmark API and `html_reports` feature
used by this workspace.

The refreshed 2026-07-23 RustSec scan of 240 locked packages reports:

| Advisory | Locked package | Kind | Available action |
|---|---|---|---|
| `RUSTSEC-2026-0204` | `crossbeam-epoch 0.9.18` | vulnerability | update to `>=0.9.20` |
| `RUSTSEC-2026-0190` | `anyhow 1.0.102` | unsoundness | update to `>=1.0.103` or remove the stale lock entry |
| `RUSTSEC-2025-0052` | `async-std 1.13.2` | unmaintained | no patched release; replacing/removing changes a public feature commitment |
| `RUSTSEC-2025-0141` | `bincode 2.0.1` | unmaintained | no patched release; replacement must preserve the established bincode-1 legacy wire format |

The vulnerable crossbeam path is benchmark-only:
`criterion -> rayon -> crossbeam-deque -> crossbeam-epoch`. The advisory still
blocks a release because developer and benchmark dependencies are part of the
locked source and release gates. The affected `anyhow` entry is reachable only
through target/tooling packages in the lockfile, but `cargo audit` correctly
scans the complete lockfile; an unused affected entry is not a reason to leave
known unsound code locked.

Plain `cargo audit` currently exits nonzero for the vulnerability while merely
printing the three informational warnings. That default is not a project
policy: it has no reviewed expiry, cannot distinguish an acknowledged
compatibility constraint from a forgotten warning, and allows a future
unsoundness advisory by default. RFC 009 therefore requires RFC 014 to define
the deny/warn/exception semantics and machine-readable input for the release
security gate.

## Goals

1. Preserve the published Rust 1.85 compatibility promise.
2. Make every RFC 009 package/feature MSRV row compile with the exact declared
   toolchain and locked graph.
3. Clear all fixable vulnerability and unsoundness findings with minimal lock
   churn.
4. Give every remaining advisory an explicit, exact, expiring disposition.
5. Fail closed on new, unknown, stale, malformed, or expired policy state.
6. Make CI exercise the real MSRV and advisory policy before M6 integrates
   them into the canonical release runner.
7. Preserve SQLite behavior, benchmark availability, async feature names, and
   bincode legacy payload compatibility.
8. Close B-06 and B-08 through one focused M4 implementation acceptance gate.

## Non-goals

- Raising the MSRV to Rust 1.86, 1.95, stable, or the canonical producer's
  Rust version.
- Removing or renaming the public `async-std` feature in v0.20.1.
- Replacing bincode or changing any stored payload byte. Such a change needs a
  separate compatibility RFC and historical-fixture proof.
- Updating every dependency to its newest release or treating age alone as a
  security defect.
- Vendoring, forking, or locally patching third-party crates.
- Changing SQLite schema, migration behavior, bundled-vs-system selection, or
  public database APIs.
- Completing RFC 009 M6 release-runner integration, packaging, legal-file
  generation, release housekeeping, RC construction, publication, tagging, or
  hosted release work.
- Expanding RFC 015's async/watcher failure-semantics scope.

## Terminology

- **Declared MSRV**: the exact `[workspace.package].rust-version`, currently
  `1.85`, normalized to toolchain `1.85.0` for rustup/Cargo invocation.
- **Stable gate**: current supported stable Rust validation. It supplements
  but never substitutes for the declared-MSRV gate.
- **Denied finding**: a report which makes the policy checker nonzero.
- **Acknowledged warning**: one exact advisory/package/version tuple permitted
  temporarily by checked-in policy. It remains visible and has an expiry.
- **Exception**: a time-limited authorization for an otherwise denied finding.
  No exception is proposed for the initial M4 result.
- **Advisory database identity**: the exact Git commit of the RustSec database
  used to create a report.
- **Eligible registry package**: every `Cargo.lock` package whose source is the
  exact crates.io registry. Path/workspace and Git packages have no crates.io
  yanked state; another registry source is unsupported and fails closed.
- **Registry snapshot**: the locally frozen set of crates.io sparse-index
  responses and exact version records used to decide yanked state, identified
  by a canonical manifest SHA-256.
- **Stale policy entry**: an acknowledgement or exception which matches no
  finding in the current audit report.

## Requirements

### R1 — Rust 1.85 remains the contract

`[workspace.package].rust-version` remains `1.85`. Member crates continue to
inherit it. No implementation may raise the value to make the current graph
compile.

The MSRV gate reads this manifest field and verifies that both
`rustc --version` and `cargo --version` belong to the normalized `1.85.0` toolchain
before running Cargo. A job using `stable` is neither named nor counted as
MSRV evidence.

Edition 2024 is supported by Rust 1.85 and does not justify a higher minimum.
The canonical producer toolchain remains a separate release reproducibility
decision under RFC 009.

### R2 — Evidence-backed direct dependency selection

The workspace manifest changes only these direct dependency lines for MSRV:

```toml
rusqlite = { version = "0.39", features = ["bundled", "limits"] }
criterion = { version = "0.7", features = ["html_reports"] }
```

The resulting lockfile must select `rusqlite 0.39.x`,
`libsqlite3-sys 0.37.x`, and `criterion 0.7.x`. Patch versions remain
lockfile decisions; they are not fixed unnecessarily in the manifest.

The implementation must verify that the selected rusqlite line retains every
API and feature localcache uses, including bundled SQLite, runtime limit
configuration, transactions, open flags, and schema/introspection APIs. Any
source adaptation must be mechanical and behavior-preserving. Discovery of a
semantic database/API change returns this RFC for review rather than silently
expanding scope.

### R3 — Minimal remediation of fixable advisories

Within semver-compatible transitive requirements, update:

- `crossbeam-epoch` to at least `0.9.20`; and
- `anyhow` to at least `1.0.103`, unless normal lockfile resolution removes
  the unreachable entry entirely.

Do not add either crate as an artificial direct dependency merely to force a
version. Use normal Cargo resolution and review the complete lockfile diff.
Unrelated lockfile changes are excluded unless the Rust 1.85 resolver or one
of these exact remediations requires them.

After the known direct changes, Cargo may identify another locked package
whose declared or actual MSRV exceeds 1.85. The implementation may select the
newest semver-compatible Rust-1.85-compatible version without another design
round only when all of the following hold:

1. the change is transitive or stays within the existing direct dependency's
   current semver-compatible line;
2. the MSRV failure and candidate metadata/source evidence are recorded;
3. no public API, feature, wire format, schema, or runtime behavior changes;
4. stable and MSRV gates both pass; and
5. the implementation review lists the exact additional selection.

A required major/minor direct-dependency change beyond `rusqlite 0.39` and
`criterion 0.7` returns the RFC for design review.

### R4 — Fail-closed advisory taxonomy

The project policy applies to the complete checked-in `Cargo.lock`, including
normal, optional, target-specific, development, benchmark, and build
dependencies.

Default actions are:

| Finding kind | Default action |
|---|---|
| vulnerability | deny |
| unsound | deny |
| yanked | categorically deny; not exceptionable |
| unmaintained | deny until exactly acknowledged |
| notice or another informational kind | deny until exactly classified |
| unknown/malformed result | deny |

An exact policy entry may downgrade an informational finding to a visible
warning. An otherwise denied RustSec finding requires an explicit exception.
Both forms must name the advisory ID, package, exact locked version, kind,
owner, rationale, approval date, expiry date, and follow-up decision. Policy
uniqueness is the complete `(advisory ID, package, version, kind)` tuple; the
same advisory may legitimately affect two locked versions. Entries may not use
wildcards, version ranges, advisory categories, or package-only matching, and
contradictory entries for one complete tuple are invalid.

Yanked state has no RustSec advisory ID. It is identified by exact registry
source, package name, version, and lockfile checksum and is always denied. The
policy schema must reject an attempted warning or exception for a yanked
package.

The checker fails on or after the entry's expiry date. It also fails on a stale
entry, so an obsolete acknowledgement cannot silently authorize a future
different version or hide after the dependency is repaired.

### R5 — Initial advisory dispositions

The accepted M4 implementation must have no vulnerability, unsoundness,
yanked-package, or notice exceptions.

Two exact unmaintained findings are acknowledged as warnings through
2026-10-21:

| Advisory/package/version | Rationale | Follow-up boundary |
|---|---|---|
| `RUSTSEC-2025-0052` / `async-std` / `1.13.2` | v0.20.1 preserves the advertised optional runtime feature; the finding reports discontinued maintenance, not a known vulnerability | before expiry, separately decide deprecation/replacement/removal; RFC 015 may improve failure parity but does not implicitly remove the feature |
| `RUSTSEC-2025-0141` / `bincode` / `2.0.1` | bincode legacy configuration is the persisted 0.x payload contract; replacing it without format proof risks user data | before expiry, separately assess a format-compatible maintained implementation with all historical fixtures |

The date provides a review deadline, not automatic permission to remove either
compatibility surface. Renewal requires owner approval and a policy change
reviewed with current advisory and dependency evidence.

### R6 — One machine-readable policy and checker

Add:

- `security/advisory-policy.json` as the sole classification/expiry source;
- `scripts/check_advisories.py` as the policy executor; and
- focused standard-library unit tests under `scripts/tests/`.

JSON is chosen so the existing Python release tooling can parse policy without
a new package dependency. The policy has a schema version, default action map,
and exact finding entries. The checker validates the whole document before
classifying a report: unknown keys where ambiguity would matter, duplicate
complete tuples, invalid dates/actions, missing ownership/rationale/follow-up,
wildcarded versions, yanked overrides, and contradictory entries are errors.

In normal mode the checker:

1. invokes pinned `cargo-audit 0.22.2` with `--no-yanked` in JSON mode against
   `Cargo.lock` and the fixed RustSec checkout;
2. records the raw RustSec report separately from the human summary;
3. maps every vulnerability and informational warning by advisory ID,
   package, version, and kind;
4. loads the independently fetched and frozen R7 registry snapshot, proves one
   exact checksum-matching record for every eligible locked registry package,
   and categorically denies every record whose `yanked` value is true;
5. applies the checked-in RustSec policy and current UTC date;
6. prints every acknowledged warning and its expiry;
7. rejects every denied, unknown, expired, mismatched, incomplete, or
   operationally uncertain finding; and
8. exits zero only when both complete inputs are accounted for.

Fixture tests call classification functions with an injected date. The live
command exposes no `--today`, policy-path, registry-snapshot, action, ignore,
or exception override. Production and CI cannot alter date or classification
through environment variables or permissive flags. A separate unmistakable
fixture-only entry point may accept fixture paths, but it cannot initiate a
live scan.

Pinned cargo-audit 0.22.2 uses status 0 for a successful report without a
finding selected for its own denial, 1 for a successful report with such a
finding, and 2 for operational failure. Only status 0 or 1 together with valid
expected-schema JSON may reach policy classification. Status 2, another
status, missing/invalid JSON, or a fetch/tool/process failure is an operational
denial. The checker therefore parses a valid status-1 report instead of
confusing “findings present” with execution failure.

### R7 — RustSec and crates.io registry provenance

The blocking gate first invokes cargo-audit with fetching enabled and
`--no-yanked`. A valid expected-schema JSON report with status 0 or 1 proves
that the RustSec refresh and parsing completed. The gate records the RustSec
Git commit, then runs the authoritative advisory scan with `--no-fetch
--no-yanked` against that fixed checkout. It records the commit afterward and
requires equality. Fetch failure, missing Git identity, invalid JSON, status 2
or another status, or an audit tool version other than RFC 009's pinned
`cargo-audit 0.22.2` fails; `--stale` is forbidden.

Yanked state is not taken from cargo-audit because version 0.22.2 can continue
after registry refresh and per-package lookup failures. Instead, the checker
parses `Cargo.lock`, enumerates every eligible registry package, groups exact
versions by package name, and fetches each corresponding crates.io sparse-index
record over HTTPS in the current gate invocation. Requests identify localcache,
request revalidation, and accept only a complete HTTP 200 response; a cached
on-disk fallback, HTTP 304, timeout, TLS/HTTP error, or missing response is a
failure.

The URL is derived from the lowercased validated package name using Cargo's
crates.io sparse-index layout: `1/name` for one character, `2/name` for two,
`3/f/name` for three, and `fi/rs/name` (first two and next two characters) for
four or more. Names outside Cargo's package-name grammar fail before URL
construction; response-controlled text never selects a URL or path.

Fetching is bounded to one response per unique eligible package name, 16 MiB
per response, 256 MiB for the complete snapshot, a 30-second per-response
timeout, and a 15-minute overall deadline. Exceeding any bound fails rather
than truncating or skipping a package.

Each sparse-index response is stored as opaque evidence and hashed. Its
newline-delimited JSON is then validated without executing text. For every
eligible lock entry the response must contain exactly one record whose name,
version, and checksum equal `Cargo.lock` and whose `yanked` field is Boolean.
A missing/duplicate record, checksum mismatch, malformed line, unsupported
registry source, or any package lookup failure denies the gate.

The checker writes a canonical sorted registry manifest containing the sparse
URL, response SHA-256, relevant HTTP identity/freshness headers (`ETag`,
`Last-Modified`, and `Date` when supplied), fetch UTC timestamp, and every
selected `(source, name, version, checksum, yanked)` record. The manifest's
SHA-256 is the exact yanked-input identity. The authoritative classification
reads only this frozen local snapshot and verifies the manifest digest before
and after classification; any mutation fails. This proves complete lookup for
the lockfile without claiming the remote registry is globally atomic.

Evidence records:

- `cargo audit --version`;
- advisory database remote and exact commit SHA;
- registry snapshot manifest and SHA-256;
- raw sparse-index response SHA-256 values and available HTTP identity headers;
- the complete eligible-lock-entry coverage result;
- scan UTC timestamp;
- SHA-256 of `Cargo.lock` and `security/advisory-policy.json`;
- raw cargo-audit JSON report;
- human-readable policy result; and
- process exit status.

The RustSec commit and registry manifest digest must remain unchanged across
the authoritative classification. RFC 009 M6 owns the final evidence directory
and canonical-runner orchestration; this RFC supplies the gate behavior and
data which that runner consumes.

### R8 — Complete declared-MSRV matrix

The implementation runs the exact package-scoped RFC 009 matrix with
`--locked` on toolchain `1.85.0`:

```sh
cargo +1.85.0 check -p localcache --all-targets --all-features --locked
cargo +1.85.0 check -p localcache --all-targets \
  --no-default-features --features localcache/async-std --locked
cargo +1.85.0 check -p localcache --all-targets \
  --no-default-features --features localcache/smol --locked
cargo +1.85.0 check -p localcache-cli --all-targets --all-features --locked
```

Separate async-std and smol rows are mandatory because the all-feature
priority selects Tokio. All targets includes the benchmark, examples, tests,
library, and CLI surfaces and therefore detects both production and
development dependency drift.

### R9 — CI now enforces the M4 decisions

Replace the current `MSRV (stable)` job with a declared-MSRV job that obtains
the value from the root manifest, installs exact `1.85.0`, verifies the active
compiler/Cargo, and runs all four R8 rows. Hard-coding `stable` or silently
falling back to it is forbidden.

Add a separate dependency-security job which installs the pinned audit tool,
refreshes RustSec, obtains the complete live crates.io registry snapshot, and
invokes the R6 checker. CI may cache registry and build outputs, but neither a
cached advisory database nor cached sparse-index records may substitute for a
successful current refresh.

M6 will make CI and `Makefile.toml` invoke RFC 009's canonical runner rather
than retain duplicated command lists. That later orchestration refactor must
preserve these exact M4 outcomes and is not a reason to leave today's CI jobs
mislabelled or non-enforcing.

### R10 — Compatibility and regression preservation

The dependency change must preserve:

- schema v1-to-v5 migration and raw payload-byte fixtures from RFC 010;
- identifier validation, index creation/listing, and query-hint behavior from
  RFC 011;
- read-only open and mutation boundaries from RFC 012;
- path/glob/query behavior from RFC 013;
- bincode legacy serialization and every compatibility fixture;
- bundled SQLite operation on the supported host; and
- benchmark compilation with `localcache/json`.

No generated `.crate` artifact, license/notice copy, release archive, or
version change belongs to M4 implementation.

### R11 — Documentation and change record

Update maintainer-facing documentation and `CHANGELOG.md` to state:

- Rust 1.85 is tested, not merely declared;
- dependency updates must preserve the declared-MSRV matrix;
- the security gate's deny and expiry behavior;
- the two exact temporary unmaintained acknowledgements and review deadline;
  and
- warnings are not vulnerabilities and are not described as an entirely clean
  dependency graph.

Do not rewrite historical release entries or mark v0.20.1 released. RFC 005
and RFC 008 remain historical; any clarification is a forward correction
reference rather than a lifecycle rewrite.

### R12 — One M4 acceptance gate

Implementation remains one coherent M4 change: compatible dependency
selection, policy/checker, CI enforcement, tests, and documentation. Request
one independent implementation review after all evidence is available.

A favorable implementation review plus an owner-accepted implementation
commit may close B-06 and B-08 and mark M4 complete. It does not move RFC 014
to `done/`, begin M5 automatically, or authorize any release action.

No implementation handoff is proposed because this RFC contains the exact
dependency, policy, CI, and test sequence and work is not delegated. If
implementation is delegated later, a handoff must reference this RFC and the
review outcome.

## Detailed design

### Policy shape

The checked-in policy is intentionally small. An illustrative shape is:

```json
{
  "schema": 1,
  "defaults": {
    "vulnerability": "deny",
    "unsound": "deny",
    "yanked": "deny",
    "unmaintained": "deny",
    "notice": "deny"
  },
  "findings": [
    {
      "id": "RUSTSEC-2025-0052",
      "package": "async-std",
      "version": "1.13.2",
      "kind": "unmaintained",
      "action": "warn",
      "owner": "localcache maintainers",
      "approved": "<RFC-014-owner-approval-date>",
      "expires": "2026-10-21",
      "reason": "Preserve the v0.20.1 public runtime feature.",
      "follow-up": "Decide replacement, deprecation, or renewed acceptance before expiry."
    }
  ]
}
```

The implementation includes the analogous bincode entry. No policy entry
changes `cargo audit`'s report; classification happens afterward so raw
evidence remains complete. The `approved` placeholder is replaced with the
actual RFC 014 owner-approval date during implementation. Yanked findings do
not appear in this array and cannot be overridden.

### Checker result model

The checker produces one line per finding with `DENY`, `WARN`, or `PASS`
classification and a final count. It returns nonzero if:

- cargo-audit returns a status other than 0 or 1 or fails to produce valid
  expected-schema JSON;
- a vulnerability or warning is missing required fields;
- an exact policy match is absent or ambiguous;
- a denied category is present;
- an acknowledgement/exception is expired;
- a policy finding is stale;
- the crates.io snapshot cannot be fetched or does not exactly cover every
  eligible lockfile entry;
- a registry record is yanked, malformed, duplicated, checksum-mismatched, or
  changes after the snapshot is frozen; or
- evidence files cannot be written to the caller-provided M6 output boundary.

Warnings never disappear from human output. A zero result means “all findings
match current reviewed policy,” not “the dependency graph has no warnings.”

### Dependency update sequence

Implementation order is:

1. change only the two direct manifest minor lines;
2. resolve the lockfile and inspect the full diff;
3. update/remove the exact crossbeam and anyhow affected versions;
4. run each Rust 1.85 row and address only permitted additional MSRV
   selections;
5. run stable regressions and the advisory scan;
6. add policy/checker tests and CI enforcement;
7. run formatting once after implementation, then all final gates; and
8. request the single M4 implementation review.

This ordering uses the compiler and audit evidence to drive lock selection
without mixing a general dependency refresh into corrective work.

## Test plan

### Dependency and MSRV tests

- Assert workspace `rust-version` remains `1.85` and both members inherit it.
- Assert the lockfile contains the selected compatible rusqlite,
  libsqlite3-sys, criterion, crossbeam-epoch, and anyhow outcomes.
- Run all four R8 commands with exact Rust/Cargo 1.85.0.
- Compile the benchmark explicitly with `localcache/json`.
- Run the complete stable all-feature/all-target workspace tests.
- Run warnings-denied all-target/all-feature clippy and rustdoc.
- Run focused RFC 010–013 regression suites and doctests.

### Advisory-policy tests

Fixture-driven tests cover:

- a report with no findings;
- each default denied kind;
- the two exact acknowledged warnings;
- wrong advisory ID, package, version, or kind;
- unknown informational kinds;
- the day before expiry and the exact expiry date;
- duplicate complete tuples, two versions under one advisory, contradictory
  entries, malformed, wildcarded, and missing policy fields;
- stale policy entries;
- cargo-audit statuses 0, 1, 2, and an unexpected status with valid/invalid
  JSON combinations;
- registry refresh/HTTP failure and an unavailable sparse-index response;
- one per-package lookup failure among otherwise complete responses;
- missing, duplicate, malformed, wrong-version, and checksum-mismatched sparse
  records;
- an actual yanked record, which has package/version/checksum identity but no
  advisory metadata and is categorically denied;
- unsupported alternate registry source;
- registry manifest mutation/identity change across classification;
- internal test-date injection without a live-command date override; and
- raw report preservation with summary/exit-status agreement.

The live acceptance gate uses a freshly fetched RustSec database and must show
zero vulnerabilities, zero unsound findings, complete checksum-bound crates.io
coverage, zero yanked packages, and exactly the two reviewed unmaintained
warnings unless a dependency is safely removed before implementation review.
If either warning disappears, its stale policy entry must be removed and the
review request must explain why.

### Repository checks

- `cargo fmt --all -- --check`
- `git diff --check`
- `python3 -m unittest discover -s scripts/tests -v`
- `python3 scripts/source_integrity.py --require-tracked`
- `mdbook build docs`, followed by removal of generated `docs/book/`

## Security considerations

The policy never converts a known vulnerability or unsoundness report into a
warning in the initial implementation. Future exceptions are narrow,
version-exact, expiring, and visible. Unknown report kinds fail rather than
being silently treated as cargo-audit's default warning.

Audit data is untrusted structured input. The checker parses JSON without
executing report text, bounds ordinary file reads, writes only beneath an
explicit output boundary, and does not interpolate package/advisory text into
shell commands. It never reads or requests credentials.

Sparse-index paths are derived only by the crates.io lowercase package-name
algorithm after lockfile source/name validation; response bytes never select a
filesystem destination. Every eligible version is checksum-bound to the
lockfile, and yanked findings are non-exceptionable. Partial registry
availability cannot become a passing report.

A passing advisory gate is time- and database-specific. It is recorded with
the RustSec revision and lockfile/policy digests and does not claim future
security.

## Compatibility

Keeping Rust 1.85 and moving dependencies to compatible minor lines restores
the advertised downstream contract. Public features and method signatures are
unchanged. Bundled SQLite remains enabled, payload bytes remain bincode legacy,
and the current database schema remains version 5.

The two unmaintained acknowledgements are operational risk records, not a
change to runtime semantics. Any eventual dependency replacement must preserve
the relevant public and wire compatibility or use a separately approved
breaking/migration policy.

## Alternatives considered

### Raise MSRV to Rust 1.95

Rejected. It would make `rusqlite 0.40` compile but break the existing package
contract and downstream consumers in a corrective patch. The owner has not
authorized that compatibility change.

### Keep Criterion 0.8 because it is development-only

Rejected. Benchmarks and all targets are part of the source/release contract,
and the declared MSRV applies to the workspace's supported development graph.

### Remove async-std immediately

Rejected for v0.20.1. It is an advertised additive Cargo feature. Its
unmaintained status warrants a deadline and separate product decision, not an
unreviewed feature removal.

### Replace bincode immediately

Rejected. Payload bytes are persisted user data with historical fixtures and
an explicit legacy-format guarantee. Maintenance status alone does not prove a
replacement is wire-compatible.

### Use `cargo audit --ignore` or default exit status directly

Rejected. A bare ignore has no package/version binding, rationale, ownership,
or expiry. Default audit status permits informational unsoundness and does not
fail on forgotten acknowledgements.

### Add cargo-deny and a second policy system

Rejected for this milestone. The pinned cargo-audit JSON report plus a small
standard-library checker provides the required exact/expiring semantics
without another release tool or duplicate advisory source. M6 can reconsider
tool consolidation only through RFC 009's producer-tool review boundary.

## Rollback

Before release, rollback is the ordinary reversal of manifest, lockfile,
policy/checker, CI, and documentation changes. No database or payload migration
is involved.

After v0.20.1 publication, do not restore an MSRV-incompatible or known
vulnerable lock graph. A regression requires a new patch release selecting a
compatible safe graph, with the same stable/MSRV/security evidence.

## Open questions

None. Replacement or retirement of async-std and bincode is deliberately a
future product/compatibility decision bounded by the policy expiry.
