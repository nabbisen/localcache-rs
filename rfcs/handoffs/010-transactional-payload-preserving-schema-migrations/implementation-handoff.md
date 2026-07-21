# RFC 010 Implementation Handoff

## 1. Summary

Implement the writable migration/data-integrity boundary defined by
[RFC 010](../../accepted/010-transactional-payload-preserving-schema-migrations.md).
RFC 010 is Accepted; its requirements are authoritative. This handoff
sequences the work and identifies review evidence but does not add or override
design decisions.

The Accepted transition is commit
`edbe4fbfd58b8071b33ee7b9a85b0549d1a7518c`. The design authority trail is:

1. originating B-02 findings:
   `.git-exclude/reviewed/architect-preparation-review-2026-07-17.md` and
   `.git-exclude/reviewed/architect-preparation-review-2026-07-18.md`;
2. first design review:
   `.git-exclude/reviewed/architect-rfc-010-design-review-2026-07-21.md`;
3. focused design acceptance:
   `.git-exclude/reviewed/architect-rfc-010-focused-rereview-2026-07-21.md`;
4. accepted RFC:
   `rfcs/accepted/010-transactional-payload-preserving-schema-migrations.md`;
   and
5. companion QA authority:
   `rfcs/handoffs/010-transactional-payload-preserving-schema-migrations/acceptance-qa-checklist.md`.

Every implementation review request must cite all five authority groups above
so a reviewer who did not participate in design can reconstruct the contract.

RFC 010 implementation closes B-02 only. It does not close RFC 011's SQLite
identifier injection risk, RFC 012's read-only boundary, all of M2, or any
release gate. It performs no tag, push, publish, hosted release, or release
authorization.

## 2. Scope followed

### Implementation scope

1. Generate and commit immutable historical v0.1 and released-v4 user-index
   fixtures with auditable provenance and executable digest checks.
2. Implement exact, non-executing schema classification for physical versions
   0 through 5, including table/index/FK/sequence contracts and allowed-extra
   object policy.
3. Make valid-v5 initialization a consistent no-write validation path.
4. Reorder connection PRAGMAs so caller runtime settings cannot mutate or
   weaken a rejected/migrating database.
5. Put every required starting-version-to-v5 step, the one final
   `user_version` write, and all postconditions inside one typed `Immediate`
   transaction.
6. Replace v1 through paired parent/child shadow tables only after
   byte-inclusive, bidirectional relational equivalence succeeds.
7. Preserve IDs, payload relationships, valid v4 public indexes, and the
   effective AUTOINCREMENT high-water through `i64::MAX`.
8. Validate and convert the full released v4 `mtime` input domain without
   SQLite numeric promotion or double conversion.
9. Implement the nineteen returned-error failpoints, destructive panic-unwind
   proof, successful retry, and deterministic concurrency barriers required by
   R8.
10. Update migration documentation and the coming-release changelog without
    marking a release or M2 complete.

### Explicitly out of scope

- Designing or fixing caller-controlled SQLite identifiers (RFC 011).
- Read-only schema validation and mutation/watcher policy (RFC 012).
- Advancing schema version beyond 5 or changing payload encoding.
- Repairing arbitrary corrupt, co-located, or hand-modified databases.
- Automatic backup-file creation or a migration CLI.
- New public cache features.
- M2 completion, release-candidate work, or release actions.

### Recommended slices and review points

#### Slice 1 — Fixtures, classifier, and no-write validation

- Commit historical fixture generators/provenance/digests and fixture tests.
- Implement semantic schema contracts, allowed-object policy,
  AUTOINCREMENT/sequence validation, and negative near-match cases.
- Implement valid-v5 consistent read-snapshot validation with no migration
  writes.
- Add the SQLite-owned `index_xinfo` auxiliary-row classifier test from the
  focused review's non-blocking finding.

Stop for a focused implementation review before enabling destructive migration
behavior. Its review package must reference the Accepted RFC, both handoffs,
both design reviews, exact commit, generated fixture paths/hashes, and observed
test commands.

#### Slice 2 — Transactional migration and connection boundary

- Reorder/verify PRAGMAs and define stable post-commit configuration errors.
- Implement the typed outer transaction, shadow migration, later steps,
  sequence restoration, checked v4 conversion, and final postconditions.
- Add all failpoint, panic-unwind, option-matrix, concurrency, retry, and
  preservation tests.
- Update docs/changelog and run complete applicable implementation gates.

Stop for independent RFC 010 implementation acceptance. Do not record B-02
closed merely because Slice 1 passed.

## 3. Files changed

This handoff commit is expected to contain only:

- `implementation-handoff.md`;
- `acceptance-qa-checklist.md`; and
- the Accepted RFC's companion-handoff links.

Expected implementation areas, subject to the Accepted RFC, are:

- `src/db/schema.rs`, or small private modules extracted from it when that
  makes the classifier/migrator boundary easier to review;
- `src/cache/engine.rs` for open/configuration ordering;
- `src/cache/options.rs` only if internal helpers are needed without changing
  public option semantics;
- `src/error.rs` documentation only unless design review is reopened; RFC 010
  deliberately retains the existing public variant set;
- focused schema/migration integration or unit tests under `tests/` and/or the
  private database module;
- `tests/fixtures/compat-v0_1.sqlite3` and the exact-tag v4 user-index fixture;
- committed fixture generators/provenance under a reviewable
  `tests/fixtures/` or `scripts/` path;
- `tests/fixtures/README.md`;
- `docs/src/migration.md`; and
- `CHANGELOG.md` under the already-prepared coming-release section.

Do not combine RFC 011 implementation into these files merely because both
touch SQLite. If implementation requires a design choice not settled by RFC
010, stop and return to RFC review.

## 4. Design decisions and assumptions

- Fresh means no non-internal application object in `main`; an unrelated
  object is not fresh.
- Physical version 0 is either empty, exact released v0.1/effective v1, or
  rejected unchanged.
- Schema validation combines bound metadata inspection with a limited
  non-executing DDL tokenizer. Database-derived SQL/names are never executed or
  interpolated.
- `table_xinfo`/`index_xinfo` hidden and auxiliary rows must be interpreted
  according to SQLite semantics. The unavoidable SQLite-owned rowid locator
  (`key = 0`, commonly `cid = -1`) is allowed; extra user-defined key terms,
  expressions, collations, ordering, uniqueness, or predicates are not.
- Versions 4 and 5 preserve only exact released `lc_user_*` path indexes. RFC
  011 owns creation-time identifier safety.
- Caller-requested MEMORY/OFF settings are withheld until after migration
  commit or no-write v5 validation. File migrations use an existing disk
  rollback journal/WAL plus verified synchronous FULL.
- `SQLITE_BUSY` has no internal retry; the caller may retry the complete open.
- Post-commit configuration failure reuses
  `LocalFileCacheError::UnsupportedFeature(String)` with the RFC's stable
  prefix/fields. A dedicated error variant is deferred to a future breaking
  API boundary.
- Migration copies payload BLOBs without decoding them. Known fixture values
  are decoded separately through the public API.
- v4 `mtime` conversion accepts only INTEGER storage in the exact safe-seconds
  range; ambiguous in-range values remain legitimate released inputs.
- AUTOINCREMENT high-water and `i64::MAX` behavior are preserved without
  increment/wrap.
- Bidirectional SQL equivalence may use SQLite temporary storage. Resource
  exhaustion is an ordinary rollback error, not a performance guarantee.
- One primary implementer should own each slice. Independent implementation
  review is a separate gate.

## 5. Tests and gates run

At handoff creation, observed design/lifecycle evidence against Accepted
transition commit `edbe4fbfd58b8071b33ee7b9a85b0549d1a7518c` is:

- focused architecture design verdict — **Accept**, no blocking findings;
- `git diff --check c98bc3f7b9dff92e4d3e81e79f49af7c3bb3f98d..edbe4fbfd58b8071b33ee7b9a85b0549d1a7518c`
  — PASS;
- `python3 scripts/source_integrity.py --require-tracked` — PASS, 2 manifests
  and 7 manifest targets;
- `mdbook build docs` — PASS; generated `docs/book/` removed afterward; and
- tracked worktree — clean before handoff authoring.

No RFC 010 implementation, fixture-generation, database-migration, Cargo test,
clippy, MSRV, package, security, or release gate is claimed as passing. Those
missing gates block implementation acceptance but do not invalidate the
Accepted design or this delegation handoff.

## 6. Generated artifacts

This handoff generates repository documentation only:

- `implementation-handoff.md`;
- `acceptance-qa-checklist.md`; and
- Accepted RFC links to both documents.

Future implementation must generate and retain in the repository:

- immutable `compat-v0_1.sqlite3` fixture;
- exact-tag v4 public-user-index fixture;
- committed fixture generator sources and provenance records; and
- executable fixture SHA-256 expectations.

Temporary historical checkouts, Cargo targets, WAL/SHM files, test databases,
and diagnostic evidence stay outside tracked source unless the Accepted RFC
explicitly requires the artifact. They must contain no secrets, credentials,
private paths, or customer data.

## 7. Known limitations

- B-02 remains present in production code until Slice 2 is accepted.
- Neither historical fixture nor generator exists yet.
- The exact schema classifier, DDL tokenizer, and negative matrix do not exist.
- The current v1 migration still drops payloads; do not run destructive manual
  migration experiments on user databases.
- Current `CacheEngine::open` still applies caller PRAGMAs before schema
  initialization.
- Current v4 multiplication can promote overflow to REAL and can double-convert
  a partially migrated database.
- The nineteen failpoints and deterministic concurrency gates do not exist.
- The stable-string error compromise is accepted for v0.20.1 but should be
  retired at a future breaking API boundary.
- RFC 011 remains separately blocking for public SQLite identifier injection.
- Acceptance of RFC 010 is design evidence, not release evidence.

## 8. Recommended next step

After the owner commits this handoff, begin Slice 1 only:

1. add committed historical generator sources and immutable fixtures;
2. add executable fixture-digest/provenance tests;
3. implement the fail-closed schema classifier and valid-v5 no-write validator;
4. implement the focused positive/negative classifier matrix, including
   SQLite-owned `index_xinfo` auxiliary rows; and
5. create a Slice 1 implementation review request that references the Accepted
   RFC, both architecture reviews, this handoff, the QA checklist, exact commit,
   fixture paths/hashes, and observed commands.

Do not implement destructive migration before the Slice 1 review point. Use
the companion checklist throughout and return to RFC review if code reveals a
design conflict.
