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
- Guaranteeing byte-identical archives across different `git`, `tar`, or gzip
  implementations. Repeated generation in the supported release environment
  must be deterministic.
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

For the current `cache_bench` target, implementation must choose exactly one:

1. restore and track a meaningful `benches/cache_bench.rs`, retaining
   `criterion` and the benchmark gates; or
2. remove the `[[bench]]` declaration, benchmark tasks, benchmark dependency,
   and documentation claims as one coherent change.

The preferred correction is to restore the benchmark because the project
already documents it as a release-supporting artifact. A placeholder that
only makes Cargo parse is not sufficient.

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

**Proposed resolution:** follow the project rule and place project files at the
archive root. A consumer extracts into a directory of their choice:

```text
mkdir localcache-vX.Y.Z
tar -xzf localcache-vX.Y.Z.tar.gz -C localcache-vX.Y.Z
```

Accepting this RFC with that resolution supersedes the v0.19.0 archive-layout
decision (handoff DEC-007) while retaining the `v`-prefixed archive filename.
The change must be called out in the coming release notes.

If the owner instead chooses the versioned parent directory, the project rule
must be updated in the same design decision. The rule, RFC, Makefile, tests,
documentation, and evidence may not remain contradictory.

### R5 — Safe archive structure

Before extraction, validation must reject an archive containing:

- absolute member paths;
- `..` traversal components;
- unexpected device nodes or other special files; or
- duplicate paths that could make extraction tool-dependent.

After extraction, validation must assert the chosen R4 layout and the presence
of at least:

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

The archive must be extracted into a newly created temporary directory. No
command may rely on the original checkout after extraction.

The minimum M1 smoke gate is:

```sh
cargo metadata --locked --format-version 1
cargo check --workspace --all-targets --all-features --locked
cargo bench --no-run --features json --locked
mdbook build docs
```

The runner must put Cargo build output outside the extracted source directory
so the archive contents remain inspectable. The final M6 archive gate runs the
complete release gate defined below, not only this smoke subset.

### R7 — Canonical stable feature matrix

Cargo features are additive, but `--all-features` exercises only the
highest-priority Tokio async backend. The canonical stable matrix therefore
contains:

1. no optional features;
2. each optional feature individually:
   `async`, `async-std`, `smol`, `compression`, `json`, `encryption`,
   `tracing`, `watching`, `metrics`, and `opentelemetry`;
3. `--all-features`;
4. the full non-Tokio set with `async-std`; and
5. the full non-Tokio set with `smol`.

The exact full non-Tokio sets are derived from the complete feature list, with
the other runtime features removed. They must not be maintained as unrelated
hand-written strings in multiple files.

Every matrix row runs, at minimum:

```sh
cargo clippy --workspace --all-targets --locked ... -- -D warnings
cargo test --workspace --all-targets --locked ...
```

The no-feature row uses `--no-default-features`. Individual rows also start
from `--no-default-features` so their coverage is explicit. The all-feature
row uses `--all-features`.

Formatting is checked once with:

```sh
cargo fmt --all --check
```

### R8 — Declared-MSRV gate

CI and the release gate must read the declared `rust-version` and install that
exact toolchain. A job using stable may not be named or counted as MSRV.

After RFC 014 selects a compatible dependency stack, the declared-MSRV gate
must compile:

```sh
cargo +<declared-msrv> check --workspace --all-targets --all-features --locked
cargo +<declared-msrv> check --workspace --all-targets \
  --no-default-features --features async-std --locked
cargo +<declared-msrv> check --workspace --all-targets \
  --no-default-features --features smol --locked
```

The separate async-std and smol checks are required because the all-feature
priority selects Tokio.

### R9 — Documentation and package gates

The final release gate includes:

```sh
cargo doc --workspace --no-deps --all-features --locked
mdbook build docs
cargo package -p localcache --locked
cargo package -p localcache-cli --locked
```

Package commands run without `--allow-dirty` and without `--no-verify`.
If packaging the CLI requires an explicit version on its path dependency, the
manifest must declare that version and the version-consistency gate must check
it.

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
archive     construct, inspect, extract, and verify source archive
release     all required modes plus evidence summary
```

Names may vary, but there must be one canonical `release` entry point with
fail-fast nonzero status.

### R13 — Security-gate integration point

The release gate must expose a dedicated dependency-security step. RFC 014
defines which advisories deny, warn, or have time-limited exceptions.

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
- stable compiler and Cargo versions;
- exact declared-MSRV compiler and Cargo versions;
- mdBook and security-tool versions;
- every invoked command or canonical gate-mode name;
- per-step exit status and log;
- archive filename, byte size, and SHA-256 digest;
- archive-layout assertion;
- extracted-archive gate results; and
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

## Design

### Canonical release runner

Add a small checked-in release runner under `scripts/` (exact name decided
during implementation). It owns:

- feature-set enumeration;
- command construction;
- tool/version capture;
- gate ordering;
- archive construction and inspection;
- temporary extraction;
- evidence logging; and
- final summary status.

Shell is acceptable if it uses strict error handling, quotes all paths, avoids
evaluating generated text, and has focused self-tests. A small Rust `xtask`
is also acceptable if it does not create a bootstrap cycle that prevents
`cargo metadata` from running when the main manifest is broken.

Because B-01 is a manifest-parse failure, archive preflight and basic source
validation must remain runnable without compiling the workspace.

### Gate ordering

The canonical `release` mode runs in this order:

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
11. full verification from the extracted archive;
12. evidence manifest and final summary.

Later stages must not run after an earlier blocking failure, except that the
runner may finalize a clearly failing evidence summary.

### Local and CI integration

- `Makefile.toml` becomes a thin alias layer over canonical runner modes.
- CI invokes the same runner. It may split feature rows into parallel jobs,
  provided the row definitions come from the canonical source and a final job
  verifies that all required rows completed.
- The declared-MSRV job installs the exact configured version.
- Clippy always receives `-- -D warnings`.
- An archive job uploads the verified source archive and evidence only as CI
  artifacts; it does not publish a release.

### Project archive construction

Preferred flow:

1. verify clean committed `HEAD`;
2. derive `X.Y.Z` from workspace metadata;
3. build an export from `HEAD`, applying reviewed export-ignore rules;
4. compress to `localcache-vX.Y.Z.tar.gz`;
5. list and validate members before extraction;
6. extract into a new temporary directory;
7. verify R4/R5 layout and required contents;
8. run archive gates entirely from the extracted directory;
9. calculate SHA-256; and
10. record the commit-to-digest mapping in evidence.

Two consecutive constructions from the same commit in the supported release
environment must produce the same SHA-256 digest. A self-test checks this
without treating cross-platform compressor differences as a product promise.

### Cargo package distinction

The project source archive and crates.io packages have different audiences:

- the project source archive supports development, documentation, examples,
  tests, and benchmark compilation; and
- Cargo packages contain the files necessary to consume and verify one crate.

Excluding `benches/` from a Cargo package may remain acceptable if Cargo's
normalized package manifest is valid and package verification passes.
Excluding it from the project source archive is not acceptable while the
workspace manifest declares the target.

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

The project source archive layout may change, depending on the R4 owner
decision. If root layout is accepted, the coming release notes must explicitly
supersede the v0.19.0 versioned-parent convention and show the extraction
command.

The `localcache-vX.Y.Z.tar.gz` filename convention remains stable.

## Security considerations

- Constructing from committed tracked files avoids accidentally packaging
  local secrets and `.git-exclude/` review material.
- Clean-tree enforcement prevents evidence from describing a commit while
  testing uncommitted code.
- Archive member validation mitigates path traversal during smoke extraction.
- Release scripts must not print environment variables or registry credentials.
- CI archive jobs receive no publish credentials.
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

### Archive tests

- Required files, including the retained benchmark, are present.
- `.git/`, `.git-exclude/`, `target/`, `docs/book/`, and nested release
  archives are absent.
- Absolute paths and `..` paths are rejected.
- The selected root/parent layout is asserted exactly.
- Two builds from the same clean commit have equal SHA-256 in the supported
  environment.
- `cargo metadata --locked` succeeds from extraction.
- Stable smoke checks, benchmark compile, and mdBook build succeed from
  extraction.
- Final RC runs the complete release gate from extraction.

### Gate-policy tests

- Canonical feature enumeration contains all Cargo features exactly once in
  the individual-feature tier.
- `--all-features`, async-std-only, and smol-only rows are present.
- Every clippy invocation includes `-D warnings`.
- MSRV resolution equals `[workspace.package].rust-version`.
- Library and CLI versions match.
- Stale previous-version installation examples fail the coming-version check.
- Historical changelog references are not rewritten or falsely rejected.
- Archive filename and evidence version match Cargo metadata.

### CI acceptance

- Pull requests run source integrity and stable matrix gates.
- CI runs a real declared-MSRV job.
- CI constructs and verifies an archive from a clean commit.
- A final required job fails if any required matrix or artifact job is absent.
- No CI job performs tag, push, publish, yank, or hosted-release actions.

## Implementation sequence

### M1 — Source and archive recovery

1. Restore the real benchmark target or coherently remove benchmarking.
2. Add source-integrity preflight independent of Cargo compilation.
3. Implement committed-source archive construction.
4. Implement member validation and extracted smoke checks.
5. Resolve R4 and update the authoritative archive rule/decision records.
6. Demonstrate `cargo metadata`, stable all-target check, benchmark compile,
   and mdBook build from both checkout and extraction.

M1 is a focused implementation review point.

### M6 — Complete release controls

1. Integrate the canonical full feature matrix with CI and local aliases.
2. Integrate the exact MSRV policy delivered by RFC 014.
3. Integrate package, security, version, and documentation checks.
4. Set the authorized coming version and complete version-facing housekeeping.
5. Generate a fresh RC archive and evidence bundle.
6. Request independent architecture review.

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

No separate implementation handoff is required for RFC review. If accepted,
an optional release-engineering checklist may be added under
`rfcs/handoffs/009-reproducible-source-archives-and-release-gates/` when the
runner implementation is split across M1 and M6. The checklist must not add or
override design requirements from this RFC.

## Open questions

### OQ-1 — Archive extraction layout (owner decision required)

Choose one authoritative contract:

1. **Project files at archive root (proposed):** complies with the current
   project rule and supersedes v0.19.0 DEC-007; or
2. **Versioned parent directory:** preserves v0.19+ behavior and requires
   changing the project rule.

This must be resolved before RFC acceptance and M1 archive implementation.

### OQ-2 — Benchmark disposition

Proposed resolution: restore the real benchmark source and retain compile-only
benchmark gating. Remove the target only if benchmark maintenance is
explicitly rejected during review.

### OQ-3 — Cross-platform deterministic compression

Proposed resolution: require byte-identical regeneration only in the supported
release environment. Cross-platform archives must have equivalent contents,
but identical gzip bytes are not required.

## Acceptance criteria

This RFC is ready to move to `done/` only when:

- OQ-1 and OQ-2 have explicit owner resolutions recorded in the RFC;
- the repository contains every manifest-required target;
- the project archive contains the full development source required by R2;
- checkout and extracted archive pass the M1 smoke gate;
- CI/local gate definitions use the canonical source;
- exact MSRV, warning, feature, docs, package, and security gates are enforced;
- coming-version references are consistent before release;
- current RC evidence satisfies R14;
- no release action is embedded in the gate; and
- the implemented behavior ships in the approved coming release.
