# RFC 012 — Read-only Schema and Mutation Contract

| Field | Value |
|---|---|
| Status | Proposed |
| Feature | *(core; parity for `watching` and async runtime features)* |
| Touches | `crates/localcache/src/cache/engine.rs`, schema validation, watcher construction, pool/async forwarding tests, `crates/cli/src/main.rs`, read-only documentation |
| Finding | Architect review B-04 and CLI read-only observation |
| Milestone | Phase 21 M3 |

## Summary

Make read-only mode a capability enforced at every layer rather than a flag
which most write methods happen to inspect.

A file-backed read-only engine opens SQLite with read-only flags, enables and
verifies connection-local `query_only`, and validates the complete current v5
schema in one read snapshot. It never creates or migrates a schema and rejects
empty, historical, future, or malformed schemas without mutation.

Every public operation which can change database state must return
`LocalFileCacheError::ReadOnly` before validating other arguments or performing
other work. This includes `touch` and both watcher constructors. Read-only
watcher construction must not create a write-capable helper connection.
Connection pools and async wrappers inherit the same boundary. CLI commands
which only observe the cache must explicitly open the database read-only so
that listing or diagnosis cannot create or migrate it.

This is a corrective patch-release contract. It changes no schema, payload
format, or public method signature. Proposed status does not authorize
implementation, close B-04, complete M3, or authorize a release.

## Motivation

The current boundary has three independently observable gaps:

1. `CacheEngine::open` uses SQLite read-only flags for file-backed readers but
   skips RFC 010 schema classification. It enables foreign keys and accepts any
   database SQLite can open, including a schema newer than v5. This contradicts
   the external error contract that a schema newer than the build returns
   `UnsupportedFeature`. It also lets historical or malformed shapes reach
   later queries without a deliberate compatibility decision.
2. `CacheEngine::touch` writes `last_accessed_at` without calling the shared
   write guard. SQLite usually rejects the statement at a lower layer, but the
   public contract promises the typed `ReadOnly` boundary and must not depend
   on the connection backend to recover a missing API guard.
3. `watcher()` and `debounced_watcher()` create new engines with
   `read_only: false`. A watcher requested from a read-only engine can therefore
   obtain a write-capable connection and delete rows after filesystem events.

The CLI has the same policy drift at a higher layer. Its base options say
inspection is read-only but leave `read_only` at the writable default. Commands
such as `list`, `stats`, and `inspect` can create a missing database or migrate
an older one merely to report information.

SQLite read-only open flags are necessary but not sufficient as the public
contract. The library must also decide which schemas a read-only build can
understand, provide stable typed errors before unrelated validation, and
prevent helpers from minting stronger authority than their originating
engine.

## Goals

1. Accept a read-only engine only for a complete current v5 database.
2. Reject fresh, historical, future, and malformed schemas without mutation.
3. Make every database-mutating public API return `ReadOnly` consistently.
4. Prevent ordinary and debounced watchers from escalating a read-only engine.
5. Preserve pure reads, including query hints and index listing, on valid v5.
6. Apply the same contract through `ConnectionPool` and async wrappers.
7. Make observational CLI commands unable to create or migrate the cache.
8. Prove the boundary with complete before/after state and error-precedence
   tests.

## Non-goals

- Changing schema v5, `PRAGMA user_version`, migration algorithms, payload
  bytes, or RFC 010 classification policy.
- Supporting read-only access to historical schemas through compatibility
  queries. Historical versions require the existing writable migration path.
- Making watcher registration, event delivery, channel overflow, or callback
  errors reliable. RFC 015 owns those operational-failure semantics.
- Changing Unicode path, glob, deleted-path, or CLI truncation behavior. RFC
  013 owns those corrections.
- Adding a new public error variant or changing public method signatures.
- Making `CacheEngine<T>` a compile-time read-only type. `ReadPool<T>` already
  supplies a reduced read-only surface where that abstraction is appropriate.
- Preventing other processes or independently opened writable engines from
  changing the same database.
- Treating output files written by CLI `export` as cache-database mutations.

## Terminology

- **Explicit read-only**: `CacheOptions::read_only == true`.
- **Implied read-only**: a file-backed engine opened with
  `CacheOptions::shared_cache == true`, as specified by RFC 004.
- **Effective read-only**: explicit or implied read-only for a file-backed
  database.
- **Observational operation**: an operation whose contract does not change
  cache schema or rows. Reading source-file metadata is observational.
- **Mutating operation**: an operation which may change schema, cache rows,
  payload bytes, index definitions, LRU timestamps, or database storage.
- **Authority inheritance**: a helper created by an engine may not receive
  greater database-write authority than that engine.
- **Current schema**: the exact RFC 010-recognized v5 schema, including allowed
  indexes, constraints, foreign keys, sequence state, and object policy.

## Requirements

### R1 — Effective read-only connection capability

For a file-backed database, effective read-only is true when either
`read_only` is explicitly true or `shared_cache` implies read-only. The
connection must be opened with SQLite's read-only flag. Shared-cache URI opens
must retain both `mode=ro` and the read-only flag.

Before schema inspection, every effective read-only connection must set
`PRAGMA query_only = ON` and verify that it reads back as enabled. This is
connection-local defence in depth; it does not replace the operating-system
open flags or the public API guard. Foreign-key enforcement must likewise be
enabled and verified without applying caller-selected journal or synchronous
configuration.

An explicit read-only request for an in-memory database is rejected with
`UnsupportedFeature`. There is no pre-existing private `:memory:` database to
inspect, and initialization would itself be a write. The existing
`shared_cache + :memory:` special case remains read-write only when the caller
did not also request explicit read-only. This preserves the documented RFC 004
testing convenience without silently ignoring an explicit restriction.

A read-only open of a nonexistent file must not create the file. The native
SQLite open failure remains a `Database` error; localcache must not retry with
create or read-write flags.

### R2 — Current-schema validation without migration

Schema code must expose one read-only validation entry point separate from
`initialize`. The read-only path must:

1. begin one Deferred transaction after the connection-only safety PRAGMAs;
2. read `main.user_version` inside that transaction;
3. invoke RFC 010's existing strict classifier in the same snapshot;
4. accept only `SchemaState::Version { version: 5, .. }`; and
5. commit the read transaction before returning the engine.

The validator must not create tables, migrate, change `user_version`, apply
runtime journal/synchronous settings, repair objects, or retry through the
writable initializer.

Classification behavior remains fail-closed:

- an exact v5 schema succeeds;
- a valid physical v1-v4 schema is recognized but rejected because read-only
  mode cannot migrate it;
- the released physical-version-0 v0.1 shape is recognized as historical and
  rejected for the same reason;
- an empty version-0 database is rejected because read-only mode cannot
  initialize it;
- future, negative, or otherwise unsupported physical versions use RFC 010's
  stable unrecognized-schema `UnsupportedFeature` contract; and
- malformed or co-located schemas use the existing classifier rejection and
  remain unchanged.

Recognized fresh or historical states return `UnsupportedFeature` with this
stable message:

```text
read-only open requires the current database schema; initialization or migration is not permitted; database was not modified
```

The message deliberately does not suggest that reopening writable is safe
without the caller's normal backup and migration planning. An explicit
read-only in-memory request returns `UnsupportedFeature` with the stable
message `read-only mode does not support in-memory databases`.

One open decision belongs to one SQLite snapshot. A different process may
change the database after that transaction commits; later SQLite/schema errors
are not converted into a false continuing guarantee.

### R3 — Complete public mutation guard

`CacheEngine::guard_write` remains the single typed policy check. Every
mutating public method must invoke it before path normalization, payload
encoding, input parsing, index-name validation, source scanning, callback or
factory invocation, database lookup, transaction creation, filesystem watcher
registration, or helper-engine construction.

The guarded surface includes:

| Category | Methods |
|---|---|
| Entry writes | `set`, `batch_set`, `remove`, `touch` |
| Bulk/import writes | `import_entries`, `import_from`, `preload`, `namespace_copy` |
| Maintenance | `cleanup_missing_files`, `cleanup_expired`, `purge_stale_versions`, `shrink_database` |
| Schema/index writes | `create_path_index`, `drop_path_index` |
| Feature-gated writes | `rotate_encryption_key` |
| Background invalidation | `watcher`, `debounced_watcher` |

On an effective read-only engine these methods return exactly
`LocalFileCacheError::ReadOnly`, even when another argument is missing,
malformed, undecodable, out of grammar, or otherwise invalid. No supplied
factory or eviction callback may run before that error.

Read operations remain available on a validated v5 database. This includes
`get`, `get_if_fresh`, batch reads, status/diagnostic methods, scans, keys,
statistics, export, namespace listing, query `run()`/`dry_run()`, and path-index
listing. A read-only `get` or `get_if_fresh` must not update
`last_accessed_at`; the existing conditional best-effort update remains a
writable-engine optimization, not part of the read result.

### R4 — No watcher authority escalation

Both watcher entry points must run the R3 guard before loading cached paths or
performing OS registration. A rejected construction creates no watcher, helper
engine, callback thread, or database connection and cannot later invalidate a
row.

For a writable originating engine, watcher callbacks still need a dedicated
connection because rusqlite connections are not shared across the notify
thread. The private constructors may create that writable helper only after
the public entry point has established writable authority. They remain private
and must not accept a caller-supplied permission override.

RFC 012 does not redesign the current double-open/registration structure
unless simplification is necessary to carry the authority correctly. It does
not change event classification or swallowed delivery/setup errors; those are
RFC 015 work.

### R5 — Pool and async parity

`ConnectionPool<T>` may be built from read-only options. Its forwarding
methods must expose the engine's same guard and error precedence; in particular
pooled `touch` must return `ReadOnly`. `with` and `with_mut` do not grant access
to private connections or bypass guards on public engine operations.

`ReadPool<T>` continues to force read-only and expose no mutation methods. All
of its slots must pass R1 and R2, so constructing a pool over an old, future,
fresh, or malformed database fails rather than producing partially usable
slots.

Each enabled async runtime wrapper delegates to the corrected synchronous
engine and returns the same `ReadOnly` and schema-validation errors. No runtime
may translate `ReadOnly` into `Database`, panic, or successful no-op behavior.
RFC 012 adds no watcher methods to `AsyncCacheEngine`.

### R6 — CLI database capability matrix

CLI routing must set database authority explicitly per command. It must not
rely on `CacheOptions::default().read_only` or a misleading shared comment.

| Command | Cache database authority |
|---|---|
| `list`, `stats`, `check`, `scan`, `export`, `query`, `inspect`, `namespaces` | Read-only |
| `cleanup`, `vacuum`, `purge-version`, `import`, `copy`, `migrate`, `watch` | Writable or mixed as described below |

For observational commands, the database is opened with `read_only: true`.
They do not create a missing database or migrate an old database. `export` may
create its requested output file, but its cache connection remains read-only.

`copy` opens its source namespace read-only and its destination writable. Its
destination may initialize or migrate the database before copying. Because
both namespaces normally share one database file, destination initialization
must complete before the source reader is opened if schema migration is
needed.

`migrate` remains an explicitly mutating command. Its source may be opened
writable because the documented command can upgrade an old localcache schema
before copying; its destination is writable. This possible source migration
must be documented rather than hidden behind an observational command.
`watch` is writable because its defined behavior invalidates cache rows.

CLI read commands against an old schema return the R2 migration-required
error. They do not silently reopen writable. CLI read commands against a
missing path leave no SQLite file behind.

### R7 — Error and compatibility contract

No new public error variant is added. The contract uses:

- `ReadOnly` for an attempted mutating API on a successfully opened read-only
  engine;
- `UnsupportedFeature` for an existing database whose schema cannot be used
  read-only, including valid schemas requiring initialization/migration and
  future/malformed schemas classified by RFC 010; and
- `Database` when SQLite cannot open the requested file read-only or a later
  database operation fails independently of the policy boundary.

The `ReadOnly` guard has precedence over all method-specific argument errors.
Schema validation occurs during `open`, before an engine exists, so schema
errors necessarily precede method errors.

The error documentation, builder/options documentation, migration guide, CLI
help where relevant, and changelog must describe the corrected behavior:
read-only mode requires a current schema; it never initializes or migrates;
watchers are mutating; and observational CLI commands do not change the cache
database.

### R8 — No hidden lifecycle transition

Implementation acceptance closes B-04 only after its focused tests and review
are accepted. It does not complete M3 by itself; RFC 013 and the combined M3
exit gate remain. RFC 012 stays under `rfcs/accepted/` until the implementation
ships in a release, when it may move to `rfcs/done/` under RFC 000.

No implementation or acceptance step authorizes a release candidate, tag,
push, publication, hosted release, or release action.

## Detailed design

### Open-path structure

`CacheEngine::open` should determine the connection mode once and make the
branches explicit:

```text
explicit read-only + any in-memory mode
    -> UnsupportedFeature, no database initialization

shared in-memory without explicit read-only
    -> existing named shared read-write initialization path

effective file read-only
    -> SQLite read-only open
    -> query_only ON + verified
    -> foreign_keys ON + verified
    -> schema::validate_read_only_current in one Deferred snapshot

writable file or ordinary in-memory
    -> existing RFC 010 initialize/migrate path
    -> existing runtime configuration rules
```

This branching prevents the current `is_memory || !read_only` condition from
initializing an explicitly read-only in-memory engine.

The schema module should share the classifier, not duplicate schema SQL or
maintain a second version policy. The read-only validator is deliberately
small: classify one snapshot and accept only v5.

### Capability propagation

No new public capability type is required. The existing private `read_only`
field and `guard_write` are sufficient when their use is complete. Private
watcher constructors receive only data from a public entry point which has
already passed `guard_write`; they must not reconstruct authority from user
defaults or hard-coded public options before that check.

The method inventory in R3 is the review checklist. Future public mutators
must add the guard as their first product operation and add a read-only
regression assertion in the same change.

### Read behavior and LRU

`get` and `get_if_fresh` are observational in read-only mode even though they
normally perform a best-effort LRU touch. They continue to return payloads but
skip the timestamp update. Explicit `touch` is unambiguously a mutation and
returns `ReadOnly`.

This distinction preserves useful read-only cache consumption without
pretending that an explicit warming request succeeded.

## Test plan

### Schema-open matrix

- Create an exact v5 database with representative file/payload bytes and a
  valid public index. Open it through ordinary read-only, file shared-cache,
  `ReadPool`, `ConnectionPool`, and each enabled async wrapper; prove reads,
  index list, hinted `run()`, and `dry_run()` work.
- Copy the authenticated released physical-version-0/v1 and v4 fixtures and
  construct exact v2 and v3 databases. Read-only open must return
  `UnsupportedFeature` and preserve complete semantic snapshots and fixture
  bytes. No migration or `user_version` change is allowed.
- Cover an existing empty version-0 database, negative and future versions,
  and representative malformed/co-located v5 shapes. Assert the stable error
  category and unchanged schema, rows, payload bytes, version, and indexes.
- Attempt read-only open on a nonexistent file and prove no database, WAL, or
  SHM file is created.
- Assert `query_only == 1` and `foreign_keys == 1` in an internal connection
  test for both ordinary and shared-cache readers.
- Reject explicit `read_only + :memory:` and explicit
  `read_only + shared_cache + :memory:`. Retain a positive regression for the
  existing implicit shared-memory read-write special case.

### Mutation and precedence matrix

- On one representative read-only v5 database, invoke every method in R3 and
  compare a complete database snapshot after each call.
- Supply deliberately invalid paths, suffixes, payload data, keys, or empty
  inputs where applicable and assert exact `ReadOnly` precedence.
- Prove `batch_set` performs no partial preparation/write, `preload` does not
  call its factory, and no eviction callback runs.
- With `encryption`, prove invalid key rotation still returns `ReadOnly` first.
- With `watching`, reject both watcher variants, then change a source file and
  prove the cache row remains. Retain positive writable watcher invalidation
  tests.
- Prove normal reads do not change `last_accessed_at`, while explicit `touch`
  returns `ReadOnly`.

### Wrapper and CLI matrix

- Open a read-only `ConnectionPool` and prove pooled `touch` and representative
  writes return `ReadOnly` unchanged.
- For Tokio, async-std, and smol, prove read success plus `touch` and a
  representative write rejection. The synchronous full matrix remains the
  authority for methods which are thin runtime wrappers.
- Test CLI command routing as a table rather than one assertion per match arm.
- Run representative observational commands against a current database and
  compare schema/data state before and after.
- Run `inspect` and another read command against a missing database and prove
  no file is created; run against a historical fixture copy and prove no
  migration occurs.
- Retain positive tests for each writable or mixed-authority CLI command
  affected by routing changes, especially `copy`, `migrate`, and `watch`.

### Regression gates

- RFC 010 classifier, fixture, migration, rollback, and configuration suites.
- RFC 011 read-only list/hint and identifier-error precedence tests.
- Default and all-feature package tests, focused alternate-runtime tests,
  workspace/all-target tests, warnings-denied clippy, rustdoc, mdBook,
  formatting, and source-integrity checks.
- Focused independent implementation review before B-04 is closed.

## Security considerations

Read-only mode is a least-authority boundary. A caller may use it to inspect an
untrusted or operationally sensitive database while expecting localcache not
to initialize, migrate, vacuum, invalidate, or update access timestamps.

The controls are layered:

1. SQLite read-only open flags prevent storage writes at the connection level.
2. `query_only` is verified as connection-local defence in depth.
3. strict one-snapshot schema classification prevents queries against an
   unknown layout;
4. one public guard provides stable policy and error precedence; and
5. authority inheritance prevents background helpers from upgrading access.

The API guard is not a substitute for SQLite flags. SQLite flags are not a
substitute for the guard: callers depend on the stable typed error, and helper
connections can otherwise escape the original flags.

The design does not claim filesystem immutability against SQLite itself,
another process, backup software, or a separately authorized writer. Tests
must control those actors and claim only localcache-originated mutation.

## Compatibility and rollout

The public Rust signatures and error enum remain stable. Corrected behaviors
are intentionally stricter:

- read-only open rejects old, empty, future, and malformed schemas instead of
  returning a partially compatible engine;
- explicit read-only in-memory configurations are rejected instead of being
  initialized through a writable connection;
- `touch` returns `ReadOnly` at the API boundary;
- watcher construction from a read-only engine returns `ReadOnly`; and
- observational CLI commands no longer create or migrate databases.

Writable open and migration retain RFC 010 behavior. Existing current-v5
read-only users continue to read normally. Shared in-memory behavior without
an explicit read-only request remains unchanged.

The correction belongs in v0.20.1 release notes and the migration/builder/API
documentation. No schema or payload migration is introduced by RFC 012.

Implementation should be one coherent M3 slice. A separate implementation
handoff is unnecessary unless work is delegated; this RFC's method and test
matrices are the execution boundary. One design review and one implementation
acceptance gate are sufficient absent material corrective findings.

## Alternatives considered

### Rely only on SQLite read-only flags

This prevents most writes but leaves inconsistent error variants, unknown
schema acceptance, watcher privilege escalation, and unclear method
precedence. Rejected.

### Allow read-only access to every historical schema

That would require a parallel compatibility query layer for v1-v4 and would
duplicate migration knowledge throughout repository reads. It also makes
results depend on historical column defaults and timestamp units. Rejected;
use the existing atomic writable migration before read-only consumption.

### Let read-only watchers emit events without invalidating rows

That creates a second watcher semantic in which the type name and existing
documentation promise invalidation but only notifications occur. It also does
not address helper authority. Rejected for this patch; a future explicitly
observational watcher API would require separate design.

### Make all reads fail because LRU timestamps cannot update

LRU touching is best-effort metadata, not the primary result of `get`.
Read-only engines already skip it. Rejecting payload reads would make the mode
useless and contradict `ReadPool`. Rejected.

### Introduce `ReadOnlyCacheEngine<T>`

A capability-specific type could eliminate runtime write methods, but it is a
larger public-API change and duplicates the established `ReadPool` direction.
The corrective boundary can be made complete with existing types. Deferred to
a future major API design if demand appears.

### Add a CLI `--read-only` flag and leave commands writable by default

Safety of an observational command should not depend on the user remembering
an extra flag. Command semantics already determine whether mutation is
required. Rejected.

## References

- [RFC 004 — Read-only Shared-memory DB Mode](../done/004-shared-memory-db.md)
- [RFC 007 — Read-only Connection Pool](../done/007-read-only-connection-pool.md)
- [RFC 010 — Transactional, Payload-Preserving Schema Migrations](../accepted/010-transactional-payload-preserving-schema-migrations.md)
- [RFC 011 — Safe SQLite Identifier Boundary](../accepted/011-safe-sqlite-identifier-boundary.md)
- SQLite [`sqlite3_open_v2`](https://www.sqlite.org/c3ref/open.html)
- SQLite [`PRAGMA query_only`](https://www.sqlite.org/pragma.html#pragma_query_only)
- SQLite [read-only databases](https://www.sqlite.org/uri.html)
- Originating architecture reviews:
  `.git-exclude/reviewed/architect-preparation-review-2026-07-17.md` and
  `.git-exclude/reviewed/architect-preparation-review-2026-07-18.md`

## Design review and acceptance gates

RFC 012 is Proposed and awaits one independent design review. If that review
accepts the design and the owner explicitly approves it, move this file to
`rfcs/accepted/` and update the RFC index in the same transition before
implementation begins.

B-04 closes only after the implementation matrix and focused independent
implementation review are accepted. M3 remains incomplete until RFC 013 and
the combined mutation/input-safety exit gate are also accepted. Neither design
nor implementation acceptance authorizes release work.
