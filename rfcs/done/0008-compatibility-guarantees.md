# RFC 0008 — Compatibility Guarantees: Payload Wire Format and Path Semantics

| Field    | Value |
|----------|-------|
| Status   | Implemented (v0.19.0) |
| Feature  | *(core, no feature flag — documentation + regression tests)* |
| Touches  | `docs/src/migration.md`, `docs/src/api.md`, `src/serialization.rs` (doc comments only), `src/cache/engine.rs` (doc comments only), new `tests/compat.rs`, new `tests/fixtures/` |

## Summary

Promote two behaviours that adopters currently have to infer from
internal notes into **documented, test-enforced guarantees**:

1. **Wire-format stability.** Payloads encoded with the `Bincode` codec
   are readable by every past and future `0.x` release.  Backed by a
   committed golden-fixture database that CI must always be able to
   decode.
2. **Path semantics.** Paths are canonicalized once, at write time;
   lookups canonicalize their input before comparison; maintenance
   operations (`cleanup_missing_files` et al.) operate on the stored
   canonical strings.  Documented precisely, including the Windows
   case-insensitivity consequences.

No behaviour changes.  The deliverables are documentation, doc comments,
and regression tests that turn today's de-facto behaviour into a
contract.

## Background

A downstream adopter (arama, RFC 002) asked two due-diligence questions
before migrating:

- **Q3** — "Is the bincode payload layout guaranteed stable across
  localcache minor versions?  This determines whether we must bump
  `payload_version` on localcache upgrades."
- **Q4** — "Does `cleanup_missing_files()` canonicalize before
  comparing?  We store canonicalized paths and run on case-insensitive
  filesystems (Windows)."

Both have firm answers today, but the answers live in `DEVELOPMENT.md`
dependency notes, `HANDOFF.md` invariants, and the source — none of
which an external adopter reads.  Worse, nothing *enforces* the wire
format: a well-intentioned future change from `config::legacy()` to
`config::standard()` would compile, pass every existing test (they all
write-then-read within one process), and silently corrupt every
existing user database.  The guarantee deserves a tripwire.

## Requirements

### Wire format (Q3)

- A public, documented commitment in `docs/src/migration.md`:
  - the `Bincode` codec uses `bincode::config::legacy()`, byte-compatible
    with databases written by every release since 0.1;
  - this holds for **all 0.x and any future 1.x** releases; it may only
    change behind a schema-version migration that transparently rewrites
    existing payloads, which would be a headline CHANGELOG item;
  - consequence spelled out for adopters: **a localcache upgrade never
    requires a `payload_version` bump** — `payload_version` belongs to
    the *application's* payload schema, not to localcache's.
- The same commitment as a doc comment on `Codec::Bincode` and at the
  top of `src/serialization.rs`.
- A **golden-fixture regression test**: a small SQLite database file,
  committed under `tests/fixtures/`, containing entries written with the
  current release across representative shapes (`Vec<f32>`, a struct
  with `String`/`Option`/nested vec; plus a zstd-compressed entry gated
  on `compression`).  `tests/compat.rs` opens it read-only and asserts
  every payload decodes to the expected values.  The fixture is **never
  regenerated** routinely — regenerating it is precisely the act the
  test exists to make loud.

### Path semantics (Q4)

- A "Path handling" section in `docs/src/api.md` stating the contract:
  - `set` / `get` / `remove` / `contains` / `check_status` canonicalize
    the *input* path (`Path::canonicalize`) when the file exists, and
    fall back to raw-string matching when it does not (so entries for
    deleted files remain reachable);
  - stored keys are therefore canonical paths with **on-disk casing at
    insertion time**;
  - `cleanup_missing_files()` iterates stored strings and tests
    `Path::exists()` directly — no re-canonicalization.  On
    case-insensitive filesystems (Windows, default macOS) a file renamed
    *only by case* still `exists()`, so its entry survives cleanup —
    the correct outcome on such filesystems;
  - the practical rule for applications: always go through the
    `localcache` API rather than comparing stored path strings
    themselves.
- Matching doc comments on `cleanup_missing_files` and in the
  `normalize_path` module.
- Regression tests in `tests/compat.rs` (platform-portable subset):
  - relative vs absolute input resolve to the same entry;
  - symlinked path resolves to its target's entry (Unix-gated);
  - entry for a deleted file is still found by `contains` / removed by
    `remove` via the raw-string fallback;
  - `cleanup_missing_files` removes exactly the entries whose stored
    paths no longer exist, and leaves the rest untouched.

## Design

### Golden fixture generation

A one-off, manually-run generator (not part of the test run) creates the
fixture; the generator source is committed next to the fixture so the
provenance is auditable:

```
tests/fixtures/
  compat-v0_18.sqlite3        ← committed binary fixture (a few KiB)
  README.md                   ← what it contains, how it was generated,
                                and "do not regenerate" policy
scripts/gen-compat-fixture.rs ← `cargo run --example`-style generator
```

`tests/compat.rs` copies the fixture to a temp dir (never opens the
committed file directly — read-only open still creates WAL sidecars next
to the database) and asserts decode equality.

The fixture is generated with `payload_version = 0` and namespace
`compat`, using only stable std/serde types so the expected values can
be spelled out literally in the test.

### Failure mode the test catches

If a future change alters the bincode configuration, the on-disk
encoding tags, the compression framing, or the schema in a
non-migrating way, `tests/compat.rs` fails in CI with a decode error —
*before* any user database is harmed.  Schema migrations that are
intentional must extend the test (open old fixture → migrated →
payloads still decode), which is exactly the discipline the
DEVELOPMENT.md migration policy prescribes.

## Test plan

Covered above; in summary —

- `compat_bincode_payloads_decode` (always),
- `compat_compressed_payload_decodes` (`compression` feature),
- `path_relative_and_absolute_resolve_same_entry`,
- `path_symlink_resolves_to_target` (`#[cfg(unix)]`),
- `deleted_file_entry_reachable_by_raw_fallback`,
- `cleanup_missing_files_exact_set`.

All exercise the public API only, per the testing guidelines.

## Security considerations

None — no behavioural change.  The committed fixture contains only
synthetic data.

## Open questions

1. Should the guarantee be surfaced in `README.md` as well as
   `migration.md`?  Proposed: one sentence in README's design notes
   linking to the full statement.
2. Add a fixture variant per *historical* release (0.13, 0.16, …)?
   Proposed: no — `config::legacy()` makes current-release fixtures
   representative; multiplying binaries adds repo weight without new
   coverage.  Revisit if a schema migration ever lands.
