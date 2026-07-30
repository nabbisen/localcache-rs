# RFC 010 — Transactional, Payload-Preserving Schema Migrations

| Field | Value |
|---|---|
| Status | Implemented (0.20.1) |
| Feature | *(core database compatibility; no Cargo feature)* |
| Touches | `src/db/schema.rs`, `src/cache/engine.rs`, migration tests, `tests/fixtures/`, `docs/src/migration.md`, `CHANGELOG.md` |
| Finding | Architect review B-02 |
| Milestone | Phase 21 M2 |

## Summary

Make every schema upgrade from a supported historical database to the current
schema one atomic, fail-closed transaction, and repair the v1-to-v2 step so it
preserves every payload byte and its file relationship.

This RFC also closes an older ambiguity: localcache 0.1.0 created the schema
without setting SQLite's `user_version`, so a real v0.1 database reports `0`,
the same value as a new empty database. Initialization must classify those two
states by inspected schema shape before it mutates anything.

The compatibility claim is proved with an immutable database produced by the
historical `0.1.0` release, raw-byte assertions across the complete v1-to-v5
chain, decoded-value assertions through the current public API, and injected
failures proving rollback after every migration stage.

This RFC does not authorize implementation while Proposed, does not change the
current schema version (`5`), and does not authorize a release.

## Motivation

The current `migrate_v1_to_v2` copies `files`, drops `payloads`, and recreates
`payloads` empty. Opening a version-1 database therefore destroys all cached
values while retaining their file rows. This contradicts the public promise
that databases back to v0.1 migrate transparently and violates RFC 008's
wire-format guarantee.

There is a second atomicity defect. Individual migration functions execute
schema changes and then update `PRAGMA user_version` separately. Some steps
have their own `BEGIN`/`COMMIT`, while others do not. A failure in a later step
can leave an earlier step committed, and a failure between schema mutation and
the version update can leave schema and `user_version` disagreeing.

Finally, the historical `0.1.0` implementation never wrote
`PRAGMA user_version = 1`. The current `version == 0` path assumes a fresh
database and runs `CREATE TABLE IF NOT EXISTS`; against a real v0.1 database,
that can retain the old table shape while labelling it current. A fixture that
sets `user_version = 1` by hand does not represent the released artifact and
cannot prove compatibility with real users' databases.

## Goals

1. Preserve all supported historical file and payload data through migration.
2. Make the complete starting-version-to-v5 transition atomic.
3. Distinguish a truly empty database from the released unversioned v0.1
   schema without guessing destructively.
4. Fail without mutation on corrupt, inconsistent, or unrecognized input.
5. Establish fixture provenance and regression gates that future migrations
   must extend.

## Non-goals

- Changing payload serialization, codec tags, or application-owned
  `payload_version` values.
- Advancing the database schema beyond version 5.
- Repairing arbitrary hand-edited or corrupt SQLite databases.
- Defining read-only open behavior; RFC 012 owns that boundary.
- Defining safe caller-controlled SQLite identifiers; RFC 011 owns that
  boundary.
- Creating automatic backup files or a general migration CLI.
- Treating cache recomputability as permission to discard stored values.

## Terminology

- **Physical version**: the integer returned by `PRAGMA user_version`.
- **Effective version**: the schema generation selected after classifying a
  physical version of `0` as either empty or the released v0.1 shape.
- **Fresh database**: no non-internal application table, index, trigger, or
  view exists in the `main` schema. Objects owned internally by SQLite, such
  as `sqlite_sequence`, do not count. A database containing an unrelated
  application object is not fresh.
- **Historical v1 shape**: the `files`, `payloads`, and `idx_files_path`
  contract emitted by localcache `0.1.0`, with no namespace, payload encoding,
  or later columns. Its physical version is normally `0`.
- **Migration unit**: one SQLite `Immediate` transaction covering the
  authoritative classification, every required schema step, final version
  update, postconditions, and commit.

## Requirements

### R1 — Classify before mutation

Initialization must read `user_version` and inspect schema metadata before any
DDL or DML. For a migration candidate, the authoritative read and
classification occur after acquiring the migration transaction so no writer
can change the schema between classification and mutation.

- Physical versions `1` through `5` map to the corresponding schema generation.
- Physical version `0` with no non-internal application objects in `main` is
  fresh.
- Physical version `0` matching the released v0.1 shape is effective version
  `1` and must run the full v1-to-v5 chain.
- A non-empty physical-version-0 database that does not match the released v0.1
  shape must return an error without mutation.
- Values outside the supported non-negative range must be rejected rather than
  wrapped by an integer cast.
- A version claiming `1` through `5` must match the complete semantic contract
  and allowed-extra-object policy for that version. Missing, changed, or
  disallowed objects fail closed before migration.

Classification must use bound/static introspection queries (`sqlite_schema`,
`PRAGMA table_xinfo`, `PRAGMA table_list`, `PRAGMA index_list`,
`PRAGMA index_xinfo`, `PRAGMA foreign_key_list`, and equivalent APIs). A small
read-only tokenizer/parser of `sqlite_schema.sql` must confirm released DDL
properties that metadata PRAGMAs cannot prove, including `AUTOINCREMENT`,
absence of column `COLLATE` clauses, and ordinary-rowid/non-STRICT table form.
It must never execute parsed text. Schema text or object names read from the
database must never be interpolated into executable SQL.

For the ambiguous version-0 case, unexpected application tables, views,
triggers, or indexes make the shape unrecognized. This deliberately refuses to
guess how to rewrite a hand-modified pre-versioned database.

The normative version contracts and extra-object policy are specified under
[Schema-shape validation](#schema-shape-validation). Validation must inspect
hidden/generated columns as well as ordinary columns. `PRAGMA table_info`
alone is insufficient.

### R2 — One transaction for the complete transition

For starting versions 1 through 4, all required steps through version 5 must
run inside one `rusqlite::Transaction` with `Immediate` behavior. Fresh schema
creation must likewise be atomic.

- Caller-requested `journal_mode` and `synchronous` settings must not be
  applied before classification or migration. The current pre-initialization
  ordering in `CacheEngine::open` must change.
- `PRAGMA foreign_keys = ON` is enabled and verified before the transaction.
- For a file-backed migration, the connection must report an existing
  rollback-capable journal mode (`delete`, `truncate`, or `persist`) or `wal`.
  Existing `memory` or `off` mode is rejected without schema/data/version
  mutation. The migration sets connection-local `synchronous = FULL` and
  verifies it before beginning, independently of caller-requested MEMORY/OFF
  runtime settings. An in-memory fresh database receives transactional error
  atomicity but makes no process-crash durability claim.
- A preliminary version read may select the fast path, but a candidate version
  `0` through `4` is re-read and classified authoritatively inside the
  `Immediate` transaction. A concurrent migration observed there is handled
  according to its new version rather than using stale classification.
- Per-version helpers accept the transaction and contain no transaction-control
  statements.
- Per-version helpers do not update `user_version`.
- `user_version` becomes `5` once, near the end of the migration unit.
- Postconditions run before commit.
- Any returned error or panic-unwind before commit drops the transaction and
  rolls back the entire unit.
- No production path catches an error and commits partial work.

`CacheEngine::open` may make its newly created connection mutable so
`Connection::transaction_with_behavior` provides compile-time nested-
transaction exclusion. `unchecked_transaction` is not preferred for this
boundary.

Only after successful migration commit, or successful read-snapshot validation
of an existing v5 database, may `CacheEngine::open` apply caller runtime
settings. It applies and verifies the connection-local requested
`synchronous` setting first, then applies and verifies the requested
`journal_mode` as the last fallible configuration operation. If configuration
fails after migration committed, the returned error must explicitly state
that schema migration committed successfully and report the requested and
observed configuration; it must not claim the database is unchanged. If no
migration occurred, the error must still report any observed persistent
journal-mode change.

### R3 — Preserve the v1 relational data exactly

The v1-to-v2 step must use shadow parent and child tables so old and new data
coexist until equivalence is established.

Before dropping the old tables, the migration must establish all of the
following inside the same transaction:

- every `files.id` is preserved;
- every file metadata value (`path`, `mtime`, `file_size`, nullable `hash`, and
  `updated_at`) is preserved;
- every migrated file receives namespace `default`;
- every `(file_id, content)` payload row is present in the shadow table;
- every `content` BLOB is byte-for-byte equal, including empty BLOBs and bytes
  that are not decodable as the payload type used by the current caller;
- files without payload rows remain files without payload rows;
- payload foreign-key relationships remain valid; and
- the `files` AUTOINCREMENT high-water mark does not decrease.

AUTOINCREMENT classification requires both the read-only DDL parse described
in R1 and valid `sqlite_sequence` state. For `files`:

- no sequence row is accepted only when the table has never been written and
  has no live rows;
- otherwise exactly one row must exist and `typeof(seq)` must be `integer`;
- duplicate, NULL, REAL, TEXT, BLOB, or negative sequence values are rejected;
- the effective high-water is `max(valid sequence, max live positive id)`;
- the shadow relation receives that effective value without adding one; and
- `i64::MAX` is preserved exactly, so a later insert may correctly return
  SQLite's exhaustion error rather than wrap or reuse an ID.

The implementation must compare old and shadow relations in both directions,
using row counts plus set-difference/equivalence queries that include BLOB
content. It must not load an unbounded cache into Rust memory merely to prove
equivalence.

Only after those checks pass may it drop the old child table, drop the old
parent table, rename the shadow tables, recreate the released indexes, and
continue to v3. Shadow object names are internal constants, and pre-existing
objects using those names cause a fail-closed error.

### R4 — Preserve data across every later step

The v2-to-v3, v3-to-v4, and v4-to-v5 steps remain schema transformations, not
opportunities to rewrite payload bytes.

- v2-to-v3 adds `files.payload_version = 0` and
  `payloads.encoding = 'raw'` to historical rows.
- v3-to-v4 adds `files.last_accessed_at = 0` and the LRU index.
- v4-to-v5 converts `mtime` seconds to nanoseconds exactly once, only after
  validating the complete numeric input domain below.
- File identities, paths, sizes, hashes, update times, payload BLOBs, and
  file-to-payload relationships remain unchanged except for the fields each
  step explicitly owns.
- Reopening a successfully migrated v5 database is idempotent and performs no
  migration writes.

Before v4 mutation, every `files.mtime` must have SQLite storage class
`integer` and lie in the inclusive seconds range
`[-9_223_372_036, 9_223_372_036]`. Those are the exact signed `i64` values
whose multiplication by `1_000_000_000` cannot overflow. A NULL, REAL, TEXT,
BLOB, just-outside value, or normal-scale already-nanosecond value is rejected
before mutation; the migration does not coerce, clamp, guess, or repair it.

The transaction records `(id, old_mtime)` in an internal temporary/shadow
relation before the update. After conversion it must prove for every row that
`typeof(mtime) = 'integer'` and
`new_mtime = old_mtime * 1_000_000_000`, with bidirectional ID/count
equivalence. This prevents SQLite from silently promoting overflow arithmetic
to REAL.

An already-converted v4 value within the safe-seconds range is mathematically
indistinguishable from a legitimate imported seconds value. RFC 010 therefore
uses the exact storage-class/range rule rather than an undocumented timestamp
plausibility heuristic. Realistic nanosecond epoch values and the known
partial-migration failure mode fall outside the range and are rejected.

### R5 — Validate integrity and final shape before commit

The migration unit must fail before commit unless:

- `PRAGMA foreign_key_check` returns no rows;
- the final required tables, columns, defaults, foreign key, and indexes match
  schema version 5;
- no shadow migration objects remain;
- `user_version` is exactly `5`;
- v1 shadow-copy equivalence checks passed when applicable; and
- exact v4 timestamp conversion checks passed when applicable;
- every allowed `lc_user_*` index present at migration start remains present,
  structurally identical, and usable; and
- row-count and payload-byte invariants captured for the starting database
  still hold.

The pre-migration path must also reject orphaned payload rows or other foreign-
key violations. RFC 010 preserves valid historical data; it does not silently
delete or invent rows to repair an inconsistent database.

### R6 — Failure semantics

Migration errors must propagate from `CacheEngine::open` as a normal
`LocalFileCacheError`; no panic, process exit, retry loop, or destructive
fallback is permitted.

For an unrecognized schema shape, the error must identify the physical version
and state that the database was not modified. SQLite errors may remain wrapped
by `LocalFileCacheError::Database`, but tests must not depend on unstable full
SQLite error prose.

For this claim, “not modified” means the rejected open changed no localcache
schema, row data, `user_version`, or persistent journal mode. Normal SQLite
recovery performed while opening the connection is outside the claim and must
be documented separately. Connection-local PRAGMA state disappears with the
failed connection.

After any injected or organic failure:

- the original `user_version`, tables, indexes, columns, row values, payload
  bytes, foreign-key relationships, and sequence high-water mark are unchanged
  semantically;
- reopening with the fault removed can perform the migration successfully; and
- no shadow objects remain.

`SQLITE_BUSY` from acquiring the `Immediate` transaction is returned without
an internal retry loop and without partial migration work. Documentation may
tell callers to retry the entire open after contention clears.

Post-commit runtime-configuration failure is a distinct case: migration cannot
be rolled back after commit. The error contract must distinguish
`schema_migration_committed = true` from a pre-commit migration failure and
report requested/observed PRAGMA state. It must never tell a caller to assume
the old schema remains.

To avoid adding a source-breaking variant to the currently exhaustive public
error enum in a patch release, this configuration path returns
`LocalFileCacheError::UnsupportedFeature` with the stable prefix
`database runtime configuration failed:` followed by
`schema_migration_committed=<true|false>`, requested values, observed values,
and the SQLite cause when available. Tests assert the stable prefix and fields,
not platform-specific trailing SQLite prose.

### R7 — Historical fixture provenance

Commit `tests/fixtures/compat-v0_1.sqlite3`, generated by the public API of the
repository's annotated historical tag `0.1.0` at
`e44cfedc16cf96e3dfe97ad7ccbc1631b2398775` and its locked dependency set, not
by manually recreating the old schema in current test code.

The fixture must contain synthetic, non-secret data covering:

- at least two file rows with distinct real bincode 1.3 payloads;
- a nullable hash case;
- a public-API-created ID gap or nontrivial AUTOINCREMENT state where the
  historical API permits it; and
- the exact physical `user_version` emitted by 0.1.0 (`0`).

`tests/fixtures/README.md` must record:

- the source tag and commit;
- the committed generation source and exact command, Rust/Cargo host and
  target, SQLite runtime version, and relevant locked crate versions;
- the fixture SHA-256;
- expected schema objects, physical version, row counts, IDs, and decoded
  values; and
- the rule that the fixture is immutable and never routinely regenerated.

Fixture generation must occur in an isolated historical checkout. It must not
replace the current worktree, and the committed fixture must contain no WAL or
SHM sidecars, local paths identifying a developer, credentials, or private
data.

A focused test or fixture gate must compute the committed fixture SHA-256 and
compare it to the recorded value before copying/opening the fixture. Fixture
immutability is executable, not documentation-only.

A small synthetic physical-version-1 fixture/test is also required so both the
released unversioned representation and the explicitly versioned compatibility
path remain covered. Synthetic edge cases, rather than the historical fixture,
must cover a file without a payload and an AUTOINCREMENT high-water mark above
the largest live ID when those states cannot be produced through the 0.1.0
public API.

The valid-user-index v4 fixture must be produced through the `0.19.0` public
`create_path_index` API (or retain equivalently strong exact-tag provenance),
then copied before current-code migration. A current-code hand-built lookalike
does not by itself prove compatibility with the released API.

### R8 — Acceptance tests

The implementation must add tests for:

1. An empty physical-version-0 database creates schema v5 atomically.
2. The immutable v0.1 fixture is classified as effective v1.
3. The fixture migrates through the complete chain to v5.
4. Raw payload BLOBs before and after are byte-identical.
5. Payloads decode to the documented values through the current public API.
6. File metadata, IDs, missing-payload rows, namespace/defaults, foreign keys,
   indexes, and AUTOINCREMENT high-water behavior are correct after migration.
7. A synthetic physical-version-1 database follows the same preserving path.
8. Existing v2, v3, and v4 fixtures/constructors migrate to v5 while preserving
   representative payload bytes and decoded values.
9. A released-shape v4 fixture containing an exact public-API-created
   `lc_user_*` path index migrates successfully; the index survives with the
   same definition and remains usable.
10. Opening current v5 twice is idempotent. The validator passes on a
    `query_only` connection/read snapshot, `total_changes` remains zero, and
    schema/data/version semantic snapshots are identical.
11. Unsupported future versions and malformed/unrecognized version-0 shapes
    fail without mutation, including a version-0 database containing only an
    unrelated application table/index/view/trigger.
12. Schema near-matches fail unchanged: reordered/extra/hidden/generated
    columns, wrong affinity/nullability/default/PK/UNIQUE/FK/collation,
    WITHOUT ROWID or STRICT tables, changed built-in indexes, unexpected
    triggers, lookalike `lc_user_*` indexes, invalid `sqlite_sequence`, orphan
    payloads, and conflicting shadow names.
13. v4 timestamp conversion accepts both inclusive bounds and representative
    ordinary seconds. It rejects both just-outside bounds and REAL/TEXT/BLOB
    storage, and a realistic already-nanosecond/partially migrated value; every
    rejection preserves the complete semantic snapshot and version.
14. Every public `JournalMode`/`SynchronousMode` combination migrates using the
    migration-safe configuration before applying requested runtime settings.
    Unrecognized inputs requested with WAL, MEMORY, and synchronous OFF retain
    schema/data/version and persistent journal mode. Existing unsafe migration
    modes, injected rollback, and post-commit configuration failure follow R2
    and R6 exactly.
15. A representative larger synthetic migration preserves equivalence while
    permitting temporary-storage/resource exhaustion to surface as an
    ordinary rollback error; this is not a performance promise.
16. The mandatory private failpoints below prove full rollback and successful
    retry.
17. Deterministic two-connection tests cover `SQLITE_BUSY`, stale preliminary
    state superseded by the authoritative in-transaction re-read, and the
    already-v5 no-write validation path.

Fault injection must be a private `cfg(test)` mechanism or an equivalent test-
only boundary. No environment-variable or public production bypass may alter
migration behavior.

The minimum failpoint enum/list is normative. It must permit a returned
synthetic error after:

1. authoritative classification and preconditions;
2. shadow `files` creation;
3. shadow `payloads` creation;
4. parent copy;
5. child/payload copy;
6. bidirectional equivalence completion;
7. old `payloads` drop;
8. old `files` drop;
9. new `files` rename;
10. new `payloads` rename;
11. sequence restoration;
12. v2 index creation and v2-shape validation;
13. v2-to-v3;
14. v3-to-v4;
15. v4 numeric snapshot/preconditions;
16. v4-to-v5 conversion and exact equivalence;
17. final `user_version` write;
18. final shape/foreign-key/postcondition validation; and
19. immediately before commit.

In addition, one `catch_unwind` test must panic at a destructive v1 point
(after old `files` is dropped) and prove semantic rollback. This proves the
normal panic-unwind profile. A `panic = abort` process termination relies on
SQLite journal recovery and is not claimed as deterministic in-process test
evidence.

The stale-read concurrency test uses a private test barrier after preliminary
version read: a second connection completes migration, then the first resumes,
acquires its transaction, observes v5 authoritatively, validates, and performs
no migration writes. The busy test holds a competing write transaction while
open attempts `Immediate`; it must receive `SQLITE_BUSY` with unchanged
semantic state.

Tests compare semantic database snapshots rather than whole SQLite file bytes;
journal headers and page layout may legitimately change even when a transaction
rolls back semantically.

### R9 — Documentation and release record

Update `docs/src/migration.md` to distinguish:

- payload wire-format compatibility;
- atomic database-schema migration;
- the historical unversioned v0.1 classification; and
- strict rejection of co-located/unrecognized schema and invalid historical
  numeric/object state;
- migration-safe journal/synchronous settings versus post-commit caller
  runtime settings;
- `SQLITE_BUSY` whole-open retry guidance and post-commit configuration-error
  semantics; and
- the recommendation to keep normal application backups even though migration
  is designed to preserve data.

Update `CHANGELOG.md` under the already-prepared coming-release section. Do not
mark the version released, move RFC 010 to `done/`, tag, publish, or claim M2
complete until implementation review and the remaining M2 RFC 011 exit work
are accepted.

### R10 — Future migration discipline

Every future schema version must extend the same migration unit and add a
fixture or constructor for the immediately preceding version. A future step
must state its owned transformations, preserved fields, rollback point, and
postconditions. `user_version` is never advanced outside the transaction that
establishes and validates the corresponding schema.

## Detailed design

### Initialization flow

The intended control flow is:

```text
open writable connection
  -> do not apply caller journal/synchronous options yet
  -> enable and verify foreign_keys
  -> preliminary physical-version read without lossy cast
  -> reject an unsupported future version
  -> preliminary v5: validate through a consistent read snapshot; no writes
  -> candidate 0..4: BEGIN IMMEDIATE through rusqlite Transaction
       (file-backed: existing WAL/rollback journal + synchronous FULL)
       -> re-read version and classify schema before mutation
       -> reject unsupported/inconsistent input
       -> fresh: create v5 schema
       -> v1: v1->v2->v3->v4->v5
       -> v2:     v2->v3->v4->v5
       -> v3:             v3->v4->v5
       -> v4:                     v4->v5
       -> concurrently-observed v5: validate, then perform no writes
       -> migrated/fresh branch: set user_version = 5
       -> validate final shape, invariants, and foreign keys
     COMMIT
  -> apply and verify requested synchronous setting
  -> apply and verify requested journal mode as final fallible configuration
  -> return engine
```

If a version-5 database fails final structural validation, opening fails rather
than attempting to recreate missing objects with `IF NOT EXISTS`. This RFC
does not turn initialization into silent repair.

### Connection configuration and durability

Classification occurs before caller runtime PRAGMAs so an unrecognized
database cannot be switched to WAL or otherwise persistently reconfigured by
the rejected open. Merely opening SQLite may perform normal journal/WAL
recovery; that SQLite-owned recovery is not a localcache migration mutation.

For a recognized file-backed migration, the connection records its observed
journal and synchronous state. The migration proceeds only with existing WAL
or a disk-backed rollback journal and verified connection-local
`synchronous = FULL`. Caller-requested `Memory` and `Off` settings are delayed
until after commit. If the existing database somehow opens in `memory` or
`off` journal mode, migration fails without changing it rather than silently
normalizing an unsafe mode outside the transaction.

After commit (or no-write v5 validation), requested synchronous mode is applied
and read back first. Requested journal mode is then applied and read back as
the final fallible operation. A mismatch returns a dedicated configuration
error carrying whether schema migration committed plus requested and observed
settings. The error documentation states that a successful schema commit is
durable even though engine construction failed, and that observed persistent
journal configuration—not the pre-open mode—is authoritative after a
post-commit configuration error.

Fresh `:memory:` databases use one transaction for error atomicity but cannot
offer file-journal crash recovery. File-backed automatic backups remain out of
scope because the migration never adopts caller-requested weak durability
until after successful commit.

### Schema-shape validation

Classifiers compare semantic metadata rather than raw `CREATE TABLE` text, so
irrelevant whitespace and SQLite's normalized SQL rendering do not affect the
result. Column order is normative, not merely membership.

All released `files` and `payloads` tables are ordinary rowid tables: not
virtual, not STRICT, and not WITHOUT ROWID. Every declared column has
`hidden = 0`; extra, hidden, or generated columns are rejected. No column has
an explicit `COLLATE` clause, so text comparison uses BINARY. Declared types
and resulting affinities must match the following table exactly:

| Version | `files` columns in order |
|---|---|
| v1 | `id INTEGER PRIMARY KEY AUTOINCREMENT`; `path TEXT NOT NULL`; `mtime INTEGER NOT NULL`; `file_size INTEGER NOT NULL`; `hash TEXT NULL`; `updated_at INTEGER NOT NULL` |
| v2 | `id`; `namespace TEXT NOT NULL DEFAULT 'default'`; `path`; `mtime`; `file_size`; `hash`; `updated_at` |
| v3 | v2 columns, then `payload_version INTEGER NOT NULL DEFAULT 0` |
| v4/v5 | v3 columns, then `last_accessed_at INTEGER NOT NULL DEFAULT 0` |

Unelided columns inherit the exact v1 declaration above. `id` is the sole
primary-key column. The released table-level uniqueness contracts are
`UNIQUE(path)` for v1 and `UNIQUE(namespace, path)` for v2-v5, using ASC/BINARY
terms, with no expression or partial predicate.

| Version | `payloads` columns in order |
|---|---|
| v1/v2 | `file_id INTEGER PRIMARY KEY`; `content BLOB NOT NULL` |
| v3-v5 | v1/v2 columns, then `encoding TEXT NOT NULL DEFAULT 'raw'` |

`payloads.file_id` is the sole primary key, is not AUTOINCREMENT, and has one
foreign key to `files(id)` with `ON DELETE CASCADE`, `ON UPDATE NO ACTION`, and
no deferred clause. No other PK, UNIQUE, CHECK, or FK constraint is accepted.

The required explicit built-in indexes are:

| Version | Index contract |
|---|---|
| v1 | `idx_files_path` on `files(path ASC BINARY)` |
| v2/v3 | `idx_files_namespace_path` on `files(namespace ASC BINARY, path ASC BINARY)` |
| v4/v5 | the v2/v3 index plus `idx_files_lru` on `files(namespace ASC BINARY, last_accessed_at ASC BINARY, updated_at ASC BINARY)` |

Each built-in index is an ordinary non-unique, non-partial index with no
expressions or included/hidden key terms. The UNIQUE autoindex must have the
matching columns/order/collation and `origin = 'u'`. Validation relies on
semantic metadata for these properties rather than autoindex name text.

`AUTOINCREMENT` is established non-mutatingly by a limited read-only parse of
the `files` `CREATE TABLE` statement plus the `sqlite_sequence` validation in
R3. `table_info`/`table_xinfo` primary-key metadata or mere presence of a
sequence row is not accepted as proof, because neither independently proves
the released AUTOINCREMENT clause.

The allowed extra-object policy is:

- physical version 0/effective v1 and explicit versions 1-3: no non-internal
  objects beyond the exact built-in contract;
- physical versions 4 and 5: the exact built-in contract plus zero or more
  released-public-API path indexes whose names begin `lc_user_` and whose
  structure is exactly a non-unique, non-partial ordinary index on
  `files(namespace ASC BINARY, path ASC BINARY)`, with no expressions or
  predicate; and
- every version: reject unrelated application tables, indexes, or views and
  reject every unexpected trigger before any migration DML.

A prefix alone never makes an index valid. A lookalike `lc_user_*` index with
a different table, term, order, collation, uniqueness, expression, or partial
predicate is rejected. Structurally valid v4 user indexes are recorded before
migration and must survive v4-to-v5 byte-for-byte in definition and remain
usable. Names are treated only as bound metadata; RFC 011 owns the grammar for
creating new identifiers.

SQLite-owned autoindexes and `sqlite_sequence` are internal objects, but their
semantic contents are validated where specified. This strict policy means a
database co-locating unrelated application schema with localcache is rejected
rather than partially rewritten under localcache's database-global
`user_version`. That fail-closed compatibility tradeoff is intentional and
must be documented.

A truly fresh database is created with non-conditional DDL after the empty
classification. `IF NOT EXISTS` must not mask a conflicting object.

### v1 shadow-table sequence

The implementation may vary names, but the required ordering is:

1. Snapshot counts, relational invariants, and `sqlite_sequence` high-water.
2. Create a v2-shaped shadow `files` table.
3. Create a shadow `payloads` table referencing the shadow parent.
4. Copy file rows with identical IDs and metadata plus namespace `default`.
5. Copy payload rows with identical `file_id` and `content` BLOB.
6. Compare old and shadow relations in both directions and run foreign-key
   checks that cover the shadow relation.
7. Remove the old child before the old parent.
8. Rename the shadow parent and child, preserving the resulting FK target.
9. Restore a non-decreasing AUTOINCREMENT high-water mark.
10. Create the v2 index and validate the v2 shape.
11. Continue later steps inside the same outer transaction.

No payload decoding occurs during the migration. Serialization compatibility
is tested separately so migration can preserve even caller-specific or
currently unavailable payload types byte-for-byte.

### v4 numeric conversion sequence

Before `UPDATE`, the migration validates `typeof(mtime)` and the inclusive
safe-seconds bounds for every row, then copies `(id, mtime)` into an internal
transactional snapshot relation. Any invalid row aborts before the first
timestamp mutation.

The update uses the fixed integer multiplier. Post-update queries reject any
non-integer storage class, missing/extra ID, count difference, or value not
exactly equal to its snapshotted value times `1_000_000_000`. The snapshot is
removed before final postconditions. A realistic v4 database left with
nanoseconds but version 4 by the old update/version split is rejected unchanged
rather than multiplied twice.

### Test-only failure injection

Schema helpers expose no public failpoint. Unit-test configuration may pass an
internal callback or the normative enum from R8. Returning a synthetic error at
every mandatory point must cause the `Transaction` guard to roll back. Each
case reopens the database with raw SQLite, compares its semantic snapshot to
the pre-open snapshot, then retries without the failpoint. The destructive
panic-unwind case and deterministic concurrency barriers are also required by
R8; a four-point “one per version helper” implementation is insufficient.

### Fixture handling

Tests copy the committed fixture into a temporary directory before opening it.
They first capture raw rows and payload bytes using `rusqlite`, then open the
copy with current localcache, and finally assert both the raw post-migration
state and decoded public behavior. The committed fixture is never opened in a
mode that can alter it.

## Compatibility

- Public method signatures and the `LocalFileCacheError` variant set remain
  source-compatible. Post-commit configuration failure uses an existing error
  variant with stable, explicit context including
  `schema_migration_committed=true|false`, requested settings, and observed
  settings; no new public enum variant is required by this RFC.
- Schema version stays at 5; fixed code changes how historical versions reach
  it.
- Valid v1-through-v4 databases gain stronger preservation and atomicity.
- Released v4 `lc_user_*` path indexes are recognized and preserved.
- Real unversioned v0.1 databases become supported rather than being
  misclassified as fresh.
- Hand-modified or corrupt databases that initialization previously changed
  opportunistically may now receive an error. This is an intentional
  fail-closed correction.
- Databases containing unrelated co-located application schema are rejected;
  localcache does not claim safe ownership of their database-global
  `user_version`.
- Structurally valid v4 databases with unsafe/non-integer `mtime` values are
  rejected unchanged rather than silently storing REAL or multiplying twice.
- Caller-requested journal/synchronous settings take effect after migration or
  v5 validation, not before. An error after commit explicitly reports that the
  schema is already v5.
- Payload bytes and application-owned payload versions are not transformed.

## Security and safety considerations

- All migration SQL is static; values are bound and database-derived object
  names are never executed as SQL.
- The limited schema tokenizer reads DDL only to validate released structure;
  it has no execution path.
- `BEGIN IMMEDIATE` prevents a concurrent writer from changing the database
  between classification and migration commit.
- Caller-requested MEMORY/OFF durability is withheld until after commit;
  migration uses existing WAL/disk rollback journaling with synchronous FULL.
- Foreign-key validation occurs before destructive table replacement and
  before commit.
- Shadow-copy equivalence makes omission detectable before old data is
  dropped.
- Unexpected triggers and numeric storage-class/range violations are rejected
  before migration DML.
- The fixture contains only synthetic data and no developer-identifying path.
- This design does not promise recovery from malicious SQLite files or resource
  exhaustion; SQLite parser failures and I/O errors propagate without a
  destructive fallback.

Normative SQLite behavior used by this design is documented by SQLite in:

- [AUTOINCREMENT](https://www.sqlite.org/autoinc.html);
- [the `sqlite_sequence` file-format contract](https://www.sqlite.org/fileformat.html#the_sqlite_sequence_table);
- [WAL activation and persistence](https://www.sqlite.org/wal.html#activating_and_configuring_wal_mode);
- [`PRAGMA journal_mode`](https://www.sqlite.org/pragma.html#pragma_journal_mode);
  and
- [expression arithmetic and numeric promotion](https://www.sqlite.org/lang_expr.html).

## Alternatives considered

### Copy payloads out to a Rust `Vec` and insert them later

Rejected. It scales memory with cache size and creates an avoidable
out-of-memory failure mode. SQL shadow tables permit bounded-memory copying and
equivalence checks in the same transaction.

### Keep the current per-step transactions

Rejected. They cannot roll back an earlier committed step when a later step
fails and can leave `user_version` out of sync with schema changes.

### Treat every physical version 0 database as fresh

Rejected. That is incompatible with the actual 0.1.0 release, which emitted a
non-empty schema while leaving `user_version` at 0.

### Treat every non-empty version 0 database as v1

Rejected. It would run destructive migration SQL against unrelated or
hand-modified schemas. Only the reviewed historical shape is recognized.

### Recompute or discard old payloads because this is a cache

Rejected. Payload computation can be expensive or impossible to reproduce,
and the project already promises transparent migration and stable payload
bytes.

### Create an automatic backup before migration

Rejected for this RFC. Backup naming, storage exhaustion, retention, and
atomic replacement introduce a separate operational contract. Transactional
rollback and normal application backups remain the defined boundary.

### Apply caller journal/synchronous settings before migration

Rejected. WAL can persistently change the database before classification, and
MEMORY/OFF weakens the crash/rollback basis of the migration. Runtime settings
are applied only after a safe migration commit or no-write v5 validation.

## Implementation sequence after acceptance

1. Add exact versioned schema/object classifiers, safe DDL token validation,
   and semantic snapshot helpers in tests.
2. Produce and document the immutable 0.1.0 fixture in an isolated checkout.
3. Refactor connection PRAGMA sequencing and initialization around one typed
   `Immediate` migration transaction.
4. Implement v1 shadow-table copying and equivalence checks.
5. Add checked v4 numeric conversion; move later steps and final
   `user_version` into the same transaction.
6. Add fixture, user-index, PRAGMA, preservation, malformed-shape, numeric,
   idempotence, failpoint, panic-unwind, and concurrency tests.
7. Update migration documentation and the coming-release changelog.
8. Run focused migration tests, the complete applicable test matrix, source
   archive smoke, and independent implementation review.

RFC 011 may be implemented as a separate slice in M2. M2 is complete only
after both RFCs pass their focused security/data-integrity review gate.

## Design review history

The first independent design review is recorded at
`.git-exclude/reviewed/architect-rfc-010-design-review-2026-07-21.md`. Its
verdict was **Accept with changes; remain Proposed**. This revision addresses:

- F-01 with a single fresh definition, complete versioned schema contracts,
  DDL/AUTOINCREMENT validation, per-version extra-object policy, and released
  v4 user-index preservation;
- F-02 with pre-classification PRAGMA prohibition, migration-safe durability,
  delayed caller configuration, and explicit post-commit failure semantics;
- F-03 with exact INTEGER storage/range preconditions and post-conversion
  equivalence for v4 `mtime`; and
- F-04 with nineteen mandatory error failpoints, destructive panic-unwind
  proof, deterministic busy/stale-read concurrency tests, and a no-write v5
  validation gate.

The reviewer found the four proposed owner decisions reasonable after these
edits. The owner approved all four on 2026-07-21; they are recorded below.

The focused re-review at
`.git-exclude/reviewed/architect-rfc-010-focused-rereview-2026-07-21.md`
returned **Accept** with no blocking findings. The owner authorized the
repository-visible Accepted transition on 2026-07-21. The RFC moved to
`rfcs/accepted/` with its Status, index, and inbound roadmap link updated
together. This accepts the design; it does not claim implementation evidence,
M2 completion, or release authorization.

## Acceptance criteria

RFC 010 may move from Proposed to Accepted only after independent review finds
that:

- the real unversioned v0.1 ambiguity is closed safely;
- the full-chain transaction boundary and rollback semantics are unambiguous;
- payload bytes, relational identity, and AUTOINCREMENT state are preserved;
- fixture provenance cannot be satisfied by a current-code synthetic schema;
- malformed inputs fail without mutation;
- journal/synchronous sequencing cannot weaken or mutate a rejected migration;
- the full released v4 timestamp domain is checked before multiplication;
- allowed extra objects and every mandatory rollback/concurrency proof point
  are normative;
- the boundary with RFCs 011 and 012 is clear; and
- owner questions below are resolved.

Implementation is accepted only after the historical fixture, raw-byte and
decoded-value assertions, full-chain fault injection, and focused independent
review all pass on one identified commit.

## Owner decisions

The owner approved all four decisions on 2026-07-21. This approval resolves
the design questions for focused re-review. At that point it did not by itself
move the RFC to Accepted or authorize implementation; the later focused
acceptance and owner-authorized transition are recorded above.

1. **Version-0 policy:** An empty database is fresh, the exact released v0.1
   shape is effective v1, and every other non-empty version-0 shape fails
   without mutation. **Decision: approved.**
2. **Fixture set:** Require both the immutable 0.1.0-generated physical-version-
   0 fixture and a small synthetic physical-version-1 case.
   **Decision: approved.**
3. **Atomicity mechanism:** Permit private `cfg(test)` failpoints at every
   normative proof point, with no production or environment-controlled
   switch. **Decision: approved.**
4. **Automatic backups:** Keep backup creation outside RFC 010 and document
   normal backup practice instead. **Decision: approved.**

## Acceptance and implementation boundary

RFC 010 is Accepted for implementation following independent focused review
and explicit owner authorization on 2026-07-21. Implementation must not be
delegated until this transition is committed and the implementation/fixture QA
handoff is created with references to this RFC and both architecture reviews.

Companion implementation documents:

- [implementation handoff](../handoffs/010-transactional-payload-preserving-schema-migrations/implementation-handoff.md);
- [acceptance and QA checklist](../handoffs/010-transactional-payload-preserving-schema-migrations/acceptance-qa-checklist.md).

Acceptance does not supply fixture, migration, test, M2, or release evidence.
The RFC moves to `done/` only after the implementation ships under the
project's lifecycle policy.
