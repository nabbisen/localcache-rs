# RFC 010 — Transactional, Payload-Preserving Schema Migrations

| Field | Value |
|---|---|
| Status | Proposed |
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
- **Fresh database**: no localcache application objects exist in
  `sqlite_schema`; SQLite-internal objects do not count.
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
- Physical version `0` with no localcache application objects is fresh.
- Physical version `0` matching the released v0.1 shape is effective version
  `1` and must run the full v1-to-v5 chain.
- A non-empty physical-version-0 database that does not match the released v0.1
  shape must return an error without mutation.
- Values outside the supported non-negative range must be rejected rather than
  wrapped by an integer cast.
- A version claiming `1` through `5` but missing its required structural
  contract must fail closed before migration.

Classification must use bound/static introspection queries (`sqlite_schema`,
`PRAGMA table_info`, `PRAGMA foreign_key_list`, and equivalent APIs). Schema
text or object names read from the database must never be interpolated into
executable SQL.

For the ambiguous version-0 case, unexpected application tables, views,
triggers, or indexes make the shape unrecognized. This deliberately refuses to
guess how to rewrite a hand-modified pre-versioned database.

### R2 — One transaction for the complete transition

For starting versions 1 through 4, all required steps through version 5 must
run inside one `rusqlite::Transaction` with `Immediate` behavior. Fresh schema
creation must likewise be atomic.

- `PRAGMA foreign_keys = ON` is enabled and verified before the transaction.
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
- v4-to-v5 converts `mtime` seconds to nanoseconds exactly once.
- File identities, paths, sizes, hashes, update times, payload BLOBs, and
  file-to-payload relationships remain unchanged except for the fields each
  step explicitly owns.
- Reopening a successfully migrated v5 database is idempotent and performs no
  migration writes.

### R5 — Validate integrity and final shape before commit

The migration unit must fail before commit unless:

- `PRAGMA foreign_key_check` returns no rows;
- the final required tables, columns, defaults, foreign key, and indexes match
  schema version 5;
- no shadow migration objects remain;
- `user_version` is exactly `5`;
- v1 shadow-copy equivalence checks passed when applicable; and
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

After any injected or organic failure:

- the original `user_version`, tables, indexes, columns, row values, payload
  bytes, foreign-key relationships, and sequence high-water mark are unchanged
  semantically;
- reopening with the fault removed can perform the migration successfully; and
- no shadow objects remain.

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
- the generation command/source and relevant locked versions;
- the fixture SHA-256;
- expected schema objects, physical version, row counts, IDs, and decoded
  values; and
- the rule that the fixture is immutable and never routinely regenerated.

Fixture generation must occur in an isolated historical checkout. It must not
replace the current worktree, and the committed fixture must contain no WAL or
SHM sidecars, local paths identifying a developer, credentials, or private
data.

A small synthetic physical-version-1 fixture/test is also required so both the
released unversioned representation and the explicitly versioned compatibility
path remain covered. Synthetic edge cases, rather than the historical fixture,
must cover a file without a payload and an AUTOINCREMENT high-water mark above
the largest live ID when those states cannot be produced through the 0.1.0
public API.

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
9. Opening current v5 twice is idempotent.
10. Unsupported future versions and malformed/unrecognized version-0 shapes
    fail without mutation.
11. Orphan payloads and conflicting shadow objects fail without mutation.
12. A test-only failpoint after each migration stage and immediately before
    commit proves full rollback and successful retry.

Fault injection must be a private `cfg(test)` mechanism or an equivalent test-
only boundary. No environment-variable or public production bypass may alter
migration behavior.

Tests compare semantic database snapshots rather than whole SQLite file bytes;
journal headers and page layout may legitimately change even when a transaction
rolls back semantically.

### R9 — Documentation and release record

Update `docs/src/migration.md` to distinguish:

- payload wire-format compatibility;
- atomic database-schema migration;
- the historical unversioned v0.1 classification; and
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
  -> enable and verify foreign_keys
  -> preliminary physical-version read without lossy cast
  -> reject an unsupported future version
  -> candidate 0..4: BEGIN IMMEDIATE through rusqlite Transaction
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
  -> preliminary v5: validate through a consistent read snapshot; no writes
```

If a version-5 database fails final structural validation, opening fails rather
than attempting to recreate missing objects with `IF NOT EXISTS`. This RFC
does not turn initialization into silent repair.

### Schema-shape validation

Classifiers compare semantic metadata rather than raw `CREATE TABLE` text, so
irrelevant whitespace and SQLite's normalized SQL rendering do not affect the
result. The released v1 contract requires:

| Object | Required shape |
|---|---|
| `files` | `id INTEGER PRIMARY KEY AUTOINCREMENT`; unique, non-null `path`; non-null integer `mtime`, `file_size`, and `updated_at`; nullable text `hash` |
| `payloads` | `file_id INTEGER PRIMARY KEY`; non-null BLOB `content`; foreign key to `files(id)` with `ON DELETE CASCADE` |
| `idx_files_path` | index of `files(path)` |

SQLite-owned autoindexes and `sqlite_sequence` are internal objects. For the
ambiguous physical-version-0 classifier, every non-internal application object
must belong to the released v1 contract. For explicitly versioned schemas,
validation requires the columns, constraints, foreign key, and built-in
indexes owned by that version. A current v5 database may also contain valid
user-created indexes exposed by localcache; those are preserved and are not
mistaken for schema corruption.

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

### Test-only failure injection

Schema helpers expose no public failpoint. Unit-test configuration may pass an
internal callback or enum checked after each completed step and before the
final commit. Returning a synthetic error at each point must cause the
`Transaction` guard to roll back. Each case reopens the database with raw
SQLite, compares its semantic snapshot to the pre-open snapshot, then retries
without the failpoint.

### Fixture handling

Tests copy the committed fixture into a temporary directory before opening it.
They first capture raw rows and payload bytes using `rusqlite`, then open the
copy with current localcache, and finally assert both the raw post-migration
state and decoded public behavior. The committed fixture is never opened in a
mode that can alter it.

## Compatibility

- Public Rust APIs remain source-compatible.
- Schema version stays at 5; fixed code changes how historical versions reach
  it.
- Valid v1-through-v4 databases gain stronger preservation and atomicity.
- Real unversioned v0.1 databases become supported rather than being
  misclassified as fresh.
- Hand-modified or corrupt databases that initialization previously changed
  opportunistically may now receive an error. This is an intentional
  fail-closed correction.
- Payload bytes and application-owned payload versions are not transformed.

## Security and safety considerations

- All migration SQL is static; values are bound and database-derived object
  names are never executed as SQL.
- `BEGIN IMMEDIATE` prevents a concurrent writer from changing the database
  between classification and migration commit.
- Foreign-key validation occurs before destructive table replacement and
  before commit.
- Shadow-copy equivalence makes omission detectable before old data is
  dropped.
- The fixture contains only synthetic data and no developer-identifying path.
- This design does not promise recovery from malicious SQLite files or resource
  exhaustion; SQLite parser failures and I/O errors propagate without a
  destructive fallback.

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

## Implementation sequence after acceptance

1. Add schema classifiers and semantic snapshot helpers in tests.
2. Produce and document the immutable 0.1.0 fixture in an isolated checkout.
3. Refactor initialization around one typed `Immediate` transaction.
4. Implement v1 shadow-table copying and equivalence checks.
5. Move later steps and final `user_version` into the same transaction.
6. Add fixture, preservation, malformed-shape, idempotence, and failpoint tests.
7. Update migration documentation and the coming-release changelog.
8. Run focused migration tests, the complete applicable test matrix, source
   archive smoke, and independent implementation review.

RFC 011 may be implemented as a separate slice in M2. M2 is complete only
after both RFCs pass their focused security/data-integrity review gate.

## Acceptance criteria

RFC 010 may move from Proposed to Accepted only after independent review finds
that:

- the real unversioned v0.1 ambiguity is closed safely;
- the full-chain transaction boundary and rollback semantics are unambiguous;
- payload bytes, relational identity, and AUTOINCREMENT state are preserved;
- fixture provenance cannot be satisfied by a current-code synthetic schema;
- malformed inputs fail without mutation;
- the boundary with RFCs 011 and 012 is clear; and
- owner questions below are resolved.

Implementation is accepted only after the historical fixture, raw-byte and
decoded-value assertions, full-chain fault injection, and focused independent
review all pass on one identified commit.

## Open questions for owner/reviewer

1. **Version-0 policy:** Accept the proposed strict classifier: empty means
   fresh, the exact released v0.1 shape means effective v1, and every other
   non-empty shape fails without mutation? **Proposed: yes.**
2. **Fixture set:** Require both the immutable 0.1.0-generated physical-version-
   0 fixture and a small synthetic physical-version-1 case? **Proposed: yes.**
3. **Atomicity mechanism:** Permit a private `cfg(test)` failpoint to prove
   rollback after each stage, with no production/environment switch?
   **Proposed: yes.**
4. **Automatic backups:** Keep backup creation outside RFC 010 and document
   normal backup practice instead? **Proposed: yes.**

## Review and authorization boundary

While this file remains under `rfcs/proposed/`, it is design material only.
After an independent acceptance recommendation and explicit owner approval,
move it to `rfcs/accepted/`, update its Status and the RFC index in the same
commit, and create the implementation/fixture QA handoff before delegating any
code changes.
