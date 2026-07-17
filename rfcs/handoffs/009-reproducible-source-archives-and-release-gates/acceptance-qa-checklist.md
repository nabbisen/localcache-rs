# RFC 009 M1 Acceptance and QA Checklist

This checklist operationalizes the accepted
[RFC 009](../../accepted/009-reproducible-source-archives-and-release-gates.md).
The RFC remains authoritative.

## Preconditions

- [ ] RFC 009 is under `rfcs/accepted/` with Status Accepted.
- [ ] The owner has committed the acceptance transition and handoff.
- [ ] The implementation commit under test is identified exactly.
- [ ] The tracked worktree is clean before source-context archive construction.
- [ ] No publish, release, registry, or repository credentials are available
      to verification jobs.

## Manifest and benchmark recovery

- [ ] Every explicit Cargo target has a tracked source.
- [ ] `benches/cache_bench.rs` contains meaningful cases aligned with
      documented benchmark claims.
- [ ] Benchmark compilation uses the target-specific locked command from the
      RFC.
- [ ] Benchmark measurements remain advisory.
- [ ] Removing or bypassing the declared benchmark is rejected.

## Source and artifact contexts

- [ ] Source context rejects a dirty tree and records exact `HEAD`.
- [ ] Source context derives the version from Cargo metadata.
- [ ] Artifact verification runs without `.git/`.
- [ ] Artifact verification cannot construct a nested archive.
- [ ] Expected version and root layout come from the parent orchestrator.
- [ ] The parent binds artifact results to its independently calculated digest.

## Archive construction and provenance

- [ ] The canonical producer uses the exact R16 Linux/amd64 platform digest.
- [ ] Runtime platform, Rust/Cargo 1.97.1, and auxiliary tools match the
      checked-in producer-tool manifest.
- [ ] Locale, timezone, ordering, modes, uid/gid, names, commit-derived mtime,
      and gzip headers match R16.
- [ ] Two consecutive canonical builds from the same commit have identical
      bytes and SHA-256.
- [ ] The archive filename matches the Cargo version.
- [ ] `.git/`, `.git-exclude/`, `target/`, `docs/book/`, and nested archives
      are absent.

## Structured archive validation

- [ ] Validation uses structured headers, not formatted listing text.
- [ ] The exact logical export manifest matches path, type, and executable
      mode.
- [ ] Exactly one canonical `pax_global_header` commit record is accepted.
- [ ] Its sole `comment` value matches independently expected `HEAD`.
- [ ] The validated PAX record is excluded from the logical manifest only
      after commit binding succeeds.
- [ ] A directory record loses exactly one trailing format slash before
      component validation.
- [ ] Raw canonical `git archive` output passes before extraction.
- [ ] Extraction occurs only after full validation into a private new empty
      directory.

## Hostile archive fixtures

- [ ] Absolute and traversal paths are rejected.
- [ ] Empty, `.`, `..`, control-character, NUL, and ambiguous-separator paths
      are rejected.
- [ ] Normalized duplicates and unexpected members are rejected.
- [ ] Symlink and hard-link escapes are rejected.
- [ ] Devices, FIFOs, sockets, and other special entries are rejected.
- [ ] Type and executable-mode mismatches are rejected.
- [ ] Missing, duplicate, malformed, or mismatched global PAX records are
      rejected.
- [ ] Per-entry PAX, unknown PAX keys, GNU extensions, and other extensions are
      rejected.
- [ ] Empty directory names, repeated trailing separators, and interior empty
      components are rejected.

## M1 smoke evidence

- [ ] Checkout and fresh extraction pass `cargo metadata --locked`.
- [ ] Checkout and fresh extraction pass the RFC's package-scoped stable
      all-target checks.
- [ ] Checkout and fresh extraction compile the named benchmark.
- [ ] Checkout and fresh extraction build mdBook.
- [ ] Cargo target output is outside the extracted source.
- [ ] Logs identify commands, tool versions, exit status, commit, version,
      layout, archive size, and SHA-256.
- [ ] A required skipped step makes the summary fail.
- [ ] Evidence contains no environment dump, credentials, secrets, or private
      review material.

## M1 exit and review

- [ ] Every M1 requirement has observed evidence; no unrun gate is called
      passed.
- [ ] The current checkout and extracted artifact meet their applicable M1
      gates.
- [ ] The source archive and evidence are tied to one reviewed commit.
- [ ] A focused implementation review package is created.
- [ ] No tag, push, publish, yank, hosted release, or release authorization is
      performed.

M6-only items remain unchecked until their milestone; M1 must not represent
them as completed.
