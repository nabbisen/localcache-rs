# RFC 024 Implementation Handoff — API Consistency, Part A (v0.21.5)

RFC: `rfcs/accepted/024-api-consistency.md` (accepted 2026-09-24; all five owner decisions as
recommended)
Milestones: Phase 24 **Q2c**, **Q2a**, **Q2d**, **Q2e**, all shipping in v0.21.5. Part B (**Q2f**,
v0.22.0) gets its own handoff after v0.21.5 ships. The module split **Q2b** comes after Q2e, and
gets its own handoff then.
QA companion: `rfcs/handoffs/024-api-consistency/acceptance-qa-checklist.md`

## 0. What this is

**v0.21.5 is not breaking. It allows additions and `#[deprecated]`, and nothing else.**
- No existing name changes meaning.
- No existing behaviour changes, with one stated exception: the CLI's colour on Windows terminals
  (Q2e).
- Nothing is removed.
- No error-variant change. That is Q3.

If a step seems to need any of these, stop and file a design request.

Two standing principles from the owner govern every judgement call:

- **"Finally clean, safe and secure, robust and sophisticated design."**
- **Public APIs must not confuse or mislead users.** This whole RFC exists because of that
  principle. Hold every new name, doc line, and deprecation note to it.

## 1. How to work through the slices

| Slice | RFC | Files (primary) |
|---|---|---|
| **Q2c** names and deprecations | R4–R8 | `crates/localcache/src/cache/query.rs`, `crates/localcache/src/pool.rs`, `crates/localcache/src/lib.rs`, `crates/localcache/src/cache/engine/portable.rs`, `crates/localcache/src/cache/engine/maintenance.rs`, `crates/localcache/src/cache/async_engine.rs`, `crates/localcache/src/cache/watcher.rs`, tests, examples, benches, `docs/src/`, `README.md`, `CHANGELOG.md` |
| **Q2a** wrapper completion and parity test | R1–R3 | `crates/localcache/src/pool.rs`, `crates/localcache/src/read_pool.rs`, `crates/localcache/src/cache/async_engine.rs`, new `crates/localcache/tests/api_surface.rs`, tests, `docs/src/`, `CHANGELOG.md` |
| **Q2d** reporting query | R9 | `crates/localcache/src/cache/query.rs`, the three wrappers, `crates/localcache/src/lib.rs`, tests, `docs/src/querying.md`, `CHANGELOG.md` |
| **Q2e** CLI | R10–R12 | `crates/cli/src/main.rs`, `crates/cli/src/commands/write.rs`, `crates/cli/src/commands/read.rs`, `crates/cli/Cargo.toml`, `crates/cli/tests/`, `crates/cli/README.md`, `CHANGELOG.md` |

**Order: Q2c → Q2a → Q2d → Q2e.** Q2c comes first so that Q2a's parity manifest records the final
names and never delegates a deprecated item.

The cadence is the project's usual one:
1. implement;
2. file the review request describing the **uncommitted** tree;
3. wait for the review in `.git-exclude/reviewed/`;
4. commit with the message the review gives;
5. push only when the review allows.

The tree holds exactly one slice when you file. **Commit and push only your own work, and only
after its review approves it** (owner rule, 2026-09-23).

**Failing-before evidence** applies where the RFC's test plan names it: R3 on `c6d36c5`, R7's
poisoned lock, and R9's wrong key.

**CHANGELOG.** Each slice writes its entries under `## [Unreleased]`, in the same tree:
- `### Added` for new items;
- `### Deprecated` for deprecations, one line per item naming its replacement. This is a new
  section for this file, and Keep a Changelog defines it.

The v0.21.5 release preparation writes the release summary and the v0.22.0 notice. The slices do
not.

**Deprecation mechanics, for every slice:**
- `#[deprecated(since = "0.21.5", note = "…")]`. The note names the exact replacement, for example
  `note = "use order_by(SortKey::Mtime, SortOrder::Asc)"`.
- Internal code never calls a deprecated item. The deprecated item delegates to its replacement,
  never the reverse.
- Tests, examples, benches, and docs move to the new names. **Only** a test whose subject is the
  deprecated item itself keeps it, with `#[allow(deprecated)]` on the narrowest item (the test
  function, not the module). Examples: "the alias still compiles", "the deprecated method orders
  exactly as its replacement".
- Clippy is `-D warnings` on every matrix row, so a stray use fails the gate.

---

## 2. Q2c — Names and deprecations (RFC 024 R4–R8)

### R4 — Sorting

In `crates/localcache/src/cache/query.rs`:

1. Add a public `#[non_exhaustive] #[derive(Debug, Clone, PartialEq, Eq)] pub enum SortKey`:
   - `Field(String)` under `#[cfg(feature = "json")]`, matching today's `order_by_field`;
   - `Mtime`;
   - `LastAccessed`;
   - `Path`.

   Document `Mtime` precisely: the source file's modification time as recorded when the entry was
   stored.
2. Add `pub fn order_by(self, key: SortKey, order: SortOrder) -> Self` and
   `pub fn then_by(self, key: SortKey, order: SortOrder) -> Self`. Their semantics equal today's
   `order_by_*` (clear the keys, set the primary) and `then_by_*` (append).
3. Rename the private `OrderBy::UpdatedAt` to `OrderBy::Mtime`, including its comment. It is
   private, so this is not API. The misnomer goes all the way down today, and this removes it.
4. Deprecate all eight `order_by_{field,updated_at,last_accessed,path}` and
   `then_by_{field,updated_at,last_accessed,path}`. Each becomes a one-line call to `order_by` or
   `then_by`. The notes for the two `*_updated_at` methods say plainly that they sort by the
   source file's `mtime`.
5. Re-export `SortKey` from `crates/localcache/src/lib.rs`, next to `SortOrder`.

**Do not add a `SortKey` variant for `updated_at`** (RFC 024 R4).

### R5 — `SyncCacheEngine`

In `crates/localcache/src/pool.rs`:

1. Rename `pub struct ConnectionPool<T>` to `SyncCacheEngine<T>`, along with its impl blocks and
   rustdoc. The module doc must stop describing it as a pool. Say what it is: one `CacheEngine`
   behind a mutex, the synchronous counterpart of `AsyncCacheEngine`. Point to `ReadPool` for a
   pool of read connections.
2. Add `#[deprecated(since = "0.21.5", note = "renamed to SyncCacheEngine; it is a shared engine, not a pool")] pub type ConnectionPool<T> = SyncCacheEngine<T>;`.
3. Deprecate `SharedEngine` and `shared_engine`, and correct `SharedEngine`'s doc. Its note says:
   use `SyncCacheEngine`, or `Arc::new(Mutex::new(CacheEngine::open(…)?))` for the bare form.
4. **Keep `LocalFileCacheError::Poisoned { resource: "ConnectionPool" }` unchanged.** The string
   is observable, and v0.21.5 changes no behaviour. It changes in v0.22.0 with the removals. Add a
   one-line comment at each site saying so.
5. Export `SyncCacheEngine` and the deprecated alias from `crates/localcache/src/lib.rs`.
6. The example `crates/localcache/examples/connection_pool.rs` (`[[example]] name =
   "connection_pool"` in `crates/localcache/Cargo.toml`): rename the example and its `[[example]]`
   entry to a name that describes what it demonstrates. Read it first: it currently requires the
   `async` feature. Move its code to the new names. **No example may be named after a deprecated
   type.**

### R6 — `namespace_copy`

Deprecate it with the note "identical to import_from; use import_from". Delete the rustdoc's
description of `source_namespace`/`dest_namespace`, which don't exist. Move its cross-database
example to `import_from`'s rustdoc, adapted.

### R7 — `CacheWatcher::entry_count`

In `crates/localcache/src/cache/watcher.rs`:
- Add `pub fn entry_count(&self) -> Result<usize, LocalFileCacheError>`. A poisoned engine lock
  returns `LocalFileCacheError::Poisoned { resource: "CacheWatcher" }` (check what the file
  already uses for this resource, and match it). A query failure returns its own error.
- Deprecate `watched_count`, with the note: "returns the entry count, not watched paths, and
  reports errors as 0; use entry_count".
- `CacheDebouncedWatcher` has no `watched_count` today. Add nothing to it.

**Failing-before:** a test that poisons the watcher's engine lock and shows that `watched_count()`
returns `0`. The same test shows that `entry_count()` returns `Err(Poisoned)`. If the lock cannot
be poisoned from a test without `unsafe` or a test hook, say so, and use the module's existing
`TestPoint` pattern (per-variant `#[cfg]`, one enum per module; review 006 § 3).

### R8 — The path-index API

- Deprecate `CacheEngine::create_path_index`, `drop_path_index`, `list_path_indexes`,
  `QueryBuilder::index_hint`, and `AsyncCacheEngine`'s three path-index methods. The note says the
  index duplicates the built-in unique index on `(namespace, path)` and cannot speed up a query.
  For `drop_path_index` and `list_path_indexes`, add that they remain available to remove indexes
  created earlier.
- **Do not change their behaviour.** The legacy-index compatibility tests
  (`crates/localcache/tests/fixture_integrity/public_boundaries.rs`, `db/indexes/tests.rs`) keep
  testing it, under `#[allow(deprecated)]`.
- `docs/src/querying.md` and `docs/src/api.md`: replace the path-index material with one
  paragraph saying what the index is, that it gives no speed-up, and how to drop one created
  earlier.

### Docs, examples, benches

- Move every use in `docs/src/`, `README.md`, `crates/localcache/examples/`, and
  `crates/localcache/benches/` to the new names.
- For the benches, the change is the call's spelling only; the ordering semantics are identical.
  `benches/scale_profile.rs` is the performance harness, so state in the request that no timed
  code path changed.
- Add a **migration table** to `docs/src/api.md`: each deprecated item → its replacement → "removed
  in 0.22.0" (or "kept until Q5 decides" for `drop_path_index` and `list_path_indexes`).
- Afterwards, `git grep` for each deprecated name across those locations. The only hits allowed
  are the migration table and the deprecation notes themselves. Show the command and its output.

### Tests

- For every `SortKey` and both directions, a test that `order_by(key, order)` returns the same
  order as the deprecated method on the same data, ties included (`#[allow(deprecated)]`).
- The existing ordering tests move to `order_by`/`then_by` and must pass with no change in
  expectations.
- The alias: `ConnectionPool::<Vec<u8>>::open(…)` compiles and works (`#[allow(deprecated)]`).
- R7 as above.

---

## 3. Q2a — Wrapper completion and the parity test (RFC 024 R1–R3)

### R3 first: the test, and its failing-before

Create `crates/localcache/tests/api_surface.rs`:

1. Read, with `include_str!`:
   - the engine: `src/cache/engine.rs`, `src/cache/engine/maintenance.rs`,
     `src/cache/engine/portable.rs`, `src/cache/engine/diagnose.rs`;
   - the wrappers: `src/pool.rs` (`SyncCacheEngine`), `src/read_pool.rs` (`ReadPool`),
     `src/cache/async_engine.rs` (`AsyncCacheEngine`).
2. Collect method names **only from the named type's inherent `impl` blocks**: from a line
   starting `impl` that names the type, to the matching `}` at column 0. Collect lines matching
   `    pub fn NAME` or `    pub async fn NAME`. This must ignore `impl CacheOptionsExt for
   CacheOptions` and any private helper type.
3. **Skip methods marked `#[deprecated]`** on the engine side entirely. A deprecated method is
   never newly delegated (R1), so it needs no manifest entry either. A wrapper's own older
   delegation of one (the async path-index trio) is deprecated with it in Q2c, and the test does
   not check it.
4. The manifest is a `const` table in the test: `(wrapper, engine_method, reason)`. The reasons
   are exactly: `constructor`, `borrows the engine`, `writes`, and
   `spawn_blocking boundary (RFC 024 R2)`, plus `not Send (RFC 024 R2)` if Q2a establishes it.
5. Fail, listing every violation at once:
   - an engine method with no delegation and no manifest entry;
   - a **stale** entry: the engine no longer has the method, or the wrapper now has it.

**Failing-before:** run the test before writing any delegation (the tree after Q2c). Its output
must list exactly the gaps in RFC 024's Motivation table, minus the deprecated items. Capture it.

Then show two demonstrations, and revert both:
- add a dummy `pub fn` to `CacheEngine`, and the test fails naming it;
- add a stale manifest line, and the test fails naming it.

### R2: the delegations

| Wrapper | Add |
|---|---|
| `SyncCacheEngine` | `rotate_encryption_key` (`#[cfg(feature = "encryption")]`), `namespace_list`, `preload`, `import_from`, `watcher` and `debounced_watcher` (`#[cfg(feature = "watching")]`), `query_dry_run` |
| `ReadPool` | `entry_count_by_version`, `namespace_list` |
| `AsyncCacheEngine` | `namespace_list`, `preload`, `watcher` and `debounced_watcher` (`watching`) |

- Each keeps the engine method's name, parameters, and result type. It adapts only what the
  wrapper requires, following each file's existing pattern: `lock()`, the pool checkout, or
  `spawn_blocking` with owned arguments.
- **`AsyncCacheEngine::preload`:** the factory must cross `spawn_blocking`, so it needs
  `Send + 'static`. State the bound in the rustdoc.
- **The async watchers:** establish whether `CacheWatcher<T>` and `CacheDebouncedWatcher<T>` are
  `Send` for the async engine's `T`, with a compile-time assertion (no new dependency; a
  `fn assert_send<X: Send>() {}` call). If they are, delegate them. If not, **do not force it**:
  record them in the manifest with reason `not Send (RFC 024 R2)`, and report it.
- **`AsyncCacheEngine::import_from`** stays absent, as a manifest entry with reason
  `spawn_blocking boundary (RFC 024 R2)`. Its rustdoc does not exist, so put the supported route
  (`export_entries` + `import_entries`) in the async type's module doc.

### Tests

One integration test per added delegation, through the wrapper:
- the async additions on every async backend the suite runs (follow `crates/localcache/tests/pool_observe.rs`'s runtime modules);
- `rotate_encryption_key` through `SyncCacheEngine`, with the same assertions as the engine's own
  rotation test: the engine switches keys, and reads of rotated rows succeed.

---

## 4. Q2d — `run_report` (RFC 024 R9)

1. In `crates/localcache/src/cache/query.rs`, add
   `pub fn run_report(self) -> Result<QueryReport<T>, LocalFileCacheError>`, with the
   `#[non_exhaustive]` public structs `QueryReport<T> { entries, skipped }` and
   `SkippedEntry { path, error }`, exactly as the RFC shows. Re-export both from
   `crates/localcache/src/lib.rs`.
2. `run()` and `run_report()` share **one** execution path. `run()` is `run_report()` with
   `skipped` discarded. There must be no second copy of the materialization loop. RFC 022 R2's
   offset logic lives in exactly one place.
3. `skipped` lists every undecodable entry the scan passed while producing the page, in scan
   order. That includes rows passed while counting `offset`. `offset` and `limit` count returned
   entries only (RFC 022 R2, unchanged).
4. Wrappers: add `query_run_report` to `SyncCacheEngine`, `ReadPool`, and `AsyncCacheEngine`, with
   the same closure shape as each one's `query_run`.
5. `docs/src/querying.md`: a short section on `run_report`, and when to use it. It also announces
   the v0.22.0 change (RFC 024 B1): `run()` will return the first decode error. Use the plain
   future tense, and link the RFC.

**Failing-before:** under a wrong encryption key, v0.21.4's only API, `run()`, returns
`Ok(vec![])`, and nothing reports why. After: `run_report()` returns an empty `entries` and every
row in `skipped` with its decode error. `run()` is unchanged. Also cover a mix of good and bad
rows with `offset`/`limit`: `entries` equals `run()`'s page.

---

## 5. Q2e — CLI (RFC 024 R10–R12)

### R10 — `copy --from-db`; `migrate` deprecated

1. `CopyArgs` gains `--from-db <PATH>`, defaulting to the global `-d/--database`. Without it,
   `copy` behaves exactly as today.
2. The source opens with `read_only: true`. If that fails because the schema is not current, print
   the library's message, then one line naming `--upgrade-source`. Exit non-zero.
3. `--upgrade-source` opens the source writable, permitting the upgrade. It is only meaningful
   with a source that needs one; with a current source it changes nothing.
4. `migrate` keeps its exact current behaviour. It prints to stderr, before anything else:
   `` warning: `migrate` is deprecated and will be removed in 0.22.0; use `copy --from-db` ``.
   Its `--help` text says the same.
5. `crates/cli/README.md` and the top-level command docs move to `copy --from-db`.

**Tests** (`crates/cli/tests/`):
- a cross-database copy;
- an **old-schema source**: read-only fails, and the source file's SHA-256 is unchanged; then
  `--upgrade-source` succeeds;
- `migrate` still works and prints the warning.

Use a fixture **inside the CLI crate** (for example, copy an old-schema database into
`crates/cli/tests/fixtures/`, with provenance noted in a README). Never reference
`../localcache/tests/…`: the published CLI package must be self-contained.

### R11 — Colour

- `atty_check` in `crates/cli/src/main.rs` uses `std::io::IsTerminal` on every platform.
  Delete the non-unix `false` branch.
- `NO_COLOR` disables colour only when set **and non-empty**. Both call sites are in
  `crates/cli/src/commands/read.rs`. Extract **one** helper and use it at both, rather than fixing
  two copies.
- Unit tests for the `NO_COLOR` rule: unset, empty, and non-empty.
- CHANGELOG `### Changed`: CLI colour now works on Windows terminals, and `NO_COLOR=` (empty) no
  longer disables colour.

### R12 — The doc collision

Add `doc = false` to the CLI's `[[bin]]` in `crates/cli/Cargo.toml`. Show
`cargo doc --workspace --no-deps --all-features` before (the collision warning) and after (no
warning).

---

## 6. Not yours — do not do these

- **No removals, no behaviour changes** (other than R11), and no error-variant changes. B1–B4 are
  v0.22.0.
- No schema change. Existing `lc_user_*` indexes are Q5's question.
- No module split, and no file renames under `src/`. That is Q2b, a pure move after Q2e. **Never
  mix a move with a fix.** `pool.rs` keeps its file name in Q2c.
- No MSRV or dependency change.
- No release action. The v0.21.5 summary and its v0.22.0 notice come from the release-preparation
  handoff.

## 7. Gates, for every slice

- `cargo fmt --all --check` clean.
- `python3 scripts/feature_matrix.py --run-all` (what `cargo make matrix` runs) green on every row,
  with clippy `-D warnings`.
- `cargo make msrv-check` under 1.85, in a fresh `--output-dir`. Report every attempt.
- `cargo test --workspace --all-features --locked`: **report the count you observe.**
- `RUSTDOCFLAGS="-D warnings" cargo doc -p localcache --no-deps --all-features --locked`: clean.
  The deprecation notes must render.
- `mdbook build docs` into `.git-exclude/tmp/`.
- `python3 scripts/source_integrity.py --require-tracked` OK.
- `git status --porcelain` shows only the slice's files. Scratch belongs in `.git-exclude/tmp/`.

## 8. Review request, for every slice

File it as `.git-exclude/review-request/NNN-dev-q2X-<topic>-<YYYY-MM-DD>.md`. `NNN` is the next
free number. Name this handoff, the slice, and the RFC requirements at the top.

The contents follow the organization workflow § 9.2:
1. implementation summary;
2. RFC 024 requirements addressed;
3. changed files;
4. important decisions;
5. differences from this handoff;
6. tests added and run;
7. failing-before and passing-after output where required, the deprecated-name `git grep`
   (Q2c), and each slice's CHANGELOG diff;
8. gate results, with the commands as run;
9. unresolved issues;
10. known limitations;
11. requested review focus.

**List every required step, including the ones that pass.** Report every judgement call. Do not
absorb it silently.
