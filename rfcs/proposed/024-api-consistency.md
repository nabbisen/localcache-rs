# RFC 024 — API Consistency: One Surface, Honest Names

| Field | Value |
|---|---|
| Status | Proposed (architect, 2026-09-24) |
| Feature | *(core API; `encryption`, `watching`, and the async features for the wrapper delegations; the CLI crate)* |
| Touches | `crates/localcache/src/pool.rs`, `crates/localcache/src/read_pool.rs`, `crates/localcache/src/cache/async_engine.rs`, `crates/localcache/src/cache/query.rs`, `crates/localcache/src/cache/engine/portable.rs`, `crates/localcache/src/cache/engine/maintenance.rs`, `crates/localcache/src/cache/watcher.rs`, `crates/localcache/src/cache/options.rs`, `crates/localcache/src/lib.rs`, `crates/localcache/tests/` (new `api_surface.rs`), `crates/cli/`, `docs/src/`, `README.md`, `CHANGELOG.md` |
| Finding | Phase 24 plan, milestone Q2 (owner, 2026-09-23); the Phase 24 register entries assigned to Q2; the architect's survey of 2026-09-24 |
| Milestone | Phase 24 Q2 |
| Breaking | **Not in v0.21.5**: additions and deprecations only. Part B decides three behaviour changes and a removal list for **v0.22.0**, which is already breaking |
| Authorship | High-capability model; **reviewed by the owner** (arrangement of 2026-08-01) |
| Handoffs | Created at acceptance (RFC 000, "Companion handoffs") |

## Summary

A user who picks a different handle to the cache should not get a different cache. A name should
say what the code does. Today, neither holds:

- **The three wrappers each forward a different subset of `CacheEngine`.** `ReadPool` lacks two
  read methods. `ConnectionPool` lacks key rotation, which `AsyncCacheEngine` has.
  `AsyncCacheEngine` has no way to start a watcher. And nothing notices when a new engine method
  is not forwarded.
- **Eight names say something the code does not do:**
  - `order_by_updated_at` and `then_by_updated_at` sort by the source file's `mtime`;
  - `SortOrder` is exported, but no public signature accepts it;
  - `ConnectionPool` is one connection behind a mutex, not a pool;
  - `SharedEngine`'s doc calls a `ConnectionPool` "just" an `Arc<Mutex<…>>`;
  - `namespace_copy` is `import_from` under a second name, documented with parameters it does
    not have;
  - `CacheWatcher::watched_count` counts cache entries, not watched paths, and reports a poisoned
    lock as `0`;
  - the CLI's `migrate` copies rather than moves, and silently upgrades the **source** database.
- **One feature gives no capability.** `create_path_index` can only create a duplicate of an
  index every database already has twice. The book presents it as a performance tool.
- **Four contracts surprise users,** and the register assigned their decisions here:
  - a query under the wrong key returns an empty, successful result;
  - an engine that never asked for a journal mode switches an existing database to WAL;
  - imports ignore `max_entries`;
  - the CLI never colours output on Windows.

**Part A (v0.21.5, not breaking):**
- complete each wrapper to a stated rule;
- add a test that fails when a wrapper falls behind;
- add correctly named replacements, and deprecate every misleading name;
- add a reporting query;
- fix the CLI.

**Part B (v0.22.0):** three behaviour changes, each announced in v0.21.5, and the removal list for
Q3 to execute.

## Motivation

### The wrappers disagree

The inventory is from `pub fn` / `pub async fn` in `crates/localcache/src/cache/engine.rs` and its
submodules, `crates/localcache/src/pool.rs`, `crates/localcache/src/read_pool.rs`, and
`crates/localcache/src/cache/async_engine.rs`, at commit `c6d36c5`. **Only the engine methods
missing from at least one wrapper are shown.**

| `CacheEngine` method | `ConnectionPool` | `ReadPool` (read-only) | `AsyncCacheEngine` |
|---|---|---|---|
| `rotate_encryption_key` | **missing** | n/a (writes) | ✓ |
| `entry_count_by_version` | ✓ | **missing** | ✓ |
| `namespace_list` | **missing** | **missing** | **missing** |
| `preload` | **missing** | n/a (writes) | **missing** |
| `import_from` | **missing** | n/a (writes) | missing (see R2) |
| `watcher`, `debounced_watcher` | **missing** | n/a (a watcher removes entries) | **missing** |
| `query` → `query_run` / `query_dry_run` | `query_run` only | both | both |
| `create_path_index` / `drop_path_index` / `list_path_indexes` | missing | missing | ✓ |
| `namespace_copy` | missing | n/a | missing |

`ConnectionPool` documents its purpose as providing "the same API surface" as the engine. Its
`with` escape hatch is described as "for operations not yet exposed on the pool". "Not yet" has
lasted since the type arrived in v0.12.0.

An `AsyncCacheEngine` user **cannot start a watcher at all**, because the type exposes no inner
engine.

No test compares the surfaces, so every new engine method has depended on someone remembering four
files.

### Names that mislead

| Name | What it says | What it does | Found |
|---|---|---|---|
| `QueryBuilder::order_by_updated_at`, `then_by_updated_at` | sort by the entry's `updated_at` | sort by the source file's `mtime` | RFC 021 hazard 1 |
| `SortOrder` (re-exported from the crate root) | a sort-direction parameter | appears in **no** public signature: every sort method takes `ascending: bool` | Phase 24 plan |
| `ConnectionPool` | a pool of connections | one `CacheEngine`, one SQLite connection, one `Mutex`. Its own module doc concedes it, and `ReadPool` is the real pool | 2026-09-24 survey |
| `SharedEngine` / `shared_engine` | "Convenience alias: a `ConnectionPool` is just `Arc<Mutex<CacheEngine<T>>>`" | a bare `Arc<Mutex<CacheEngine<T>>>` with none of `ConnectionPool`'s methods: a second, lesser way to share an engine | 2026-09-24 survey |
| `CacheEngine::namespace_copy` | "Copy all entries from `source_namespace` into `dest_namespace`" | calls `import_from`; the source code itself says the two differ only in "call-site framing". There are no such parameters | 2026-09-24 survey |
| `CacheWatcher::watched_count` | the number of watched paths | the namespace's **entry** count, with `unwrap_or(0)` on both the lock and the query. A poisoned lock reads as "0" | Phase 24 register |
| CLI `migrate` | move a namespace | copies it (`import_from`), and opens the **source** writable, so it may upgrade the source schema as a side effect | Phase 24 plan; 2026-09-24 survey |
| `create_path_index` / `index_hint` | a performance tool (`docs/src/querying.md`) | creates an index on `files(namespace, path)`, which is identical to the `UNIQUE(namespace, path)` autoindex and to `idx_files_namespace_path` (verified in SQLite, 2026-09-23). It cannot make any query faster | Phase 24 register |

### Contracts assigned to Q2

From the Phase 24 register:

1. **`QueryBuilder::run` skips entries it cannot decode**, where `get` returns the error. Under a
   wrong key a query returns `Ok(vec![])`. The Q0a failing-before output showed exactly that.
2. **Every writable open applies `journal_mode`, which defaults to `Wal`.** The CLI's writable
   commands, and any engine built with defaults, silently convert a `Delete`-mode database to WAL,
   and WAL persists in the file.
3. **`max_entries` is enforced by `set`/`batch_set` only,** not by `import_entries`,
   `import_from`, or `namespace_copy`.
4. **The CLI never colours output on Windows.** `atty_check` returns `false` on every non-unix
   target, although `std::io::IsTerminal` works there. Separately, `NO_COLOR` set to the empty
   string disables colour, where the no-color.org convention requires a non-empty value.
5. **`cargo doc --workspace` warns of a filename collision.** The CLI binary and the library are
   both named `localcache`.

### Why now, and why in two parts

Phase 24's rule: v0.21.5 carries additions and deprecations only, and v0.22.0 carries behaviour
changes and removals. A deprecation in v0.21.5 gives users one release of warnings before v0.22.0
removes the item. A behaviour change announced in v0.21.5 gives them one release of notice. That
is the same notice principle as RFC 023 R5 and the newly-rejected-input rule.

## Goals

1. Each wrapper's surface is defined by a **rule**, and a test enforces it.
2. Every misleading name gets a correctly named replacement and a deprecation. **No existing name
   silently changes meaning.**
3. Each register contract gets a decision. Behaviour changes ship only in v0.22.0, with notice in
   v0.21.5.
4. v0.21.5 is not breaking: a consumer's `cargo update` within `0.21` gets warnings, never errors.

## Non-goals

- **Error variants.** Which variant a failure returns is Q3 (RFC 025). This RFC uses existing
  variants, or names Q3's.
- **Removing anything in v0.21.5.** Removal is Q3's item (3), in v0.22.0. This RFC supplies the
  list (B4).
- **The schema.** Whether to drop the built-in duplicate index, and what happens to existing
  `lc_user_*` indexes, is Q5 (RFC 026, schema v6).
- **Key handling.** Zeroization and `aes-gcm 0.11` stay in the register for Q3. Q2 only forwards
  `rotate_encryption_key`; it does not change how keys are held.
- **A true watched-path count.** The watcher keeps no set of watched paths, and nothing has asked
  for one. R7 gives the existing behaviour an honest name. It does not invent a feature.

## Terminology

- **Wrapper**: `ConnectionPool` (renamed by R5), `ReadPool`, or `AsyncCacheEngine`.
- **Delegation**: a wrapper method that calls the same-named `CacheEngine` method, adapting only
  what the wrapper's type requires: locking, the pool checkout, `spawn_blocking`, owned arguments.
- **Deprecate**: `#[deprecated(since = "0.21.5", note = "…")]`, where the note names the
  replacement. Rustdoc and the book are updated in the same slice.

## Part A — v0.21.5 (additions and deprecations only)

### R1 — Each wrapper's surface is a rule

- **The shared-engine wrappers** (`SyncCacheEngine`, formerly `ConnectionPool`, and
  `AsyncCacheEngine`) delegate **every** public `CacheEngine` method, except:
  - `builder`, which is the engine's constructor (each wrapper has `open`);
  - `query`, which returns a builder that borrows the engine. The wrappers expose `query_run`
    and `query_dry_run` instead;
  - deprecated methods (R4–R8), which are never newly delegated;
  - the documented exclusions in R2.
- **`ReadPool`** delegates every public `CacheEngine` method that **never writes**, under the same
  exceptions. A method that writes, including a watcher, which removes entries, is absent by rule.
- A delegation keeps the engine method's name, parameters, and result type. It adapts only what
  the wrapper's type requires (for example, owned `PathBuf` in async).

### R2 — Complete the wrappers

| Wrapper | Add |
|---|---|
| `SyncCacheEngine` | `rotate_encryption_key` (`encryption`), `namespace_list`, `preload`, `import_from`, `watcher` and `debounced_watcher` (`watching`), `query_dry_run` |
| `ReadPool` | `entry_count_by_version`, `namespace_list` |
| `AsyncCacheEngine` | `namespace_list`, `preload`, `watcher` and `debounced_watcher` (`watching`) |

**Exclusion: `AsyncCacheEngine::import_from`.** Its source argument is a `&CacheEngine<U>` that
would have to cross a `spawn_blocking` boundary. Taking another async engine instead would lock
two mutexes, and deadlock when both are the same engine. `export_entries` + `import_entries`,
which the async type already has, is the supported route. The exclusion is recorded in the R3
manifest with that reason.

If `CacheWatcher<T>` turns out not to be `Send` for the async engine's `T`, the async watcher
methods become recorded exclusions instead, with the reason. The implementing slice establishes
which, and reports it.

### R3 — A test that fails when a wrapper falls behind

`crates/localcache/tests/api_surface.rs`, new:

- It reads the source files with `include_str!`, and collects the names of `pub fn` and
  `pub async fn` at `impl` level in the engine files and in each wrapper file. `rustfmt` fixes
  the four-space form, so the scan is deterministic. It sees `#[cfg]`-gated methods regardless of
  enabled features, which is intended: the surface is the same in every build.
- It holds a checked-in **manifest**: for each wrapper, every engine method it deliberately
  omits, with a one-line reason (`constructor`, `borrows the engine`, `writes`, `deprecated
  (RFC 024)`, and so on).
- It **fails** when:
  - an engine method is neither delegated by a wrapper nor in that wrapper's manifest;
  - a manifest entry is **stale**, meaning it names a method the engine no longer has or the
    wrapper now does. The advisory policy fails closed on stale entries for the same reason.
- Name parity is the contract. Signature adaptation is reviewed by hand under R1.

The failing-before demonstration: on `c6d36c5`, the test lists exactly the gaps in the Motivation
table.

### R4 — Sorting: one method, with an honest key and a real direction

Add to `QueryBuilder`:

```rust
pub fn order_by(self, key: SortKey, order: SortOrder) -> Self;
pub fn then_by(self, key: SortKey, order: SortOrder) -> Self;

#[non_exhaustive]
pub enum SortKey {
    /// A JSON field of the payload (`json` feature).
    Field(String),
    /// The source file's modification time when the entry was stored.
    Mtime,
    LastAccessed,
    Path,
}
```

**Deprecate all eight** `order_by_{field,updated_at,last_accessed,path}` and
`then_by_{field,updated_at,last_accessed,path}`.

- The two `*_updated_at` methods are the misleading ones.
- All eight take a bare `ascending: bool`, and `order_by_path(false)` reads as "don't order by
  path". The deprecation notes give the exact replacement, for example
  `order_by(SortKey::Mtime, SortOrder::Asc)`.
- `SortOrder` gains the signature it was exported for.
- **No `SortKey` variant sorts by `updated_at`.** Nothing has asked for it, and a variant named
  after a deprecated misnomer would repeat the confusion. `#[non_exhaustive]` leaves room for a
  distinctly named one later.
- Ordering semantics, including tie-breaking, are unchanged. The deprecated methods become
  one-line calls to the new ones.

`SortKey::Field` is `#[cfg(feature = "json")]`, matching today's `order_by_field` and
`then_by_field`. That is sound because `SortKey` is `#[non_exhaustive]`: no downstream `match` can
depend on the variant set.

### R5 — `ConnectionPool` becomes `SyncCacheEngine`

- Rename the type to **`SyncCacheEngine<T>`**. It is the synchronous counterpart of
  `AsyncCacheEngine<T>`: the same `Arc<Mutex<CacheEngine<T>>>` design, for threads instead of an
  async runtime.
- Keep `#[deprecated] pub type ConnectionPool<T> = SyncCacheEngine<T>;`. Existing code, including
  `ConnectionPool::open(…)`, compiles with a warning.
- **Deprecate `SharedEngine` and `shared_engine`.** They are a second way to share an engine that
  offers none of the wrapper's methods. The deprecation note points to `SyncCacheEngine`, or to
  `Arc::new(Mutex::new(CacheEngine::open(…)?))` for callers who want the bare form.
- `ReadPool` keeps its name. It is a pool.

### R6 — `namespace_copy` is deprecated in favour of `import_from`

The two are one behaviour under two names, and `namespace_copy`'s rustdoc describes parameters
that do not exist. The deprecation note names `import_from`, and `import_from`'s rustdoc gains the
cross-database example.

### R7 — `CacheWatcher::entry_count`

Add `pub fn entry_count(&self) -> Result<usize, LocalFileCacheError>`. It returns the watcher's
engine's entry count, and reports a poisoned lock as `LocalFileCacheError::Poisoned` and a query
failure as its error. Deprecate `watched_count`. Its note says it returns the entry count and
hides errors.

### R8 — The path-index API is deprecated

Deprecate `CacheEngine::create_path_index`, `drop_path_index`, `list_path_indexes`,
`QueryBuilder::index_hint`, and `AsyncCacheEngine`'s three path-index delegations. The note says
the index duplicates the built-in unique index on `(namespace, path)` and cannot speed up a query.

In `docs/src/querying.md` and `docs/src/api.md`, the "path index" material is replaced by one
paragraph saying so. It keeps a pointer to `drop_path_index` for users who created indexes and
want them gone.

### R9 — A query that reports what it skipped

Add:

```rust
pub fn run_report(self) -> Result<QueryReport<T>, LocalFileCacheError>;

#[non_exhaustive]
pub struct QueryReport<T> {
    pub entries: Vec<CacheEntry<T>>,
    /// Entries matched by the query but not returned because they could
    /// not be decoded, with the error each produced, in scan order.
    pub skipped: Vec<SkippedEntry>,
}

#[non_exhaustive]
pub struct SkippedEntry {
    pub path: PathBuf,
    pub error: LocalFileCacheError,
}
```

- `run()`'s behaviour is unchanged in v0.21.5.
- The wrappers gain `query_run_report`, under R1.
- `offset` and `limit` mean exactly what RFC 022 R2 defined: they count returned entries.
  `skipped` lists every undecodable entry the scan passed while producing them.

### R10 — CLI: `copy` covers databases; `migrate` is deprecated

- `copy` gains `--from-db <PATH>`. It defaults to the global `-d/--database`, so today's `copy`
  is unchanged. With it, `copy` copies a namespace between databases.
- **The source is opened read-only.** If the source needs a schema upgrade, the read-only open
  fails as RFC 012 specifies ("read-only open requires the current database schema; … database
  was not modified"). The CLI then prints that message and names the flag **`--upgrade-source`**,
  which opens the source writable and permits the upgrade. Modifying the source is always the
  user's explicit choice.
- `migrate` keeps its exact current behaviour for v0.21.x. It prints one line to stderr:
  `` `migrate` is deprecated and will be removed in 0.22.0; use `copy --from-db` ``. Its help text
  says the same.

### R11 — CLI: colour on every platform, and `NO_COLOR` per its convention

- `atty_check` uses `std::io::IsTerminal` on every platform.
- Colour is disabled only when `NO_COLOR` is set **and non-empty**.

Colour only ever appears when stdout is a terminal, so piped output, which is what scripts
consume, is unchanged.

### R12 — The doc-name collision

Set `doc = false` on the CLI's `[[bin]]` target. `cargo doc --workspace` then documents the
library alone, which is what docs.rs builds anyway.

## Part B — v0.22.0 (behaviour changes, announced in v0.21.5)

Each of these is announced in the v0.21.5 `CHANGELOG.md` summary, along with Q1c and Q3's
rejections.

### B1 — `QueryBuilder::run` returns an undecodable entry's error

In v0.22.0, `run()` returns `Err` with the first undecodable entry's error, as `get` does.
`run_report()` (R9) is the explicit way to tolerate them.

A query under the wrong key then fails loudly instead of returning nothing. For decoding, one rule
then governs `get`, `batch_get` (which already returns each path's own error), and `run`: an error
unless you ask to skip.

### B2 — An engine that does not choose a journal mode leaves the database's alone

- `JournalMode` gains `Preserve`, which becomes the `#[default]`. On a **new** database it applies
  WAL, as today. On an **existing** database it leaves the journal mode unchanged.
- `Wal` and `Delete` stay explicit choices, applied as today.
- `JournalMode` becomes `#[non_exhaustive]` at the same time. It is not today, which is why the
  new variant must wait for v0.22.0.
- The CLI's writable commands inherit `Preserve`, so they stop converting databases.

The migration precondition (a rollback-capable or WAL journal) is unchanged. A `Preserve` open of
a database in `OFF` or `MEMORY` mode is refused by the existing check, as a `Delete` open would be.

### B3 — Imports honour `max_entries`

`import_entries` and `import_from` enforce `max_entries` under **the same rule as `batch_set`**
(RFC 022 R6 and Q3):
- the eviction runs in the import's own `IMMEDIATE` transaction;
- it never evicts a row the call wrote;
- an import of more distinct entries than `max_entries` is rejected before writing, with the
  variant Q3 defines for `batch_set`.

One bound, enforced by every write path.

### B4 — The removal list, for Q3 to execute

Removed in v0.22.0:
- the eight bool sort methods;
- `ConnectionPool` (the alias), `SharedEngine`, and `shared_engine`;
- `namespace_copy`;
- `CacheWatcher::watched_count`;
- `create_path_index` and `QueryBuilder::index_hint`, with their async delegations;
- the CLI `migrate` command.

`drop_path_index` and `list_path_indexes` stay (deprecated) until Q5 decides what schema v6 does
with existing `lc_user_*` indexes. Users need a way to see and drop them until then.

## Detailed design notes

- **Deprecation inside the crate.** Internal uses of deprecated items must not warn: the
  deprecated sort methods delegate to the new ones, and tests of deprecated items carry
  `#[allow(deprecated)]` on the narrowest item. Clippy runs with `-D warnings` on every row, so the
  matrix enforces this.
- **Docs and examples** move to the new names in the same slice as each deprecation. A search for
  each deprecated name in `docs/src/`, `README.md`, and `crates/localcache/examples/` must find
  only the deprecation notes and the migration table.
- **A migration table** in `docs/src/api.md` lists each deprecated item and its replacement.
  v0.22.0's upgrade notes reuse it.
- **New public types** (`SortKey`, `QueryReport`, `SkippedEntry`) are `#[non_exhaustive]`, so they
  can grow without another breaking release.

## Test plan

- **R3:** the failing-before run on `c6d36c5` lists exactly the Motivation's gaps. After R2 it
  passes. A deliberately added engine method with no delegation fails it, and so does a stale
  manifest entry (both demonstrated in the review request, then reverted).
- **R2:** one integration test per added delegation, through the wrapper, on every async backend
  the suite runs for the async additions.
- **R4:** for each key and direction, `order_by(key, order)` returns the same order as the
  deprecated method on the same data, ties included. The existing ordering tests pass unmodified
  under `#[allow(deprecated)]`.
- **R5:** `ConnectionPool::<T>::open(…)` still compiles and works through the alias.
- **R7:** a poisoned watcher engine lock makes `entry_count()` return `Err(Poisoned)`, where
  `watched_count()` returns `0`.
- **R9:** under a wrong key, `run()` returns `Ok(vec![])` (unchanged), and `run_report()` returns
  every row in `skipped` with its decode error. With `offset`/`limit`, the entries equal `run()`'s.
- **R10:** `copy --from-db` across two databases; an old-schema source fails read-only without
  modification (its file hash is unchanged) and succeeds with `--upgrade-source`; `migrate` still
  works and prints its notice.
- **R11:** unit tests for the `NO_COLOR` rule. Windows colour is verified by review, since CI has
  no Windows row.
- **Part B:** specified by its v0.22.0 handoff after this RFC is accepted. Each change gets a
  failing-before test against v0.21.5.

## Security considerations

- R10 removes an implicit write to a source database. A copy no longer changes its input unless
  asked to.
- B1 turns a silently empty result under a wrong encryption key into an error. That is the safer
  failure: a caller can no longer mistake "undecryptable" for "absent".
- R2 exposes `rotate_encryption_key` on `SyncCacheEngine`. That capability is already reachable
  through `with`, so nothing widens.

## Compatibility

- **v0.21.5:** additions and `#[deprecated]` only. Every existing program compiles, with warnings
  where it uses a deprecated name. Behaviour is unchanged, apart from the CLI's colour now
  appearing on Windows terminals (R11). That is cosmetic and terminal-only.
- **v0.22.0:** B1–B3 change behaviour, and B4 removes items. All are announced in v0.21.5.

## Alternatives considered

### Minimal renames only, keeping the `bool` sort methods

This alternative adds `order_by_mtime`/`then_by_mtime` and deprecates only the `*_updated_at` pair.
It is less churn, but it leaves `SortOrder` in no signature, and a re-export cannot be usefully
deprecated. The boolean parameter would also survive. **Rejected** in favour of R4, which resolves
all three with one method pair. It is the first decision this RFC asks of the owner.

### Keep the name `ConnectionPool` and fix its documentation

The documentation would stop claiming it is a pool, but every call site would still say it is.
**Rejected** in favour of R5, because a deprecated alias costs users one warning. This is the
second decision.

### A trait implemented by the engine and every wrapper

The compiler would then enforce parity. But it would put a large public trait into the API,
constrain every future signature, and need `async fn` in traits with `Send`-bound workarounds for
the async wrapper. **Rejected** in favour of R3's test, which enforces the rule with no public
surface.

### Keep `run()` skipping, and only add `run_report()`

This is less disruptive, but a query under a wrong key would stay silently empty by default, and
`get`/`run` would keep disagreeing. **Rejected** in favour of B1. This is the third decision.

### Keep `Wal` as the default and document the conversion

Documentation does not stop a CLI `import` from converting a user's database. **Rejected** in
favour of B2. This is the fourth decision.

### Leave imports unbounded

This keeps one rule for `set` and another for imports. **Rejected** in favour of B3. This is the
fifth decision.

## Rollback

Part A is additions and deprecations. A deprecation can be withdrawn in a patch, and an addition
stays. Part B is withdrawn before v0.22.0 by deleting its slice and its v0.21.5 notice. After
v0.22.0, reversing B1–B3 would itself be a behaviour change.

## Decisions requested of the owner

1. **Sorting (R4):** `order_by(SortKey, SortOrder)` with all eight bool methods deprecated
   (recommended), or minimal renames.
2. **`ConnectionPool` → `SyncCacheEngine` (R5)** (recommended), or keep the name.
3. **B1:** `run()` errors on undecodable entries from v0.22.0 (recommended), or stays skipping.
4. **B2:** `JournalMode::Preserve` as the default from v0.22.0 (recommended), or keep `Wal`.
5. **B3:** imports honour `max_entries` from v0.22.0 (recommended), or stay unbounded.

The slices, which the architect schedules and the owner authorizes with this RFC:

| Slice | Content | Ships |
|---|---|---|
| **Q2c** | R4–R8: names and deprecations, docs, the migration table | v0.21.5 |
| **Q2a** | R1–R3: wrapper completion and `api_surface.rs`. It comes after Q2c, so the manifest records the final names | v0.21.5 |
| **Q2d** | R9: `run_report` and its wrapper delegations | v0.21.5 |
| **Q2e** | R10–R12: the CLI | v0.21.5 |
| **Q2b** | the module split (existing milestone), last, as a pure move | v0.21.5 |
| **Q2f** | B1–B3 | v0.22.0 |
| *(Q3)* | B4 removals | v0.22.0 |

Slice letters are identifiers. `Q2b` was already the module split.

## Open questions

None beyond the five decisions above.
