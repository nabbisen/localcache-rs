# RFC 009 — Reproducible Source Archives and Release Gates

| Field | Value |
|---|---|
| Status | Proposed |
| Feature | *(release engineering; no Cargo feature)* |
| Touches | `Cargo.toml`, `Makefile.toml`, `.github/workflows/ci.yaml`, release scripts, archive metadata, release documentation |
| Findings | Architect review B-01 and B-07 |
| Milestones | Phase 21 M1 (source/archive recovery), completed in M6 (release controls and RC evidence) |

## Summary

Define one enforceable contract for a `localcache` source release:

1. the repository contains every source file required by its Cargo manifests;
2. the project source archive is produced from one clean, committed revision;
3. extracting the archive yields a self-contained Cargo workspace that passes
   defined smoke and release gates;
4. local tasks and CI use the same gate definitions;
5. release evidence identifies the exact version, commit, toolchains,
   commands, archive digest, and results; and
6. pre-release documentation already describes the **coming release version**
   before an actual release or publication occurs.

This RFC repairs the immediate missing-benchmark failure and prevents a release
archive from being called ready when its extracted contents cannot be built.
It does not authorize tagging, publishing, or creating a release.

## Motivation

The v0.20.0 repository declares:

```toml
[[bench]]
name = "cache_bench"
harness = false
required-features = ["json"]
```

but does not contain `benches/cache_bench.rs`. Cargo therefore rejects the
manifest before build, test, clippy, package, or benchmark gates can start.
The current archive task also excludes `benches/`, even though the handoff
acknowledges that the manifest requires the file.

The release controls have additional drift:

- `Makefile.toml` calls a seven-feature string `ALL_FEATURES` while omitting
  `async-std`, `smol`, and `opentelemetry`;
- CI does not deny clippy warnings;
- the job named MSRV installs stable rather than the declared Rust 1.85;
- local tasks and CI spell their gate matrices independently;
- current archive behavior adds a versioned parent directory, while the
  project release rule requires files at the archive root; and
- historical evidence does not bind its claims to the current commit and
  extracted artifact.

A source artifact is part of the product. Testing only the original checkout
does not prove that the delivered archive contains a usable project.

## Goals

- Restore a Cargo-parseable repository.
- Make the project source archive self-contained and self-verifying.
- Establish one canonical release-gate implementation used by local workflows
  and CI.
- Cover every optional feature and every async backend deliberately.
- Run a real declared-MSRV gate.
- Make version, documentation, package, and archive checks part of release
  readiness.
- Produce reviewable evidence tied to an exact release candidate.
- Fail before publication when any release contract is inconsistent.

## Non-goals

- Publishing crates, creating tags, pushing commits, or creating a hosted
  release.
- Resolving the Rust 1.85 dependency incompatibility itself; RFC 014 owns the
  dependency selection and advisory policy.
- Changing library APIs, payload bytes, SQLite schema, or runtime behavior.
- Replacing Cargo as the authority for workspace and package metadata.
- Guaranteeing byte-identical archives across different `git`, archive, or
  compressor implementations. Repeated generation in the canonical producer
  environment must be deterministic; other supported platforms prove
  normalized content equivalence.
- Running performance benchmarks as a release blocker. Benchmark **compilation**
  is a blocker; benchmark measurements remain advisory.

## Terminology

- **Repository checkout** — the clean, committed workspace used to construct a
  release candidate.
- **Project source archive** — the maintainer-delivered `.tar.gz` containing
  the repository's releasable project sources and documentation.
- **Cargo package** — the crate-specific artifact produced by `cargo package`
  for crates.io. This is distinct from the project source archive.
- **Release candidate (RC)** — the coming version after version and
  documentation housekeeping, before actual release or publication.
- **Evidence bundle** — logs and metadata proving which checkout and project
  source archive passed which gates.

## Requirements

### R1 — Manifest completeness

Every explicit Cargo target must have its source present in the repository.
This includes library, binary, example, test, build-script, and benchmark
targets.

For the current `cache_bench` target, the owner resolution is to author and
track a meaningful `benches/cache_bench.rs`, retaining `criterion` and the
compile-only benchmark gate. Repository history contains no tracked copy, so
this is re-authoring rather than recovery. Its cases must correspond to the
project's documented benchmark claims; a placeholder that only makes Cargo
parse is not sufficient.

### R2 — Project archive completeness

The project source archive must contain every tracked file required to run:

- `cargo metadata`;
- workspace build/check/test/clippy gates;
- doctests and examples;
- benchmark compilation;
- mdBook documentation build;
- package inspection/dry runs; and
- the archive smoke test itself.

In particular, a manifest-required target may not be excluded from the project
source archive. Cargo packages may omit development-only files only when
`cargo package` produces a valid normalized manifest and the packaged crate
passes its package verification.

### R3 — Committed-source construction

The project source archive must be generated from a clean, committed revision,
not from an arbitrary working-directory traversal.

The archive command must:

- refuse a dirty worktree;
- record `git rev-parse HEAD`;
- derive the version from Cargo metadata;
- include only tracked, intentionally exported files;
- exclude `.git/`, `target/`, `.git-exclude/`, previously generated archives,
  and generated `docs/book/`;
- avoid following unexpected filesystem content outside the repository; and
- write the archive outside the source tree or to a path excluded from the
  archive input.

`git archive` plus reviewed export rules is the preferred construction
mechanism. An alternative must provide equivalent committed-source and
determinism guarantees.

### R4 — Archive name and extraction layout

The filename remains:

```text
localcache-vX.Y.Z.tar.gz
```

The extraction layout has a current policy conflict:

- `.git-exclude/rules/project-instructions-rust-cli.md` requires project files
  directly at the archive root; and
- the v0.19.0 implementation, changelog, roadmap, and handoff require a
  `localcache-vX.Y.Z/` parent directory.

**Owner resolution (2026-07-17):** follow the project rule and place project
files at the archive root. This supersedes the v0.19.0 versioned-parent
decision. A consumer must create a new empty destination and extract into it:

```text
mkdir localcache-vX.Y.Z
test -z "$(ls -A localcache-vX.Y.Z)"
tar -xzf localcache-vX.Y.Z.tar.gz -C localcache-vX.Y.Z
```

The `v`-prefixed archive filename remains unchanged. The layout change and
empty-destination requirement must be called out in the coming release notes
and maintained user documentation. The project rule, RFC, runner, tests,
documentation, and evidence must all enforce this one root-layout contract.

### R5 — Safe archive structure

Validation must use structured archive-header parsing, not line-oriented
parsing of human-formatted `tar -t` output. Before extraction, it must:

- allow only regular-file and directory entries; symbolic links, hard links,
  device nodes, FIFOs, sockets, and every other entry type are rejected;
- parse and reject link targets even though link entries are forbidden, so
  hostile link fixtures cannot bypass validation through parser drift;
- normalize member names before any comparison;
- reject absolute roots, empty components, `.` or `..` components, ambiguous
  platform separators, NUL or control characters, and any path outside the
  selected R4 root layout;
- reject duplicate normalized paths;
- compare the complete member set against an exact expected export manifest
  derived from committed tracked files and reviewed export rules; and
- compare each member's normalized path, regular-file/directory type, and
  executable mode with that manifest.

Unexpected members are failures even when every required file is present.
Only after every header passes may the verifier extract into a private, newly
created empty directory. After extraction, validation must assert the exact R4
layout and include:

```text
Cargo.toml
Cargo.lock
src/lib.rs
cli/Cargo.toml
cli/src/main.rs
benches/cache_bench.rs        # when the benchmark target is retained
docs/book.toml
docs/src/SUMMARY.md
rfcs/README.md
README.md
CHANGELOG.md
ROADMAP.md
LICENSE
NOTICE
```

### R6 — Extracted-archive smoke gate

Archive construction and artifact verification are separate execution
contexts:

1. the **source-context release orchestrator** requires Git, verifies the clean
   committed checkout, constructs and validates the archive, records the
   commit and archive digest, and invokes artifact verification; and
2. the **artifact-context verifier** requires no `.git/`, does not construct a
   nested archive, and runs the applicable source-integrity, stable/MSRV,
   documentation, package, benchmark, and security gates.

The orchestrator passes the expected version and layout to the artifact
verifier and binds its result to the externally calculated archive digest.
Archive-provided metadata is never trusted as the expected provenance value.

The archive must be extracted into a private, newly created empty temporary
directory. No artifact-context command may rely on the original checkout after
extraction.

The minimum M1 smoke gate is:

```sh
cargo metadata --locked --format-version 1
cargo check -p localcache --all-targets --all-features --locked
cargo check -p localcache-cli --all-targets --all-features --locked
cargo bench -p localcache --bench cache_bench --no-run \
  --features localcache/json --locked
mdbook build docs
```

The runner must put Cargo build output outside the extracted source directory
so the archive contents remain inspectable. The final M6 archive gate runs the
complete **artifact-context** gate defined below, not the source-only Git
preflight or recursive archive construction.

### R7 — Canonical stable feature matrix

Cargo features are additive and workspace dependency feature unification can
invalidate an apparent minimal configuration. Library isolation rows therefore
select only `-p localcache` and use package-qualified feature names.

The canonical **library** stable matrix contains:

1. no optional features;
2. each library feature individually:
   `localcache/async`, `localcache/async-std`, `localcache/smol`,
   `localcache/compression`, `localcache/json`, `localcache/encryption`,
   `localcache/tracing`, `localcache/watching`, `localcache/metrics`, and
   `localcache/opentelemetry`;
3. all `localcache` features;
4. the full non-Tokio library set with `async-std`; and
5. the full non-Tokio library set with `smol`.

Each library row runs, at minimum:

```sh
cargo clippy -p localcache --all-targets --locked \
  --no-default-features ... -- -D warnings
cargo test -p localcache --all-targets --locked \
  --no-default-features ...
```

The no-feature row supplies no `--features` value. Individual rows use the
one package-qualified feature. The all-feature row uses
`-p localcache --all-features`; full non-Tokio rows are derived from the
library feature inventory with the other runtime features removed.

The separate **CLI** matrix covers:

1. `localcache-cli` with its default dependency features; and
2. `localcache-cli/watching`.

Each CLI row selects `-p localcache-cli`; it must not be counted as evidence
for a minimal-feature library row.

Doctests are explicit because `--all-targets` does not select them:

```sh
cargo test -p localcache --doc --locked --all-features
```

The canonical runner reads package-specific feature inventories from Cargo
metadata and compares them with a reviewed checked-in policy. An unassigned
new library or CLI feature fails closed until a row explicitly covers it.
The exact full feature sets must not be maintained as unrelated hand-written
strings in multiple files.

Formatting is checked once with:

```sh
cargo fmt --all --check
```

### R8 — Declared-MSRV gate

CI and the release gate must read the declared `rust-version` and install that
exact toolchain. A job using stable may not be named or counted as MSRV.

After RFC 014 selects a compatible dependency stack, the declared-MSRV gate
must compile package-scoped configurations:

```sh
cargo +<declared-msrv> check -p localcache --all-targets \
  --all-features --locked
cargo +<declared-msrv> check -p localcache --all-targets \
  --no-default-features --features localcache/async-std --locked
cargo +<declared-msrv> check -p localcache --all-targets \
  --no-default-features --features localcache/smol --locked
cargo +<declared-msrv> check -p localcache-cli --all-targets \
  --all-features --locked
```

The separate async-std and smol checks are required because the all-feature
priority selects Tokio.

### R9 — Documentation and package gates

The final release gate includes:

```sh
cargo doc --workspace --no-deps --all-features --locked
mdbook build docs
cargo package --workspace --locked
```

The CLI's local path dependency must also declare a registry-compatible version
requirement that exactly matches the coming library version. The
version-consistency gate enforces that equality.

The canonical producer's pinned Cargo version must prove that joint workspace
packaging verifies the interdependent library and CLI before publication. If
that Cargo version cannot verify the joint operation, the implementation must
use an isolated local-registry fixture and return the RFC for design review
before changing strategy. The gate inspects both generated normalized
manifests and complete package file lists, then verifies both packages.

Packaging runs without `--allow-dirty` and without `--no-verify`. Discovery of
registry propagation behavior may not be deferred to an actual publish.

Generated documentation and package outputs are build artifacts and must not
be added to the project source archive.

### R10 — Coming-version housekeeping

Before final M6 gates, repository metadata is set to the authorized coming
release version. Release-facing files must describe that coming version before
the actual release:

- workspace and member package versions;
- the CLI dependency on the library, when versioned;
- `CHANGELOG.md`;
- `README.md` dependency examples;
- mdBook dependency examples and version-specific statements;
- RFC implementation status for work shipping in the release;
- roadmap milestone/release status; and
- archive filename and evidence metadata.

A version-reference check must detect stale references to the previous release
in maintained installation examples. A deliberate compatibility range or
historical changelog entry must be explicitly excluded rather than silently
rewritten.

This is pre-release housekeeping. It does not mean the release has already
been published or completed.

### R11 — Version and changelog consistency

The release gate must fail unless:

- library and CLI package versions match the coming version;
- the coming version has a non-empty changelog section with its intended
  release date or an explicitly approved RC placeholder;
- the archive filename matches the Cargo version exactly;
- package manifests and user-facing install examples use the approved coming
  version policy; and
- the version has not been marked released in roadmap/RFC prose before owner
  authorization.

### R12 — One gate implementation

The canonical commands and feature sets must have one executable source of
truth under version control. `Makefile.toml` and CI may provide orchestration,
parallelism, and friendly aliases, but they must invoke that source rather
than maintain divergent command lists.

The implementation should provide modes equivalent to:

```text
quick       formatting + metadata + representative check
stable      complete stable feature matrix
msrv        declared-MSRV checks
docs        rustdoc + mdBook
package     crate package verification
verify      artifact-context gates without Git or archive construction
archive     source-context construct, inspect, extract, and invoke verify
release     source-context orchestration plus evidence summary
```

Names may vary, but there must be one canonical `release` entry point with
fail-fast nonzero status and one artifact-context entry point that does not
require or emulate source-context provenance.

### R13 — Security-gate integration point

The release gate must expose a dedicated dependency-security step. RFC 014
defines which advisories deny, warn, or have time-limited exceptions and
provides a machine-readable policy plus the required advisory-database
revision or digest. RFC 009 owns execution, identity capture, and fail-closed
aggregation of that policy.

Until RFC 014 is accepted, this RFC must not hide a nonzero `cargo audit` or
equivalent result. The evidence summary records it as a failing or explicitly
pending gate.

### R14 — Evidence provenance

The final evidence bundle must include:

- project name and coming version;
- UTC timestamp and local timezone;
- exact commit SHA;
- clean-worktree assertion;
- host platform;
- canonical producer environment identity;
- target triple;
- exact `git`, structured archive parser/writer, and compressor versions;
- stable compiler and Cargo versions;
- exact declared-MSRV compiler and Cargo versions;
- mdBook and security-tool versions;
- every invoked command or canonical gate-mode name;
- per-step exit status and log;
- archive filename, byte size, and SHA-256 digest;
- archive-layout assertion;
- extracted-archive gate results; and
- CI workflow run ID, job identity, and commit binding when CI produced the
  evidence;
- RFC 014 policy revision and advisory-database revision/digest; and
- a final pass/fail summary that cannot report pass when a required step was
  skipped.

Evidence must not contain registry tokens, environment dumps, encryption keys,
or inferred secrets. Logs may redact user-specific absolute path prefixes while
preserving command and failure context.

Historical logs from another checkout are never substituted for current RC
evidence.

### R15 — No implicit release action

The release gate ends with a verified release candidate and evidence bundle.
Publishing crates, creating a tag, pushing, yanking, or creating a hosted
release remains a separate owner-authorized action.

### R16 — Canonical producer and CI trust boundary

The owner designates this canonical RC producer environment:

```text
platform: linux/amd64
base tag (informational): docker.io/library/rust:1.97.0-bookworm
platform manifest:
  docker.io/library/rust@sha256:b5a086f64ffecaa4e283063184770107915756739598173e1f5712d6b34b84d0
Rust/Cargo in base: 1.97.0
locale: C.UTF-8
timezone: UTC
mdBook: 0.5.4
cargo-audit: 0.22.2
```

The Linux/amd64 platform digest was verified through Docker Hub's public tag
metadata on 2026-07-17. The mutable tag is recorded only for humans; automation
must pull by the full platform digest and assert `linux/amd64`.

The implementation adds a checked-in producer-tool manifest containing the
exact versions and integrity hashes for every auxiliary binary not already
fixed by the base image, including the structured archive parser/writer and
compressor actually used. The runner verifies that manifest before any gate.
Changing the base digest, platform, archive implementation, or pinned
release-tool version requires design review; it is not an automatic dependency
update.

The canonical producer contract fixes:

- operating-system and target-platform identity;
- `git`, structured archive parser/writer, and compressor versions;
- locale and timezone;
- archive member ordering;
- normalized file modes;
- numeric uid/gid and empty or normalized user/group fields;
- member mtime derived from the reviewed commit timestamp; and
- gzip headers without wall-clock timestamps or source filenames.

For this RFC's first M1/M6 implementation, stable is pinned to Rust/Cargo
1.97.0 by the canonical base. A later stable-toolchain change is a reviewed
producer-manifest update, resolved once before an RC run rather than during
it. The exact selected compiler and Cargo version is used for every stable
source-context and artifact-context gate in the run. Only an archive and
SHA-256 digest produced by the canonical environment may become the release
candidate. Other supported platforms prove behavior and normalized
content-manifest equivalence, not compressed-byte identity.

CI workflow and job permissions are explicitly read-only, normally:

```yaml
permissions:
  contents: read
```

PR, archive, verification, and aggregation jobs:

- receive no publish, release, registry, or repository secrets;
- do not use `pull_request_target` to execute untrusted repository code;
- pin every third-party action to a reviewed immutable commit SHA;
- bind matrix logs, archives, and summaries to one workflow run ID and exact
  commit SHA; and
- fail closed when a required row or artifact is missing, duplicated, stale,
  or bound to another commit.

The CI aggregator verifies these bindings rather than trusting artifact
filenames or human-readable summaries.

## Design

### Canonical release runner

Add a small checked-in release runner under `scripts/` (exact name decided
during implementation). Its source-context orchestrator and artifact-context
verifier share gate definitions. Together they own:

- feature-set enumeration;
- command construction;
- tool/version capture;
- gate ordering;
- archive construction and inspection;
- temporary extraction;
- evidence logging; and
- final summary status.

Shell is acceptable for orchestration if it uses strict error handling, quotes
all paths, avoids evaluating generated text, and has focused self-tests.
Archive headers must still be handled by a structured parser with byte-safe
path validation; shell parsing of formatted archive listings is forbidden. A
small Rust `xtask` is also acceptable if it does not create a bootstrap cycle
that prevents source-integrity validation when the main manifest is broken.

Because B-01 is a manifest-parse failure, archive preflight and basic source
validation must remain runnable without compiling the workspace.

### Gate ordering

The source-context `release` mode runs in this order:

1. preflight: clean tree, required tools, exact commit, version consistency;
2. source integrity: manifest targets and required files present;
3. formatting;
4. stable clippy/test feature matrix;
5. declared-MSRV checks;
6. rustdoc and mdBook;
7. benchmark compile;
8. package verification;
9. dependency-security policy;
10. project archive construction and structural inspection;
11. full artifact-context verification from the extracted archive;
12. evidence manifest and final summary.

Later stages must not run after an earlier blocking failure, except that the
runner may finalize a clearly failing evidence summary.

The artifact-context mode starts at source integrity, skips Git/clean-tree
provenance and archive construction, and runs the applicable code,
documentation, package, benchmark, and security gates. It consumes the
expected version and layout supplied by the parent orchestrator. The parent
binds the result to its independently calculated archive digest.

### Local and CI integration

- `Makefile.toml` becomes a thin alias layer over canonical runner modes.
- CI invokes the same runner. It may split feature rows into parallel jobs,
  provided the row definitions come from the canonical source and a final job
  verifies that all required rows completed with the same workflow run ID and
  commit SHA.
- The declared-MSRV job installs the exact configured version.
- Clippy always receives `-- -D warnings`.
- An archive job uploads the verified source archive and evidence only as CI
  artifacts; it does not publish a release.
- Workflows and actions obey R16's least-privilege and immutable-provenance
  requirements.

### Project archive construction

Preferred flow:

1. verify clean committed `HEAD`;
2. derive `X.Y.Z` from workspace metadata;
3. build an export from `HEAD`, applying reviewed export-ignore rules;
4. compress to `localcache-vX.Y.Z.tar.gz`;
5. parse headers structurally and validate the exact normalized export
   manifest before extraction;
6. extract into a new temporary directory;
7. verify R4/R5 layout and required contents;
8. run archive gates entirely from the extracted directory;
9. calculate SHA-256; and
10. record the commit-to-digest mapping in evidence.

Two consecutive constructions from the same commit in the canonical producer
environment must produce the same SHA-256 digest. The producer fixes ordering,
metadata, commit-derived timestamps, locale/timezone, and gzip headers as
specified by R16. Other supported platforms compare the same normalized
content manifest and applicable behavior without claiming compressed-byte
identity.

### Cargo package distinction

The project source archive and crates.io packages have different audiences:

- the project source archive supports development, documentation, examples,
  tests, and benchmark compilation; and
- Cargo packages contain the files necessary to consume and verify one crate.

Excluding `benches/` from a Cargo package may remain acceptable if Cargo's
normalized package manifest is valid and package verification passes.
Excluding it from the project source archive is not acceptable while the
workspace manifest declares the target.

The CLI path dependency carries the exact coming library version. Both
workspace packages are prepared together using the canonical producer's
supported Cargo command, and both normalized manifests and file lists are
recorded. An isolated local registry is the reviewed fallback boundary, not an
implementation-time substitution: if joint workspace verification is not
supported as designed, implementation pauses for design review.

### Evidence layout

Evidence is generated outside the project source archive. A recommended shape:

```text
localcache-vX.Y.Z-evidence/
  manifest.md
  summary.log
  stable/
  msrv/
  docs/
  package/
  security/
  archive/
    members.txt
    sha256.txt
    smoke.log
```

The manifest identifies which logs are blocking evidence and which are
advisory. Evidence paths and format may change without affecting the library
API, but required provenance fields may not be omitted.

## Error handling

- Every failed command returns a nonzero status.
- Missing tools are failures with installation guidance, not skipped passes.
- A skipped required matrix row makes the final result fail.
- Temporary extraction directories are unique and never reuse the source tree.
- Cleanup failures are reported without overwriting the original gate failure.
- Archive validation occurs before extraction.
- Human-readable summaries must agree with machine exit status.

## Compatibility

This RFC changes no Rust API, payload wire format, or database schema.

The project source archive changes to the owner-approved root layout. The
coming release notes must explicitly supersede the v0.19.0 versioned-parent
convention and show the new-empty-destination extraction command.

The `localcache-vX.Y.Z.tar.gz` filename convention remains stable.

## Security considerations

- Constructing from committed tracked files avoids accidentally packaging
  local secrets and `.git-exclude/` review material.
- Clean-tree enforcement prevents evidence from describing a commit while
  testing uncommitted code.
- Structured archive-header validation, exact export-manifest comparison, and
  rejection of links and special entries prevent path, link-target,
  normalization, and unexpected-member extraction attacks.
- Extraction occurs only after complete validation and only into a private,
  newly created empty directory.
- Release scripts must not print environment variables or registry credentials.
- CI uses read-only permissions, immutable third-party action SHAs, no
  `pull_request_target` execution of untrusted code, and no publish credentials
  or repository secrets in build/verification jobs.
- Dependency scanning is explicit and cannot be silently ignored.
- SHA-256 identifies the exact reviewed artifact; it is an integrity identifier,
  not a signature or proof of publisher identity.

Artifact signing is outside this RFC and may be proposed separately.

## Test plan

### Runner self-tests

- Dirty worktree is rejected.
- Missing explicit target source is rejected before Cargo gates.
- Missing required tool returns nonzero and cannot produce a pass summary.
- Unknown gate mode returns nonzero.
- One deliberately failing matrix row makes the aggregate fail.
- Evidence summary records skipped required steps as failure.
- No evidence output contains a fixture secret marker.
- Artifact-context verification succeeds without `.git/` and rejects attempts
  to construct a nested archive.
- Artifact-context expected version/layout values come from the parent
  orchestrator, not archive-provided claims.

### Archive tests

- The exact path/type/executable-mode export manifest matches; required files,
  including the retained benchmark, are present.
- `.git/`, `.git-exclude/`, `target/`, `docs/book/`, and nested release
  archives are absent.
- Absolute paths; empty, `.` and `..` components; alternate separators;
  NUL/control-character names; normalized duplicates; and unexpected members
  are rejected.
- Symlink and hard-link escape fixtures, link targets, device/special entries,
  and type/mode mismatches are rejected before extraction.
- Structured archive headers are used; a line-oriented listing parser is
  rejected by test.
- The selected root layout and new-empty-destination workflow are asserted
  exactly.
- Two builds from the same clean commit have equal SHA-256 in the canonical
  producer environment.
- Each supported non-canonical platform produces the same normalized content
  manifest.
- `cargo metadata --locked` succeeds from extraction.
- Stable smoke checks, benchmark compile, and mdBook build succeed from
  extraction.
- Final RC runs the complete artifact-context gate from extraction without
  source-context Git or archive-construction steps.

### Gate-policy tests

- Package-specific Cargo metadata inventories match the reviewed library and
  CLI feature policies; a new unassigned feature fails closed.
- Library no-feature and individual-feature rows select only `-p localcache`;
  observed resolved features match the intended row.
- Library all-feature, async-std non-Tokio, and smol non-Tokio rows are present.
- CLI default and `localcache-cli/watching` rows are present and are not
  counted as minimal library coverage.
- An explicit locked library doctest row is present.
- Every clippy invocation includes `-D warnings`.
- MSRV resolution equals `[workspace.package].rust-version`.
- Library and CLI versions match.
- The CLI path dependency's registry version equals the coming library version.
- Joint workspace packaging produces and verifies both packages; both
  normalized manifests and file lists are inspected.
- Stale previous-version installation examples fail the coming-version check.
- Historical changelog references are not rewritten or falsely rejected.
- Archive filename and evidence version match Cargo metadata.

### CI acceptance

- Pull requests run source integrity and stable matrix gates.
- CI runs a real declared-MSRV job.
- CI constructs and verifies an archive from a clean commit.
- Workflows/jobs use explicit read-only permissions, immutable third-party
  action SHAs, no untrusted `pull_request_target` execution, and no publish or
  repository secrets.
- A final required job fails if any required matrix or artifact is absent,
  duplicated, stale, or bound to another workflow run or commit.
- No CI job performs tag, push, publish, yank, or hosted-release actions.

## Implementation sequence

### M1 — Source and archive recovery

1. Record the owner decisions for R1/R4/R16 and designate the canonical
   producer environment.
2. Author the meaningful benchmark target.
3. Add source-integrity preflight independent of Cargo compilation.
4. Implement the source-context orchestrator and bootstrap-safe
   artifact-context verifier.
5. Implement structured member validation, exact export-manifest comparison,
   malicious archive fixtures, and extracted smoke checks.
6. Update the authoritative archive decision records and extraction guidance.
7. Demonstrate `cargo metadata`, stable package-scoped all-target check,
   target-specific benchmark compilation, and mdBook build from both checkout
   and extraction.

M1 is a focused implementation review point.

### M6 — Complete release controls

1. Integrate the canonical package-scoped feature/doctest matrix with CI and
   local aliases.
2. Integrate the exact MSRV policy delivered by RFC 014.
3. Integrate joint workspace package verification, security, version, and
   documentation checks.
4. Enforce R16 CI permissions, immutable actions, and fail-closed provenance
   aggregation.
5. Set the authorized coming version and complete version-facing housekeeping.
6. Generate a fresh canonical-environment RC archive and evidence bundle.
7. Request independent architecture review.

Actual release remains outside implementation and requires owner authorization.

## Alternatives considered

### Keep excluding benchmarks and document the failure

Rejected. A known-unbuildable source archive is not an acceptable release
artifact.

### Test only the repository checkout

Rejected. Archive transforms and exclusions are precisely where the current
defect occurs.

### Continue duplicating commands in Makefile and CI

Rejected. The current feature and warning-policy drift demonstrates that
documentation alone does not keep duplicated gate definitions synchronized.

### Build archives from the working directory with `tar`

Rejected. It can include untracked local files, omit ignored-but-required
files, and produce evidence that does not correspond to the recorded commit.

### Publish first, then update README and docs

Rejected. The release candidate must already describe the coming version so
the artifact and user guidance are reviewed together before publication.

### Require one versioned parent directory regardless of the project rule

Not selected without owner approval. Either layout is mechanically viable;
the defect is that two authoritative sources currently disagree.

## Handoff decision

After acceptance, create a concise release-engineering implementation and QA
handoff under
`rfcs/handoffs/009-reproducible-source-archives-and-release-gates/`.
It must sequence M1/M6 work, malicious archive fixtures, package verification,
CI aggregation, and provenance checks without adding or overriding design
requirements from this RFC.

## Owner decisions

The owner approved all four resolutions on 2026-07-17:

1. **Archive layout:** project files are at archive root, superseding v0.19.0
   DEC-007; users extract only into a newly created empty destination.
2. **Benchmark disposition:** author and retain a meaningful Criterion
   benchmark with target-specific compile-only release gating; measurements
   remain advisory.
3. **Canonical RC producer:** use the exact Linux/amd64 OCI platform digest and
   pinned tool contract in R16. Byte identity is required only there; other
   supported platforms prove normalized content equivalence and behavior.
4. **Durable Accepted state:** adopt RFC 000's five-folder variant. After
   focused re-review recommends acceptance and the owner authorizes the
   transition, move RFC 009 to `rfcs/accepted/`, update its Status and index,
   create its implementation/QA handoff, and only then begin M1.

A review record under `.git-exclude/` remains evidence, not a durable approval
state.

## Acceptance criteria

This RFC is ready to move from `proposed/` to `accepted/` only when:

- B-01 through B-07 from the 2026-07-17 RFC 009 design review are incorporated;
- OQ-1 through OQ-4 have explicit owner resolutions recorded in the RFC;
- focused independent re-review recommends acceptance; and
- the owner authorizes the repository-visible Accepted transition.

It is ready to move from `accepted/` to `done/` only when:

- the repository contains every manifest-required target;
- the project archive contains the full development source required by R2;
- checkout and extracted archive pass the M1 smoke gate;
- CI/local gate definitions use the canonical source;
- exact MSRV, warning, feature, docs, package, and security gates are enforced;
- the canonical producer creates reproducible bytes and other supported
  platforms produce equivalent normalized manifests;
- coming-version references are consistent before release;
- current RC evidence satisfies R14;
- no release action is embedded in the gate; and
- the implemented behavior ships in the approved coming release.
