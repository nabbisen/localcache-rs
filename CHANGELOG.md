# Changelog

All notable changes to this project will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.21.0] — 2026-08-01

Consolidation and measurement release closing Phase 22. Release candidate `4b85b88`, accepted by
the N6 release decision on 2026-08-01 with a **Go** recommendation, and authorized by the project
owner the same day.

**Breaking:** `LocalFileCacheError` is now `#[non_exhaustive]`, so exhaustive `match` expressions
must add a `_` arm. Lock poisoning and JSON codec failures also return different variants than
before — see the N1 entry below. No schema, payload wire format, SQL, or method signature changed;
existing databases open unchanged.

Phase 22 N1: [RFC 018](rfcs/done/018-truthful-error-taxonomy.md) — truthful error taxonomy.
**Breaking; targets v0.21.0.**

### Changed

- `LocalFileCacheError` is now `#[non_exhaustive]`. Existing exhaustive `match`
  expressions must add a `_` arm.
- Added `LocalFileCacheError::Poisoned { resource: &'static str }`. Lock
  poisoning in `ConnectionPool`, `AsyncCacheEngine`, `CacheWatcher`
  construction, and `ReadPool` now returns `Poisoned` instead of
  `UnsupportedFeature`.
- **Behaviour change:** `ReadPool`'s read methods were previously infallible
  under lock poisoning and silently recovered the stale guard
  (`unwrap_or_else(|e| e.into_inner())`). They now return
  `Poisoned { resource: "ReadPool" }` — the batch methods (`batch_get`,
  `batch_get_fresh`, `check_status_batch`) return one such error per
  requested path. No method's type signature changed.
- **Behaviour change, not a rename:** JSON encode and decode failures
  (`serialize_json`, `deserialize_json`) now return `Serialization` instead
  of `UnsupportedFeature`. Code matching `UnsupportedFeature` to catch JSON
  errors stops matching.

No schema, payload wire format, SQL, or method signature changed; existing
databases open unchanged. See [Error Handling](docs/src/errors.md#v0210-migration-note)
for the full migration note.

Phase 22 N2: [RFC 019](rfcs/done/019-standing-dispositions-for-unmaintained-dependencies.md)
— standing dispositions for unmaintained dependencies. **Not breaking** — release tooling only.

### Changed

- `security/advisory-policy.json`'s `expires` field is now optional for `unmaintained` and
  `notice` disposition kinds (a **standing disposition** — valid until the version, kind, or
  dependency graph changes, with no expiry date). It remains mandatory for `vulnerability` and
  `unsound`.
- The two live `async-std`/`bincode` unmaintained dispositions were migrated to standing
  dispositions; their `follow-up` now states the condition that would change the decision
  instead of a date. No renewal date was chosen — none is needed.
- Advisory-gate output for a standing disposition reads `standing disposition` and names the
  re-raise condition, instead of printing an expiry date that no longer exists.

No library, schema, payload, or CLI change. See
[Dependency Security](docs/src/dependency_security.md#standing-dispositions-vs-deferred-fixes)
for the full reasoning.

Phase 22 N3: release and security tooling hygiene. **Not breaking** — no library change.

### Fixed

- `command_version` no longer merges a tool's stderr into the string it returns, which is
  both prefix-parsed by the MSRV check and stored verbatim as an evidence field. A
  diagnostic written to stderr on an otherwise-successful run could previously corrupt
  either. Failures still surface stderr, so they stay diagnosable.
- The evidence bundle's `target_triple` is now read from `rustc -vV`'s `host:` line and
  records a real target triple (`x86_64-unknown-linux-gnu`) instead of an invented
  `x86_64-linux`. The richer `platform` field is unchanged.
- `rc_eligible` is now computed from tracked gate completion, a clean-worktree flag, and
  on-disk evidence, rather than three hard-coded `true` literals whose correctness depended
  entirely on control flow.
- A knowingly accepted vulnerability or unsound finding now renders as `EXCEPTION` rather
  than a bare `PASS`, and the `RESULT` line counts exceptions separately.
- Transient sparse-index failures — a network error or HTTP 5xx — now retry up to three
  times with exponential backoff, within the existing overall deadline. A non-5xx status, a
  validation failure, and a size-limit breach are never retried. A flaky security gate gets
  disabled, and a disabled gate is the real risk.
- Packages excluded from advisory coverage because they have no crates.io registry source
  are now enumerated in the evidence bundle with name, version, and reason, so the
  completeness claim no longer requires a manual `Cargo.lock` cross-check.

Phase 22 N5: identifier hygiene and module-size debt. **Not breaking** — no public API change.

### Changed

- `drop_path_index` and `authorize_query_index_in_snapshot` now quote the identifier
  spelling resolved from the SQLite catalog rather than the caller's string (RFC 011 N-01).
  Both were already safe; quoting the catalog value makes that locally evident instead of
  resting on an argument spanning two functions. `create_path_index` deliberately still
  quotes its constructed name — at that point the index does not yet exist, so there is no
  catalog spelling to use.
- `identifier_eq`'s ASCII-only case folding is now documented as deliberate, matching
  SQLite's own identifier comparison (RFC 011 N-02).
- Oversized modules were split along seams the code already declared, with no public API,
  behaviour, or test-count change: `cache/engine.rs` 946 → 762 ELOC
  (`engine/diagnose.rs`, `engine/portable.rs`), `db/indexes.rs` 914 → 338 (its embedded
  test module moved to `db/indexes/tests.rs`), and `cli/src/main.rs` 728 → 257 (the 15
  subcommand handlers split by `DatabaseAuthority` into `commands/read.rs` and
  `commands/write.rs`). `db/repository.rs` and `db/schema/classifier.rs` were deliberately
  left intact — no seam in either earns the churn.

### Security

- `event-listener` moved from 5.4.1 to 5.4.2, resolving **RUSTSEC-2026-0221**: affected
  versions unconditionally implement `Send`/`Sync` for `StackSlot<'_, T>`, allowing a
  `!Send` tag set via `Event::with_tag` to cross a thread boundary and cause a data race in
  safe code. It reaches this crate only transitively, through the optional `async-std` and
  `smol` runtime backends. The patch was taken rather than dispositioned — an unsound
  finding with an available fix should be fixed, not time-boxed.

## [0.20.1] — 2026-07-30

Corrective release closing Phase 21. Release candidate `3005ac2`, accepted by the M7 architecture
re-review on 2026-07-30, which changed the 2026-07-17 recommendation from No-Go to Go, and authorized
by the project owner the same day.

### Security

- The locked dependency graph again supports the declared Rust 1.85 MSRV:
  `rusqlite`/`libsqlite3-sys` and Criterion use compatible lines, while the
  fixable `crossbeam-epoch` vulnerability and `anyhow` unsoundness advisory
  are resolved without broad dependency churn.
- A fail-closed dependency-security gate now binds RustSec results and fresh
  crates.io yanked-state records to the exact lockfile. Vulnerabilities,
  unsoundness, yanked packages, unknown findings, and stale or expired policy
  entries are denied.
- The unmaintained `async-std 1.13.2` and `bincode 2.0.1` findings remain
  visible, exact warnings through 2026-10-21 while the advertised runtime
  feature and established payload wire format are preserved. These warnings
  are not known vulnerabilities and require a new decision before expiry.

### Added

- `CacheWatcher` and `CacheDebouncedWatcher` expose `registration_errors()`,
  `dropped_event_count()`, and `failed_invalidation_count()`, making
  previously silent partial-registration failures, dropped notifications
  (256-slot channel), and failed database removals observable. Registration
  failures are also emitted as `tracing::warn!` when the `tracing` feature is
  enabled. `PathRegistrationError` is a new public type.

### Fixed

- A panic inside `AsyncCacheEngine`'s async-std or smol blocking closure now
  returns `LocalFileCacheError::AsyncTaskPanicked`, matching the existing
  Tokio behavior, instead of propagating as a raw unwind through the async
  runtime.
- A poisoned `AsyncCacheEngine` mutex now returns
  `LocalFileCacheError::UnsupportedFeature` on the next call instead of
  panicking every subsequent caller for the life of the process.
- The two `unsafe` blocks in `AsyncCacheEngine`'s query path are removed;
  `query_run`/`query_dry_run` no longer reinterpret `CacheEngine<T>` as
  `CacheEngine<U>` to support a query result type different from the
  engine's own payload type. The library now contains zero `unsafe` blocks.
- The watcher callback now treats a `contains()` error as "attempt the
  removal anyway" rather than silently skipping the path, and a failed
  database removal is counted rather than discarded; no notification is
  sent for a failed removal.
- Glob compilation is now bounded and panic-free for Unicode input, malformed
  braces, nested/multiple alternatives, and oversized expansion. Directory
  scans and SQLite path queries share one case-sensitive Unicode-scalar
  dialect and reject invalid patterns before filesystem/database work.
- Deleted-source reads, observation, and removal now use only an exact valid
  UTF-8 stored key. Ambiguous basename/suffix deletion and lossy path identity
  conversion are removed; imported portable keys remain unchanged.
- CLI path truncation now preserves UTF-8 boundaries, and `inspect` renders
  schema-v5 modification times as signed nanoseconds with nine fractional
  digits. Seconds/nanoseconds formatting is total across each `i64` domain.
- Read-only engines now validate an exact current schema without initializing
  or migrating it, reject explicit in-memory use, and enforce SQLite read-only
  flags plus `query_only`. All public mutation paths—including `touch`,
  watcher construction, pools, and async adapters—now return the stable
  `ReadOnly` error before other work.
- SQLite path-index identifiers are now constrained and encoded before they
  reach DDL or `INDEXED BY`. Runtime authorization inspects complete index
  shape in the `main` schema within one snapshot, excluding TEMP, attached,
  lookalike, and malformed objects and closing the public SQL-injection path.
- Schema upgrades from supported historical databases are now one atomic
  `IMMEDIATE` transaction. The v0.1/v1 migration preserves payload BLOBs,
  file relationships, IDs, metadata, and AUTOINCREMENT high-water state
  instead of dropping cached payloads.
- Real localcache 0.1.0 databases, whose released schema reports physical
  `user_version = 0`, are classified separately from empty databases and
  migrated through the complete preserving path.
- Historical schema validation now fails closed on malformed/co-located
  objects, invalid sequence state, and unsafe v4 timestamp storage/ranges.
  Caller journal/synchronous settings are delayed until migration commit, and
  post-commit configuration errors explicitly report whether migration
  committed and the requested/observed settings.
- `explain` now compares a stored partial hash against a freshly computed
  partial hash, and a full hash against a full hash, instead of always
  comparing against a full-file digest. The prior comparison reported a
  changed file for every unchanged entry larger than 128 KiB stored under a
  partial-hash detection mode.
- CLI `import --overwrite=false` now actually skips records whose path
  already exists in the target namespace, reporting both the imported and
  skipped counts. Previously the option had no effect and every import
  silently overwrote existing entries regardless of its value; a bare
  `--overwrite` (no value) continues to mean overwrite, unchanged.

### Changed

- Observational CLI commands now open the cache read-only, so inspection cannot
  create or upgrade a database. `copy` keeps its source read-only and opens its
  destination writable first; `migrate` explicitly opens both sides writable
  and may upgrade its source before copying.
- New `create_path_index` suffixes are limited to 1–64 ASCII letters, digits,
  or underscores. Existing structurally valid legacy names remain listable,
  usable, idempotently discoverable, and removable, but out-of-grammar names
  cannot be recreated after removal. Unauthorized query requirements now
  return a stable, non-echoing `UnsupportedFeature` error instead of a SQLite
  `Database` error.
- The repository now uses a virtual Cargo workspace with the `localcache` and
  `localcache-cli` packages under `crates/localcache` and `crates/cli`.
- Project source archives now place project files directly at the archive
  root. Create a new empty destination before extraction; the former
  `localcache-vX.Y.Z/` member prefix is no longer used.
- Source archives are constructed from a clean committed revision and
  structurally validated before a Git-free extracted-source smoke test.

---

## [0.20.0] — 2026-06-06

### Fixed — mtime nanosecond precision (schema v5)

**Bug:** a file overwritten within the same second it was cached, with
byte-identical length, was invisible to all `MetadataOnly` and
`MetadataThenHash` detection modes.  The engine returned `Fresh` (or
served the old payload) instead of `Stale`.

**Root cause:** `mtime` was stored as `modified().as_secs() as i64` —
whole-second precision.  If a file is overwritten within the same
clock second and the size does not change, both the mtime (seconds) and
the file size are identical to the stored values, so the metadata
comparison reports "nothing changed".

**Fix:** `mtime` is now stored and compared as
`modified().as_nanos() as i64` — nanosecond precision.  A same-second
overwrite advances the filesystem's nanosecond counter on every modern
OS/filesystem (Linux ext4 / tmpfs / btrfs, macOS APFS, Windows NTFS
all support sub-second mtime), closing the blind window entirely.

The value fits in an `i64` for all dates through 2262 (`i64::MAX ≈
9.2 × 10¹⁸ ns`).

### Migration — schema v4 → v5

`CacheEngine::open()` (and `initialize()`) automatically migrates
existing databases from schema v4 (seconds) to v5 (nanoseconds) by
running:

```sql
UPDATE files SET mtime = mtime * 1000000000;
```

**First-access behaviour after migration** (one-time per entry):

| Detection mode | First access after upgrade |
|---|---|
| `MetadataThenPartialHash` / `MetadataThenFullHash` | One extra hash to re-validate; payload served from cache |
| `MetadataOnly` | Reports `Stale`; caller recomputes once |
| `StrictFullHash` | Always hashes; unaffected |

After the first `set()` on the upgraded binary, entries are stored with
nanosecond mtime and detection is exact.

### Changed

- `FileMetadata::mtime`: doc comment updated — field is now **nanoseconds**
  since the Unix epoch (was seconds).
- `MetadataDiff::stored_mtime` / `current_mtime`: same unit change.
- `docs/src/migration.md` schema history table updated with v5.
- Schema version constant: `CURRENT_VERSION` = 5.

### Tests

Five new regression tests in `tests/storage.rs`:

- `metadata_only_detects_same_second_same_size_overwrite`
- `metadata_then_partial_hash_detects_same_second_same_size_overwrite`
- `metadata_then_full_hash_detects_same_second_same_size_overwrite` *(the exact reported scenario)*
- `schema_v4_migrates_to_v5_and_entries_are_accessible`
- `fresh_database_is_schema_v5`

The first three skip gracefully on filesystems with whole-second mtime
resolution (via a pre-condition check) to avoid CI flakiness on exotic
environments, while confirming the fix on the standard Linux / macOS CI.

---

## [0.19.1] — 2026-06-06

### Security / Maintenance

This is a dependency maintenance patch — no public API changes.

#### rusqlite 0.39 → 0.40.1

- Bundles **SQLite 3.53.2** (via libsqlite3-sys 0.38.1), up from SQLite 3.51.3
  in rusqlite 0.39 / libsqlite3-sys 0.37.
- All APIs used internally (`params_from_iter`, `OpenFlags`, `prepare_cached`,
  `query_map`, `execute_batch`) were probed in a scratch crate and compile
  and run identically under 0.40 — zero code changes required.

#### aes-gcm: explicit minimum `"0.10.3"` (was `"0.10"`)

- **RUSTSEC-2023-0096**: AES-GCM `< 0.10.3` may have timing variability in
  decryption.  The previous `"0.10"` SemVer bound technically allowed
  0.10.0–0.10.2 (the vulnerable range); the pin is now `"0.10.3"` to make
  the safety invariant explicit.  Cargo.lock was already resolving to 0.10.3;
  this is a tightening of the stated minimum.
- `aes-gcm 0.11.x` remains RC-only (`0.11.0-rc.4` as of this release) —
  not adopted.

#### tokio: explicit minimum `"1.23"` (was `"1"`)

- **RUSTSEC-2023-0001**: Tokio `< 1.23.0` has a vulnerability in
  `tokio::fs::canonicalize` and related I/O paths.  The previous `"1"` bound
  technically allowed any 1.x; the pin is now `"1.23"` to document the
  invariant.  Cargo.lock now resolves to **tokio 1.52.3** (latest stable).
- Note: localcache itself does not call `tokio::fs::canonicalize` (it uses
  `std::path::Path::canonicalize` instead), so the specific vulnerable code
  path was never exercised.  The pin tightening prevents this from becoming
  a concern in future changes.

#### bincode 3 — still a stub, stay on 2.x

- `bincode 3.0.0` on crates.io **remains an intentional
  `compile_error!("https://xkcd.com/2347/")` stub**.  The actual v3
  development lives in the `bincode-next` crate (3.0.0-rc.15 as of this
  release — still not stable).
- No action: we stay on `bincode 2.0.1` with `config::legacy()`.  A probe
  crate was built to confirm the stub status before this entry was written.

#### Other patch updates (via `cargo update`)

tokio 1.52.1 → 1.52.3, serde_json, metrics, inotify, hashbrown, and
various transitive deps updated to their latest compatible patch versions.

---

## [0.19.0] — 2026-06-06

### Added — RFC 007: Read-only Connection Pool (`ReadPool<T>`)

- New `ReadPool<T>` type in `src/read_pool.rs` — a cloneable, `Clone + Send + Sync`
  pool of N independent read-only [`CacheEngine`] connections.
- Checkout strategy: round-robin start, `try_lock` scan across all slots,
  blocking fallback on the start slot — no lock-ordering deadlock.
- **Read-side API only** (write methods are absent from the type):
  `get`, `get_if_fresh`, `batch_get`, `batch_get_fresh`, `check_status`,
  `check_status_batch`, `contains`, `explain`, `keys`, `list_entries`,
  `entry_count`, `cache_stats`, `export_entries`, `scan_dir`,
  `scan_dir_filtered`, `query_run`, `query_dry_run`, `size`.
- `CacheEngineBuilder::build_read_pool(size)` — fluent pool construction.
- Two connection backends controlled by `CacheOptions::shared_cache`:
  - **independent** (default) — each slot opens with plain `read_only` flags;
    fully independent page caches; maximum WAL read parallelism.
  - **shared-cache** — RFC 004 mode; shared page cache across slots;
    lower memory on large pools.
- `:memory:` databases and `size == 0` are rejected at construction with a
  clear `UnsupportedFeature` error.
- `ReadPool` re-exported from `localcache::ReadPool`.

### Added — RFC 008: Compatibility Guarantees

- **Wire-format stability guarantee** formally documented and test-enforced:
  - Documented in `Codec::Bincode` doc comment, `src/serialization.rs`
    module doc, and a new section in `docs/src/migration.md`.
  - Golden fixture database committed: `tests/fixtures/compat-v0_18.sqlite3`
    (written by v0.18.0, Delete journal mode, < 32 KiB).
  - `tests/compat.rs` opens the fixture on every CI run and asserts
    bit-exact payload decodes (`compat_plain_bincode_*`,
    `compat_compressed_*`, `compat_plain_and_compressed_coexist_in_same_db`).
  - `examples/gen_compat_fixture.rs` — the committed, auditable one-off
    generator; marked **do not run routinely**.
- **Path semantics** documented and tested:
  - New "Path handling" section in `docs/src/api.md`: canonicalization
    contract, deleted-file raw-path fallback, `cleanup_missing_files`
    semantics, Windows case-insensitivity behaviour.
  - `src/path.rs` module doc extended with the full path-handling contract.
  - `cleanup_missing_files` doc comment added with case-insensitive
    filesystem note.
  - New regression tests: `path_relative_and_absolute_resolve_to_same_entry`,
    `deleted_file_entry_reachable_by_raw_path_fallback`,
    `cleanup_missing_files_removes_exactly_absent_entries`,
    `cleanup_missing_files_leaves_all_present_entries_intact`,
    `symlink_resolves_to_target_entry` (`#[cfg(unix)]`).

### Changed

- **Release tarball structure** changed from `localcache/(files)` to
  `localcache-vX.Y.Z/(files)`:
  - Archive name now uses a `v` prefix: `localcache-v0.19.0.tar.gz`.
  - Extracted top-level directory matches the archive version:
    `localcache-v0.19.0/`.
- RFC 007 and RFC 008 moved from `rfcs/proposed/` to `rfcs/done/`,
  Status updated to `Implemented (v0.19.0)`.

---

## [0.18.0] — 2026-06-06

### Added — RFC 006: Directory-scoped Query Predicates

- `QueryBuilder::path_in_dir(dir, recursive: bool)` — exact directory
  scoping in SQL:
  - `recursive = false`: matches only **direct children** of `dir`
    (`LIKE 'dir/%' AND NOT LIKE 'dir/%/%'`).
  - `recursive = true`: matches the **entire subtree**
    (`LIKE 'dir/%'`).
  - `dir` is canonicalized when it exists on disk; falls back to the raw
    path string otherwise, so queries over deleted directories still match
    their stored entries.
  - LIKE metacharacters (`\`, `%`, `_`) inside directory names are escaped
    automatically — they always match literally.
- `QueryBuilder::path_glob(pattern)` — glob matching in SQL using the same
  dialect as `ScanOptions::glob_pattern`:
  - `*` — any sequence of characters (SQLite `GLOB *`)
  - `?` — exactly one character (SQLite `GLOB ?`)
  - `{a,b,c}` — brace alternation expanded to `OR`-chained `GLOB` terms
  - `[` in a pattern matches a literal `[`; character classes are
    intentionally unsupported.
- Both predicates AND-combine with `path_like`, `index_hint`, payload
  predicates (json feature), and each other.
- Both predicates are reflected in `dry_run()` / `query_dry_run()` EXPLAIN
  QUERY PLAN output.

### Changed

- `repository::keys()` and `repository::explain_query()` refactored to use a
  shared `build_path_sql()` helper and `rusqlite::params_from_iter` for
  variable-length parameter binding — replaces the two-case fixed-count match.
- `engine::expand_braces` and `split_top_level` promoted from private to
  `pub(crate)` (used by `QueryBuilder::path_glob`).
- `escape_like()` added to `repository.rs` (private) — escapes LIKE
  metacharacters for the `path_in_dir` prefix construction.
- RFC 006 status in `rfcs/proposed/` updated to `Implemented (v0.18.0)`
  and moved to `rfcs/done/`.

---

## [0.17.0] — 2026-06-06

### Added — RFC 001: Recursive Directory Watching (`watching` feature)

- `CacheWatcher::watch_dir(dir)` / `unwatch_dir(dir)` — register an
  entire directory subtree for recursive watching with a single OS watch.
- `CacheDebouncedWatcher::watch_dir(dir)` / `unwatch_dir(dir)` — same
  for the debounced variant.
- `CacheEngineBuilder::watch_dirs(bool)` — opt-in builder flag; when
  `true`, `watcher()` / `debounced_watcher()` auto-register each cached
  path's **parent directory** recursively instead of one OS watch per file.
- Both watcher callbacks now apply a `contains()` membership filter before
  emitting events, ensuring uncached files in watched subtrees are silently
  ignored.
- `CacheOptions::watch_dirs: bool` field (default `false`).

### Added — RFC 002: Query Index Hints and Explain Plan

- `QueryBuilder::index_hint(name)` — injects `INDEXED BY <name>` into
  the path-listing SQL; returns `Err(Database(_))` on an invalid name.
- `QueryBuilder::dry_run()` → `Result<String>` — runs
  `EXPLAIN QUERY PLAN` on the query SQL and returns the plan; no payloads
  are loaded.
- `AsyncCacheEngine::query_dry_run(|q| …)` — async wrapper for
  `dry_run()`.
- `repository::explain_query()` internal function.

### Added — RFC 003: OpenTelemetry Spans

- New `opentelemetry` Cargo feature (`["tracing", "dep:opentelemetry",
  "dep:tracing-opentelemetry"]`).  Pulls in compatible
  `opentelemetry 0.32` + `tracing-opentelemetry 0.33` so callers can
  install `OpenTelemetryLayer` without a version mismatch.  No span sites
  are added; the library emits zero OTel API calls itself.
- **`namespace` field** added to all three existing `tracing` spans
  (`localcache::get`, `localcache::set`, `localcache::check_status`).
  Gated only on `#[cfg(feature = "tracing")]` — improves plain tracing
  output too.
- `check_status` upgraded from inline `debug!` events to a proper
  `debug_span!`, consistent with `get` and `set`.

### Added — RFC 004: Read-only Shared-cache Mode

- `CacheOptions::shared_cache: bool` field (default `false`).
- `CacheEngineBuilder::shared_cache()` builder method.
- When enabled on a **file-backed** database, opens via SQLite `file:` URI
  (`mode=ro&cache=shared`) with `PRAGMA query_only = ON`; implies
  `read_only = true`.
- When enabled on **`:memory:`**, opens `file::memory:?cache=shared` in
  read-write mode — multiple engines in the same process share the named
  in-memory database (useful for testing and in-process pipelines).
- `uri_encode_path()` internal helper (escapes `%`, `#`, `?`, space in
  SQLite URI path components; no extra dependency).

### Added — RFC 005: async-std / smol Feature Variants

- New `async-std` Cargo feature — enables `AsyncCacheEngine` backed by
  `async_std::task::spawn_blocking` (async-std 1.13).
- New `smol` Cargo feature — enables `AsyncCacheEngine` backed by
  `smol::unblock` (smol 2.x).
- New `src/cache/runtime.rs` — `SpawnBlocking` trait with
  `TokioRuntime`, `AsyncStdRuntime`, and `SmolRuntime` impls; public
  `spawn_blocking` dispatch function.
- When multiple runtime features are enabled, priority order is
  **Tokio (`async`) > async-std > smol** — features remain additive,
  keeping `--all-features` and docs.rs working.
- `AsyncTaskPanicked` error variant now covers all three runtime features.
- `AsyncCacheEngine` and `runtime` module gating extended from
  `#[cfg(feature = "async")]` to
  `#[cfg(any(feature = "async", feature = "async-std", feature = "smol"))]`.

### Changed

- **RFC folder structure** — adopted the RFC 000 lifecycle policy.
  All five RFCs moved from the flat `rfcs/` root to `rfcs/done/`; Status
  fields updated to `Implemented (v0.17.0)`.  `rfcs/proposed/` and
  `rfcs/archive/` folders created.  `rfcs/README.md` rewritten as a
  proper lifecycle index.
- `rfcs/done/000-rfc-lifecycle-policy.md` added (self-applying: the
  policy document lives in `done/` because it is implemented).
- **CI matrix** extended with `watching`, `metrics`, `async-std`, `smol`,
  and `opentelemetry` feature combinations.
- **`[[example]]` targets** declared with `required-features` to prevent
  build failures when optional features are absent:
  - `document_pipeline` requires `json,compression`
  - `embedding_cache` requires `json`
  - `connection_pool` requires `async`
- Pre-existing unused-import warnings in `tests/query.rs` and
  `tests/portability.rs` suppressed with `#[allow(unused_imports)]`.

### Fixed

- `repository::keys()` and `repository::explain_query()` — replaced
  `if pattern.is_some() { … pattern.unwrap() … }` with `if let Some(pat)`
  (clippy `clippy::unwrap_used` → clean).
- Redundant raw-pointer cast in `async_engine::query_dry_run` removed
  (clippy `unnecessary_cast` → clean).

---

## [0.16.2] — 2026-05-05

### Added

- **`rfcs/` directory** — implementation-ready design specifications for
  planned features, written in English to match the rest of `docs/`.

  | RFC | Title | Template |
  |-----|-------|----------|
  | [001](rfcs/done/001-recursive-directory-watching.md) | Recursive Directory Watching | Lightweight |
  | [002](rfcs/done/002-query-index-hints.md) | Query Index Hints and Explain Plan | Full |
  | [003](rfcs/done/003-opentelemetry-spans.md) | OpenTelemetry Spans | Full |
  | [004](rfcs/done/004-shared-memory-db.md) | Read-only Shared-memory DB Mode | Full |
  | [005](rfcs/done/005-async-std-smol.md) | async-std / smol Feature Variants | Full |

  Each RFC covers: summary, motivation, public API surface, internal
  design with code sketches, test plan, and (where applicable) security
  considerations and open questions.

---

## [0.16.1] — 2026-05-04

### Changed

- `src/tests.rs` (4 722 lines) split into 8 thematic integration-test files
  under `tests/` alongside `tests/common/mod.rs` for shared helpers.
  Each file is an independent test binary; individual suites can be run with
  e.g. `cargo test --test core`.

  | File | Theme | Tests |
  |---|---|---|
  | `tests/core.rs` | Basic ops, namespaces, TTL | 26 |
  | `tests/storage.rs` | Hashing, scan, schema migration | 30 |
  | `tests/codec_lru.rs` | JSON codec, LRU, glob, encryption | 31 |
  | `tests/builder_ops.rs` | Builder, stats, brace expansion, on_evict | 24 |
  | `tests/portability.rs` | Export / import | 8 |
  | `tests/query.rs` | QueryBuilder, ordering, touch, indexes | 30 |
  | `tests/pool_observe.rs` | ConnectionPool, explain, tracing | 20 |
  | `tests/watching.rs` | Preload, watcher, debounce, metrics, namespace | 19 |

  Total: **188** integration tests + **15** doc-tests (unchanged).

- Two `pub(crate)` internal references removed from the old `src/tests.rs`
  before moving to `tests/` (required for top-level test binaries):
  * `crate::detection::hash::PARTIAL_PREFIX` → literal `"partial:"`
  * `crate::serialization::serialize_bincode` →
    `bincode::serde::encode_to_vec(..., bincode::config::legacy())`

- Unused-import warnings eliminated across all source files and new test
  files; all feature combinations are now warning-free.

---

## [0.16.0] — 2026-05-04

### Changed

- **Documentation overhaul** — the `docs/` mdBook has been completely
  rewritten to reflect the v0.15 API.  All chapters now contain working
  code examples, accurate type names, and feature-flag annotations.

  New chapters added:
  * **Features** — per-feature guide with code examples for all 7 flags.
  * **Builder API** — every `CacheEngineBuilder` method documented.
  * **Async & Thread Safety** — `AsyncCacheEngine`, `ConnectionPool`,
    `shared_engine`, SQLite concurrency model, and a decision table.
  * **Querying the Cache** — `QueryBuilder` predicates, multi-column sort,
    pagination, `explain()`, and namespace management.
  * **File Watching** — `CacheWatcher`, `CacheDebouncedWatcher`, the
    watcher lifetime contract, platform table, and a thread-ownership pattern.
  * **Data Portability** — export/import, `import_from`, `preload()`,
    and glob pattern reference.
  * **Cookbook** — 7 complete recipes: embedding pipeline, multi-threaded
    web server, reactive pipeline, versioned cache, encrypted cache,
    TTL-based expiry, metrics with Prometheus.
  * **CLI Tool** — all 17 subcommands with flags and output examples.
  * **Migration Guide** — bincode 1→2 upgrade, `payload_version` bump,
    DB migration, builder migration, schema version history.
  * **Changelog Summary** — phase-by-phase history from 0.1 to 0.15.

  Updated chapters:
  * **Introduction** — feature comparison table, quick links.
  * **Getting Started** — updated to use builder API, added `preload`.
  * **Change Detection** — all 4 modes, decision table, `explain()` usage.
  * **Architecture** — schema v4, encoding tags, write/read path diagrams.
  * **Error Handling** — full variant table including all feature-gated errors.
  * **API Overview** — complete type catalogue for v0.15.
  * **Roadmap** — completed phases table.

- `docs/book.toml` — repository URL updated to
  `https://github.com/nabbisen/localcache-rs`; search and fold enabled.
- `docs/src/SUMMARY.md` — restructured into **User Guide** / **Reference** /
  **Development** sections.

---

## [0.15.0] — 2026-05-04

### Added

- **`metrics` Cargo feature** — opt-in instrumentation using
  [`metrics 0.24`](https://crates.io/crates/metrics).  When enabled,
  the following counters and histograms are emitted (all labelled
  `namespace = <current namespace>`):
  * `localcache.get.total` — every `get()` call.
  * `localcache.get.hit` — cache hits in `get()`.
  * `localcache.get.miss` — cache misses in `get()`.
  * `localcache.set.total` — every `set()` call.
  * `localcache.set.bytes` — payload size in bytes per `set()`.
  Wire up any `metrics`-compatible exporter (Prometheus, StatsD, …) to
  consume these metrics without changing the `localcache` API.
- **`CacheEngine::namespace_list()`** — returns all distinct namespace
  names present in the current database, sorted alphabetically.
- **`CacheEngine::namespace_copy(source)`** — copy all entries from
  `source` (any `CacheEngine<U>`) into the current engine's namespace,
  replacing conflicts.  Equivalent to `import_from` with a more
  discoverable name for namespace-management workflows.
- **`CacheEngine::debounced_watcher(window)`** → `CacheDebouncedWatcher<T>`
  — a debounced variant of the file-system watcher (requires `watching`
  feature).  All OS events within `window` of each other for the same path
  are collapsed into a single `WatchEvent`, preventing floods caused by
  editors that write files incrementally or applications that flush many
  times per second.
- **`CacheDebouncedWatcher<T>`** — new public type (re-exported under
  `watching` feature).  Has the same `events() -> &Receiver<WatchEvent>`
  lifetime contract as `CacheWatcher`.
- **`notify-debouncer-mini 0.7`** added as an optional dependency (included
  by the `watching` feature alongside `notify 8`).
- **CLI `namespaces` subcommand** — prints a table of all namespaces in the
  database together with their entry counts.
- **`Makefile.toml`** — `cargo-make` task definitions for the full
  development and publish workflow:
  * `cargo make check` — format check + clippy (default, all features,
    no features).
  * `cargo make test-all` — test matrix (no features / default / all
    features).
  * `cargo make pre-publish` — all quality gates before releasing.
  * `cargo make release-check` — version consistency + changelog entry +
    all quality gates.
  * `cargo make publish-all` — publish library then CLI with a 30-second
    propagation delay.

---

## [0.14.0] — 2026-05-03

### Added

- **`watching` Cargo feature** — background file-system watcher using
  [`notify 8`](https://crates.io/crates/notify) (OS-native: `inotify` on
  Linux, `kqueue` on macOS, `ReadDirectoryChanges` on Windows).  Zero
  additional cost when the feature is disabled.
- **`CacheWatcher<T>`** — a background watcher tied to a `CacheEngine`.
  - Created via `CacheEngine::watcher()`.
  - Automatically registers all cached paths at construction time using a
    dedicated SQLite connection for the callback thread.
  - When a watched file is modified, renamed, or deleted, the corresponding
    cache entry is removed from the database and a `WatchEvent` is sent on
    an internal `mpsc::sync_channel`.
  - `watch(path)` / `unwatch(path)` — add/remove paths at runtime.
  - `events()` — borrow the `mpsc::Receiver<WatchEvent>` (watcher must stay
    alive while reading).
  - `watched_count()` — number of entries in the watcher's engine snapshot.
  - **Lifetime note**: dropping `CacheWatcher` stops the OS watcher and
    closes the channel.  Use `events()` (which borrows) rather than
    `into_receiver()` to keep the watcher alive.
- **`WatchEvent { path, reason }`** — new public type.
- **`InvalidationReason`** enum — `FileModified` | `FileRemoved` |
  `FileRenamed`.
- **`CacheEngine::preload(dir, opts, force, factory)`** — bulk-cache all
  files in a directory using a user-supplied `factory` closure:
  * `force = false` — skips entries that are already fresh (cheap check
    before calling `factory`).
  * `force = true` — recomputes every file unconditionally.
  * Returns a `PreloadReport`.
- **`PreloadReport { stored, already_fresh, skipped, errors }`** — new
  public type summarising the preload outcome; per-file error strings for
  skipped files.
- **CLI `watch` subcommand** — prints live invalidation events in the format
  `[YYYY-MM-DD HH:MM:SS] MODIFIED /path/to/file`.  Press Ctrl-C to stop.
  Gracefully exits with an error message when the `watching` feature is not
  compiled in.
- **`watching` feature in `localcache-cli`** — opt-in:
  `cargo build -p localcache-cli --features watching`.

### Dependencies

- `notify = "8"` added as optional dependency (`watching` feature).

---

## [0.13.2] — 2026-05-03

### Changed

- **`bincode` upgraded from 1.3.3 to 2.0.1** with `features = ["serde"]`.
  - `bincode 3.0.0` on crates.io is an intentional stub (see [xkcd #2347](https://xkcd.com/2347/))
    and not a real release; `2.0.1` is the actual latest stable version.
  - All encode/decode calls migrated to `bincode::serde::encode_to_vec` /
    `bincode::serde::decode_from_slice` with `bincode::config::legacy()`.
  - **Wire format is backward-compatible**: `config::legacy()` produces
    byte-for-byte identical output to bincode 1.x, so existing SQLite caches
    require no migration.
  - `LocalFileCacheError::Serialization` inner type changed from
    `Box<bincode::ErrorKind>` to `String` (the bincode 2.x error types are
    not the same as 1.x; `String` avoids exposing an upstream implementation
    detail in the public API).

---

## [0.13.1] — 2026-05-03

### Changed

- Updated repository URL to <https://github.com/nabbisen/localcache-rs> across
  all manifests and documentation.
- `rusqlite` dependency bumped from 0.32 to **0.39** (no API changes to the
  localcache public interface).
- `criterion` dev-dependency bumped from 0.5 to **0.8**; migrated
  `criterion::black_box` → `std::hint::black_box` as required by the new version.
- `base64` bumped from 0.22.0 to **0.22.1** (patch).
- Workspace-level package metadata (`[workspace.package]`) introduced: `version`,
  `edition`, `rust-version`, `authors`, `license`, `repository` are now declared
  once in the root `Cargo.toml` and inherited by `localcache-cli`.
- `criterion::black_box` replaced with `std::hint::black_box` in bench suite.
- `.gitignore`, `LICENSE`, `README.md`, and `.github/` refreshed.
- `NOTICE` copyright year updated to 2026.
- `.github/workflows/ci.yaml` added: matrix test across 7 feature combinations,
  bench compile check, and MSRV check — all using `dtolnay/rust-toolchain@stable`
  so the suite tracks the current stable release rather than a pinned version.
- GitHub Actions versions updated across both workflow files:
  | Action | Version |
  |---|---|
  | `actions/checkout` | **v6** |
  | `actions/cache` | **v5** (Node.js 24, new cache service v2) |
  | `actions/configure-pages` | **v6** |
  | `actions/upload-pages-artifact` | **v5** |
  | `actions/deploy-pages` | **v5** |
  | `dtolnay/rust-toolchain` | **@stable** (was @1.85) |

---

## [0.13.0] — 2025-05-03

### Added

- **`tracing` Cargo feature** — when enabled, key cache operations emit
  structured `tracing` events:
  * `set` — `debug_span` with path, payload bytes, and encoding on completion.
  * `get` — `debug_span` with path; logs "cache hit" or "cache miss".
  * `check_status` — `debug` log with path, status, and reason
    (e.g. `ttl_expired`, `version_mismatch`).
  Zero-cost (compiled out) when the feature is disabled.
- **`CacheEngine::explain(path)`** — returns a [`Diagnosis`] struct with:
  * `status` — overall `CacheStatus`.
  * `entry_exists` / `file_exists` — booleans.
  * `ttl_remaining_secs` — `Some(0)` if expired, `None` if no TTL configured.
  * `hash_match: Option<bool>` — hash comparison result.
  * `metadata_diff: Option<MetadataDiff>` — `mtime` / `file_size` stored vs
    current, with `mtime_changed` / `size_changed` flags.
  * `payload_version: Option<PayloadVersionInfo>` — stored vs expected version,
    with `matches` flag.
  * `summary: String` — human-readable one-liner explaining the status.
- **`Diagnosis`**, **`MetadataDiff`**, **`PayloadVersionInfo`** — new public
  types exported from `localcache`.
- **`AsyncCacheEngine::explain(path)`** and **`ConnectionPool::explain(path)`**
  — async and pooled variants.
- **`QueryBuilder::order_by_last_accessed(ascending)`** — sort by
  `last_accessed_at` timestamp.  Entries never read since being written have
  `last_accessed_at == 0` and sort as "oldest" in ascending order.
- **Multi-column sort** — `order_by` is now a `Vec<OrderBy>` (was
  `Option<OrderBy>`).  Chain secondary sort keys with:
  * `then_by_field(field_path, ascending)` (requires `json` feature)
  * `then_by_updated_at(ascending)`
  * `then_by_last_accessed(ascending)`
  * `then_by_path(ascending)`
- **CLI `inspect <PATH>`** — calls `explain()` and prints a formatted report
  with status, metadata diff, TTL, hash match, and payload version info.
- **`rust-version = "1.85"`** in `Cargo.toml` — makes the MSRV explicit
  (edition 2024 requires Rust ≥ 1.85).

---

## [0.12.0] — 2025-05-03
Benchmarks, `ConnectionPool`, `CacheOptionsExt`, examples, docs.rs metadata.

## [0.11.0] — 2025-05-03
`QueryBuilder` ordering / pagination, `touch`, persistent indexes, CLI `query`.

## [0.10.0] — 2025-05-03
`contains`, `keys`, `QueryBuilder` predicates, CLI `copy` / `migrate`.

## [0.9.0] — 2025-05-03
`export_entries` / `import_entries` / `import_from`, CLI `export` / `import`.

## [0.8.0] — 2025-05-03
Cargo workspace, `localcache-cli`, `on_evict`, multi-group brace expansion.

## [0.7.0] — 2025-05-02
Builder API, `cache_stats`, `check_status_batch`, key rotation.

## [0.6.0] — 2025-05-02
AES-256-GCM encryption, true LRU, glob scan, `list_entries`, schema v4.

## [0.5.0] — 2025-05-02
JSON codec, `max_entries`, `scan_dir_filtered`, version migration.

## [0.4.0] — 2025-05-02
`AsyncCacheEngine`, zstd, `scan_dir`, payload versioning.

## [0.3.0] — 2025-05-02
Partial hash, streaming bincode, read-only, in-memory backend.

## [0.2.0] — 2025-05-02
Namespaces, batch ops, TTL, PRAGMAs, schema migration.

## [0.1.0] — 2025-05-02
Initial release.

[Unreleased]: https://github.com/nabbisen/localcache-rs/compare/v0.16.2...HEAD
[0.20.0]: https://github.com/nabbisen/localcache-rs/compare/v0.19.1...v0.20.0
[0.19.1]: https://github.com/nabbisen/localcache-rs/compare/v0.19.0...v0.19.1
[0.19.0]: https://github.com/nabbisen/localcache-rs/compare/v0.18.0...v0.19.0
[0.18.0]: https://github.com/nabbisen/localcache-rs/compare/v0.17.0...v0.18.0
[0.17.0]: https://github.com/nabbisen/localcache-rs/compare/v0.16.2...v0.17.0
[0.16.2]: https://github.com/nabbisen/localcache-rs/compare/v0.16.1...v0.16.2
[0.16.1]: https://github.com/nabbisen/localcache-rs/compare/v0.16.0...v0.16.1
[0.16.0]: https://github.com/nabbisen/localcache-rs/compare/v0.15.0...v0.16.0
[0.15.0]: https://github.com/nabbisen/localcache-rs/compare/v0.14.0...v0.15.0
[0.14.0]: https://github.com/nabbisen/localcache-rs/compare/v0.13.2...v0.14.0
[0.13.2]: https://github.com/nabbisen/localcache-rs/compare/v0.13.1...v0.13.2
[0.13.1]: https://github.com/nabbisen/localcache-rs/compare/v0.13.0...v0.13.1
[0.13.0]: https://github.com/nabbisen/localcache-rs/compare/v0.12.0...v0.13.0
[0.12.0]: https://github.com/nabbisen/localcache-rs/compare/v0.11.0...v0.12.0
[0.11.0]: https://github.com/nabbisen/localcache-rs/compare/v0.10.0...v0.11.0
[0.10.0]: https://github.com/nabbisen/localcache-rs/compare/v0.9.0...v0.10.0
[0.9.0]: https://github.com/nabbisen/localcache-rs/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/nabbisen/localcache-rs/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/nabbisen/localcache-rs/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/nabbisen/localcache-rs/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/nabbisen/localcache-rs/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/nabbisen/localcache-rs/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/nabbisen/localcache-rs/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/nabbisen/localcache-rs/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/nabbisen/localcache-rs/releases/tag/v0.1.0
