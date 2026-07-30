# RFC 010 Implementation Acceptance and QA Checklist

This checklist operationalizes the accepted
[RFC 010](../../done/010-transactional-payload-preserving-schema-migrations.md).
The RFC remains authoritative. The companion
[implementation handoff](./implementation-handoff.md) sequences the work but
does not override it.

At handoff creation no implementation checkbox is claimed complete. Record the
exact implementation commit and observed evidence when checking an item.

## Authority and preconditions

- [ ] RFC 010 remains under `rfcs/accepted/` with Status Accepted.
- [ ] Accepted transition commit
      `edbe4fbfd58b8071b33ee7b9a85b0549d1a7518c` is an ancestor of the
      implementation target.
- [ ] The implementation handoff and this QA checklist are committed before
      development is delegated.
- [ ] The implementation review package cites the Accepted RFC, both design
      reviews, both handoffs, the exact implementation commit, and all fixture
      paths/hashes.
- [ ] The tracked worktree is clean before fixture/gate evidence is captured.
- [ ] Tests use only synthetic data and do not read, print, infer, or require
      real secrets or customer databases.
- [ ] No tag, push, publish, hosted release, M2 completion, or release
      authorization is performed.

## Historical fixture provenance

- [ ] `compat-v0_1.sqlite3` is produced through the public API of annotated tag
      `0.1.0` at `e44cfedc16cf96e3dfe97ad7ccbc1631b2398775` in an isolated
      historical checkout with locked dependencies.
- [ ] The v4 user-index fixture is produced through the `0.19.0` public
      `create_path_index` API or has equivalently strong exact-tag provenance.
- [ ] Generator sources and exact commands are committed.
- [ ] Provenance records source tag/commit, locked crates, Rust/Cargo host and
      target, SQLite runtime version, expected schema/version/rows/IDs/values,
      and fixture SHA-256.
- [ ] Fixtures contain synthetic non-secret data and no developer-identifying
      path, WAL, or SHM sidecar.
- [ ] A focused gate computes each committed fixture digest before copying or
      opening it and fails on any byte change.
- [ ] The v0.1 fixture contains two distinct real bincode 1.3 payloads, a
      nullable-hash case, nontrivial sequence/ID state where publicly
      producible, and physical `user_version = 0`.
- [ ] Synthetic cases cover physical version 1, a file without payload, and
      effective sequence high-water above the largest live ID.
- [ ] Tests copy immutable fixtures to a temporary directory before writable
      open and never mutate the committed originals.

## Fresh and version classification

- [ ] No non-internal application objects in `main` classifies as fresh.
- [ ] Exact released unversioned v0.1 shape classifies as effective v1.
- [ ] Every other non-empty physical-version-0 shape fails unchanged.
- [ ] Physical versions 1 through 5 require their exact semantic contract.
- [ ] Negative/out-of-range `user_version` values are rejected without lossy
      integer casts or mutation.
- [ ] Unsupported future versions fail without schema/data/version or
      persistent journal-mode change.
- [ ] Co-located unrelated table, index, view, or trigger is rejected.

## Table and column contracts

- [ ] Validation uses bound/static metadata inspection and never executes or
      interpolates database-derived SQL or object names.
- [ ] The limited DDL tokenizer/parser is read-only, rejects unsupported
      grammar, and proves AUTOINCREMENT, no explicit column collation, ordinary
      rowid, non-STRICT, and non-WITHOUT-ROWID form.
- [ ] `table_xinfo` validates exact column order, declared type/affinity,
      nullability, defaults, PK position, and `hidden = 0`.
- [ ] Extra, hidden, and generated columns are rejected.
- [ ] v1-v5 file/payload column sets and defaults exactly match RFC 010.
- [ ] UNIQUE terms use the required columns, order, and BINARY collation.
- [ ] The payload FK has the exact parent/key, CASCADE delete, NO ACTION update,
      and non-deferred behavior.
- [ ] Changed PK, UNIQUE, CHECK, FK, collation, table kind, or default is
      rejected unchanged.

## Index and allowed-object contracts

- [ ] `index_list`/`index_xinfo` validates exact built-in index names,
      table, term order, ASC/BINARY collation, uniqueness, origin, expression,
      and partial-predicate state.
- [ ] SQLite-owned auxiliary row-locator entries (`key = 0`, commonly
      `cid = -1`) are accepted and distinguished from user-defined key terms.
- [ ] A focused classifier unit test covers auxiliary rows for an ordinary
      index and UNIQUE autoindex.
- [ ] Physical versions 1-3 reject all non-internal extra objects.
- [ ] Versions 4-5 accept exact released `lc_user_*` path indexes only.
- [ ] Prefix-only lookalikes with changed table/term/order/collation/
      expression/uniqueness/predicate are rejected unchanged.
- [ ] Unexpected triggers are rejected before migration DML.
- [ ] A public-API-generated v4 `lc_user_*` index survives migration with the
      same definition and remains usable in a query.

## AUTOINCREMENT and sequence state

- [ ] DDL-token evidence and `sqlite_sequence` state jointly prove
      AUTOINCREMENT; neither alone is accepted.
- [ ] An absent `files` sequence row is accepted only for a never-written empty
      table.
- [ ] At most one `files` sequence row is accepted.
- [ ] NULL, REAL, TEXT, BLOB, duplicate, or negative sequence state is rejected
      unchanged.
- [ ] Effective high-water is `max(valid sequence, max live positive id)`.
- [ ] Migration preserves that high-water without adding one.
- [ ] `i64::MAX` is preserved without wrap/reuse; later insertion receives the
      expected SQLite exhaustion error.

## Valid-v5 no-write path

- [ ] Valid v5 opens through one consistent read snapshot.
- [ ] The validator passes on a `query_only` connection.
- [ ] `total_changes` remains zero.
- [ ] Schema/data/version semantic snapshots are identical before and after
      validation.
- [ ] Reopening current v5 is idempotent.

## v1 shadow migration and preservation

- [ ] One typed `Immediate` transaction owns authoritative classification,
      every schema step, the final version write, postconditions, and commit.
- [ ] Per-version helpers contain no nested transaction control or
      `user_version` update.
- [ ] Shadow parent and child tables coexist with old relations before any
      destructive drop.
- [ ] Files copy with identical IDs/metadata and namespace `default`.
- [ ] Payload `(file_id, content)` rows copy byte-for-byte without decoding.
- [ ] Files lacking payload remain without payload.
- [ ] Counts plus bidirectional set equivalence cover file and BLOB relations.
- [ ] FK checks pass before old-table drop and before commit.
- [ ] Old child drops before old parent.
- [ ] Parent/child renames preserve the expected FK target.
- [ ] Sequence high-water is restored and v2 built-in indexes are exact.
- [ ] No shadow object remains after success or rollback.

## Later migration steps

- [ ] v2-to-v3 adds `payload_version = 0` and `encoding = 'raw'` without
      changing other data.
- [ ] v3-to-v4 adds `last_accessed_at = 0` and the exact LRU index.
- [ ] v4-to-v5 runs exactly once inside the same outer transaction.
- [ ] File IDs/metadata, payload bytes/relationships, and valid public indexes
      remain unchanged except for fields explicitly owned by each step.

## v4 numeric domain

- [ ] Every pre-update `mtime` has SQLite storage class INTEGER.
- [ ] Inclusive values `-9_223_372_036` and `9_223_372_036` migrate exactly.
- [ ] Both just-outside values fail unchanged.
- [ ] REAL, TEXT, BLOB, and realistic already-nanosecond/partially migrated
      values fail before mutation.
- [ ] `(id, old_mtime)` is snapshotted transactionally before update.
- [ ] Every result remains INTEGER and equals `old_mtime * 1_000_000_000`.
- [ ] ID/count equivalence is bidirectional and the numeric snapshot is removed
      before final postconditions.

## Connection PRAGMA and durability boundary

- [ ] Caller-requested journal/synchronous options are not applied before
      classification and migration/no-write validation.
- [ ] File migration accepts existing WAL or disk-backed rollback mode and
      verifies connection-local synchronous FULL.
- [ ] Existing MEMORY/OFF file migration mode fails without persistent
      configuration or schema/data/version change.
- [ ] Every public journal/synchronous option migrates under the safe settings,
      then applies caller runtime settings after commit.
- [ ] Requested synchronous is applied/verified before requested journal mode,
      which is the final fallible configuration operation.
- [ ] Unrecognized inputs requested with WAL, MEMORY, and synchronous OFF keep
      semantic state and persistent journal mode unchanged.
- [ ] Normal SQLite recovery on connection open is documented outside
      localcache's no-mutation claim.
- [ ] Post-commit configuration failure uses the stable
      `database runtime configuration failed:` prefix and includes
      `schema_migration_committed`, requested values, and observed values.
- [ ] Tests ignore unstable trailing SQLite prose.
- [ ] In-memory fresh databases claim transactional error atomicity but not
      file-journal crash durability.

## Mandatory error failpoints

Each point returns a synthetic error, preserves the complete starting semantic
snapshot, leaves no shadow objects, and succeeds when retried without fault:

- [ ] After authoritative classification and preconditions.
- [ ] After shadow `files` creation.
- [ ] After shadow `payloads` creation.
- [ ] After parent copy.
- [ ] After child/payload copy.
- [ ] After bidirectional equivalence completion.
- [ ] After old `payloads` drop.
- [ ] After old `files` drop.
- [ ] After new `files` rename.
- [ ] After new `payloads` rename.
- [ ] After sequence restoration.
- [ ] After v2 index creation and v2-shape validation.
- [ ] After v2-to-v3.
- [ ] After v3-to-v4.
- [ ] After v4 numeric snapshot/preconditions.
- [ ] After v4-to-v5 conversion and exact equivalence.
- [ ] After final `user_version` write.
- [ ] After final shape/FK/postcondition validation.
- [ ] Immediately before commit.

## Panic, retry, and concurrency

- [ ] `catch_unwind` after old `files` drop proves semantic rollback under the
      unwind profile.
- [ ] Abort-profile process termination is not falsely claimed as deterministic
      in-process evidence.
- [ ] A held competing write transaction produces `SQLITE_BUSY` without
      partial mutation.
- [ ] No internal retry loop is used; documentation permits retrying the whole
      open after contention clears.
- [ ] A test barrier after preliminary read lets a second connection migrate;
      the first then authoritatively observes v5 and performs no migration
      writes.

## Documentation and code quality

- [ ] `docs/src/migration.md` distinguishes wire compatibility, atomic schema
      migration, unversioned v0.1 classification, strict co-location policy,
      numeric rejection, safe/runtime PRAGMAs, busy retry, post-commit errors,
      and normal backup practice.
- [ ] `CHANGELOG.md` records the correction under the coming-release section
      without marking it released.
- [ ] Public method signatures and error variant set remain source-compatible.
- [ ] Comments explain the SQLite-owned `index_xinfo` auxiliary row behavior.
- [ ] No database-derived SQL is executed or interpolated.
- [ ] No implementation expands into RFC 011 or RFC 012 scope.

## Implementation gates and review

- [ ] Fixture digest/provenance gates pass.
- [ ] Focused classifier and malformed-schema tests pass.
- [ ] Focused migration, numeric, PRAGMA, failpoint, panic, and concurrency
      tests pass.
- [ ] Existing compatibility and storage/migration suites pass.
- [ ] `cargo fmt --all --check` passes.
- [ ] Applicable package/workspace tests and feature gates pass on the exact
      implementation commit.
- [ ] `cargo clippy`/warning gates required at this milestone pass, or any
      unrun later-milestone gate is identified without being claimed.
- [ ] `mdbook build docs` passes and generated output is removed.
- [ ] `python3 scripts/source_integrity.py --require-tracked` passes.
- [ ] `git diff --check` passes.
- [ ] Slice 1 review accepts fixtures/classifier/no-write validation before
      destructive migration work begins.
- [ ] Final independent RFC 010 implementation review accepts the full change.
- [ ] Review packages reference the Accepted RFC, both design reviews, both
      handoffs, exact commit, fixture hashes, generated artifacts, and observed
      gates.
- [ ] B-02 is recorded closed only after full implementation acceptance.
- [ ] M2 remains incomplete until RFC 011 and the milestone's remaining exit
      gates are accepted.
