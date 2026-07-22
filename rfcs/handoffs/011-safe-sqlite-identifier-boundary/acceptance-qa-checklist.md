# RFC 011 Implementation Acceptance and Hostile-input QA Checklist

This checklist operationalizes the accepted
[RFC 011](../../accepted/011-safe-sqlite-identifier-boundary.md). The RFC is
authoritative; this checklist adds no permission to weaken or broaden it.

At checklist creation no implementation item is complete. Record the exact
implementation commit and observed command output before checking any item.

## Authority and preconditions

- [ ] RFC 011 remains under `rfcs/accepted/` with Status Accepted.
- [ ] Accepted transition commit
      `ff83f8a90356d22bd6acea16b94819040b1eb81b` is an ancestor of the
      implementation target.
- [ ] The initial design review at
      `.git-exclude/reviewed/architect-rfc-011-design-review-2026-07-21.md`
      and focused accepted re-review at
      `.git-exclude/reviewed/architect-rfc-011-focused-rereview-2026-07-21.md`
      are treated as review history, with the accepted RFC as normative design.
- [ ] This QA checklist is committed before implementation begins or is
      delegated.
- [ ] The implementation review package cites the Accepted RFC, both design
      reviews, this checklist, the exact implementation commit, and every test
      fixture or generated evidence path used by the review.
- [ ] The tracked worktree is clean before final gate evidence is captured.
- [ ] Tests use synthetic data only and never read, print, infer, or require
      real secrets or customer databases.
- [ ] No tag, push, publish, hosted release, B-03 closure, M2 completion, or
      release authorization is performed by this handoff.

## Scope discipline

- [ ] Implementation is limited to public SQLite index identifiers and their
      supporting internal metadata/SQL boundary.
- [ ] Schema version remains 5 and `PRAGMA user_version` is unchanged.
- [ ] RFC 010's accepted open-time schema set is not narrowed or broadened.
- [ ] RFC 012 read-only open/watcher work is not folded into this change.
- [ ] RFC 013 path/glob Unicode work and RFC 014 dependency/MSRV work remain
      separate.
- [ ] No arbitrary-index, raw-SQL-fragment, schema-qualifier, collation,
      expression, sort-direction, or planner-fallback API is added.
- [ ] Public signatures remain unchanged and no public error variant is added.

## Private identifier type boundary

- [ ] Raw suffixes, full caller spellings, authorized names, and quoted SQL
      identifiers have distinct private representations or equivalently strong
      type-state separation.
- [ ] SQL builders cannot accept a raw `&str` where an authorized identifier is
      required.
- [ ] No helper accepts caller-provided pre-quoting, schema qualification, or
      arbitrary SQL.
- [ ] Catalog-returned names and `sqlite_schema.sql` remain inspection data and
      are never interpolated into executable SQL.
- [ ] Executable SQL uses the authorized caller spelling, not a catalog-derived
      spelling.

## New-name grammar

- [ ] Creation of an absent index accepts suffixes of exactly 1 through 64
      UTF-8 bytes when every byte is ASCII `A..Z`, `a..z`, `0..9`, or `_`.
- [ ] Case is preserved and no trimming, replacement, normalization, or case
      folding occurs.
- [ ] A digit or underscore is accepted as the first suffix byte because the
      complete name has the fixed alphabetic `lc_user_` prefix.
- [ ] Empty and 65-byte suffixes are rejected before DDL.
- [ ] Whitespace, controls, Unicode, `$`, quotes, semicolons, dots, brackets,
      backticks, slash/comment punctuation, and every other punctuation byte
      are rejected for absent-object creation.
- [ ] A reserved-word suffix such as `select` is accepted, produces
      `lc_user_select`, and is still identifier-quoted.
- [ ] Supplying the full `lc_user_` name to `create_path_index` is treated as a
      suffix, not silently stripped or normalized.

## Identifier equivalence and encoding

- [ ] Identifier equivalence compares equal-length UTF-8 byte strings after
      folding only ASCII `A..Z` to `a..z`.
- [ ] Every non-ASCII byte is compared exactly; no Unicode/ICU/locale case fold
      or normalization is used.
- [ ] Resolution does not use `LIKE`, SQLite `lower`, or a
      connection-overridable collation.
- [ ] Tests cover equal ASCII case variants and unequal non-ASCII case or
      normalization variants.
- [ ] The one general encoder replaces each embedded `"` with `""` and wraps
      the full authorized identifier in `"..."`.
- [ ] Single quotes, square brackets, and backticks are never emitted as
      identifier delimiters.
- [ ] General encoder unit tests include embedded double quotes even though
      new-name grammar excludes them, preserving legacy-path coverage.

## Explicit main-schema boundary

- [ ] Object discovery reads `main.sqlite_schema`, never unqualified
      `sqlite_schema` when authorization depends on schema identity.
- [ ] Index-list metadata uses an explicit main schema argument equivalent to
      `pragma_index_list('files', 'main')`.
- [ ] Index-term metadata uses an explicit main schema argument equivalent to
      `pragma_index_xinfo(?1, 'main')`.
- [ ] Create uses one statement equivalent to
      `CREATE INDEX main."authorized_identifier" ON files(namespace, path)`.
- [ ] Create does not rely on `IF NOT EXISTS` as an ownership check.
- [ ] Drop uses one statement equivalent to
      `DROP INDEX main."authorized_identifier"`.
- [ ] Unhinted path query SQL uses `FROM main.files`.
- [ ] Hinted path query SQL uses
      `FROM main.files INDEXED BY "authorized_identifier"`.
- [ ] Static library-owned `main` is the only schema token; caller input cannot
      select TEMP or an attached schema.
- [ ] No caller-derived create/drop text reaches `execute_batch` or any other
      multi-statement execution API.

## Shared semantic ownership predicate

- [ ] RFC 010's public-index semantic predicate is shared or extracted rather
      than reimplemented as a weaker runtime check.
- [ ] RFC 011 does not blindly reuse RFC 010's current one-argument
      `index_xinfo` lookup plumbing.
- [ ] An owned public index has the exact `lc_user_` prefix and a non-NULL main
      schema definition on `main.files`.
- [ ] It is an ordinary `origin = 'c'`, non-unique, non-partial index.
- [ ] Its key terms are exactly `namespace ASC BINARY` then
      `path ASC BINARY`.
- [ ] Expressions, descending terms, changed collations, predicates, extra or
      hidden key terms, and invalid auxiliary rows are rejected.
- [ ] Allowed built-in hints are limited to exact current-v5
      `idx_files_namespace_path` and `idx_files_lru` semantic shapes.
- [ ] SQLite autoindexes, unrelated indexes, wrong-table indexes, TEMP or
      attached-only objects, prefix lookalikes, and arbitrary external indexes
      never become allowed query indexes.
- [ ] Existing RFC 010 classifier fixtures and near-match tests remain green,
      proving the shared-predicate refactor did not alter open-time policy.

## `create_path_index` contract

- [ ] Write guard runs before catalog resolution or DDL.
- [ ] One `Immediate` RAII transaction owns main-scoped resolution, optional
      DDL, result validation, and commit.
- [ ] An identifier-equivalent existing owned main index returns its catalog
      spelling without DDL, including ASCII case variants and authorized legacy
      names outside the new grammar.
- [ ] An identifier-equivalent conflicting main object returns the stable
      safety error without mutation.
- [ ] Only an absent main object proceeds to new-name grammar validation and
      main-qualified creation.
- [ ] Successful creation validates the complete owned shape before commit.
- [ ] A valid suffix absent from main creates in main even when TEMP or an
      attached schema contains the same spelling; the non-main object remains
      unchanged.
- [ ] Repeated valid creation is idempotent and returns the actual catalog
      spelling.

## `drop_path_index` contract

- [ ] Write guard runs before catalog resolution or DDL.
- [ ] One `Immediate` RAII transaction owns main-scoped resolution, DDL,
      absence validation, and commit.
- [ ] An absent main target returns `Ok(false)` without constructing DDL,
      including hostile, TEMP-only, and attached-only spellings.
- [ ] An existing main conflict which is not an owned path index returns the
      stable safety error and remains unchanged.
- [ ] An owned main target is dropped with one main-qualified statement and
      verified absent before commit.
- [ ] Same-named TEMP and attached objects remain present and byte/shape
      equivalent after main drop.
- [ ] A catalog-authorized legacy spelling can be dropped safely, but cannot
      be recreated after removal unless its suffix satisfies new-name grammar.

## `list_path_indexes` contract

- [ ] One `Deferred` RAII read transaction or equivalently proven SQLite read
      snapshot contains every metadata observation for the call.
- [ ] Discovery uses exact `lc_user_` prefix semantics, not
      `LIKE 'lc_user_%'` wildcard semantics.
- [ ] Every exact-prefix main candidate is structurally validated.
- [ ] Any invalid exact-prefix candidate in the snapshot makes the complete
      call return the stable safety error; it is neither omitted nor listed.
- [ ] Valid owned names, including legacy spellings, are returned in catalog
      name order.
- [ ] TEMP and attached candidates are ignored even when main has a same-named
      valid object.
- [ ] Documentation and tests treat returned validity as snapshot-bounded; a
      later independently validated operation may fail after schema change.

## `index_hint`, `run`, and `dry_run`

- [ ] The fluent setter remains non-fallible and stores opaque caller text.
- [ ] It takes a full index name; it does not add `lc_user_` automatically.
- [ ] `run()` and `dry_run()` call the same main-scoped resolver and SQL builder.
- [ ] Each terminal authorization decision uses one complete read snapshot.
- [ ] Allowed public legacy names and exact current-v5 built-ins work through
      both terminal paths.
- [ ] Missing, autoindex, unrelated, wrong-shape, TEMP-only, attached-only, and
      hostile hints return the stable safety error before SQL construction.
- [ ] An explicit rejected or unusable index never silently falls back to the
      automatic query planner.
- [ ] A schema change after authorization may produce `Database` at statement
      preparation, but cannot redirect to another schema, change identifier
      spelling, or expand parser structure.
- [ ] Both hinted and unhinted terminals read `main.files` when same-named TEMP
      and attached tables exist.

## Stable error contract

- [ ] Grammar rejection, unauthorized/missing hint, ownership conflict,
      ambiguity, and structural mismatch return
      `LocalFileCacheError::UnsupportedFeature`.
- [ ] The exact stable message is
      `SQLite index identifier is invalid or is not an allowed localcache index`.
- [ ] The message never echoes the rejected name, control characters, or other
      caller-provided text.
- [ ] SQLite locking, I/O, preparation, and execution failures remain
      `LocalFileCacheError::Database`.
- [ ] `ReadOnly` takes precedence for create/drop.
- [ ] Tests assert the stable localcache category/message and do not depend on
      unstable trailing SQLite prose.

## Hostile-input non-mutation matrix

For each applicable create, drop, `run()`, and `dry_run()` path, cover the
exact architect exploit and representative variants:

- [ ] `x ON files(namespace,path); DROP TABLE files; --`.
- [ ] Semicolon-separated second statements.
- [ ] Single quotes and doubled/single double quotes.
- [ ] `--` line comments and `/* ... */` block comments.
- [ ] Leading, trailing, and internal spaces; tab; CR; LF; and a control byte.
- [ ] Dot/schema qualifiers, brackets, and backticks.
- [ ] Dollar signs and representative Unicode.
- [ ] Empty, one-byte, 64-byte, and 65-byte boundaries.
- [ ] Reserved-word suffix acceptance is distinguished from hostile rejection.

For every rejection or absent-drop result:

- [ ] Snapshot main schema object names/types/definitions and `user_version`.
- [ ] Snapshot index flags and exact terms.
- [ ] Snapshot file/payload row counts and representative payload bytes.
- [ ] Assert exact semantic equality afterward.
- [ ] Assert `files` and `payloads` still exist and no injected object appears.
- [ ] Prove subsequent public read and write operations still succeed.
- [ ] Test setup never interpolates hostile input into a setup batch.

## Main/TEMP/attached collision matrix

- [ ] Same-named main and TEMP `files` tables/indexes contain distinguishable
      rows and terms.
- [ ] Same-named main and attached-schema tables/indexes contain
      distinguishable rows and terms.
- [ ] Metadata resolution always reports the main flags and terms.
- [ ] Unhinted, public-hinted, and built-in-hinted `run()` read main rows only.
- [ ] Corresponding `dry_run()` plans name the authorized main index and never
      resolve a TEMP/attached table source.
- [ ] Drop removes only the authorized main index.
- [ ] Create absent from main creates main even with a same-named TEMP or
      attached object and preserves those objects unchanged.
- [ ] TEMP/attached-only hint is unauthorized and TEMP/attached-only drop is
      `Ok(false)`.
- [ ] Listing returns main owned indexes only.

## Snapshot and two-connection races

- [ ] A private deterministic barrier can pause after the first metadata read
      and before later reads without adding a production/environment switch.
- [ ] In a journal mode permitting a second writer to commit, the paused
      resolver/list operation observes one schema generation rather than mixed
      flags and terms.
- [ ] A snapshot containing an invalid exact-prefix main candidate fails the
      entire list call deterministically.
- [ ] After list snapshot completion, a second connection changes schema and a
      later operation revalidates and fails safely.
- [ ] After hint authorization but before preparation, a second connection
      changes schema; `run()` and `dry_run()` either use the authorized main
      object or return `Database` without fallback or schema redirection.
- [ ] Race barriers have bounded waits and deterministic cleanup on assertion
      failure; no test can hang indefinitely.

## Post-DDL rollback and nesting

- [ ] A private `cfg(test)` failpoint or equivalent deterministic hook exists
      immediately after successful create DDL and before postconditions/commit.
- [ ] A corresponding hook exists immediately after successful drop DDL and
      before postconditions/commit.
- [ ] Hooks are not public, environment-controlled, or compiled into
      production behavior.
- [ ] A returned error after create DDL rolls back the newly created index.
- [ ] A returned error after drop DDL restores the dropped index exactly.
- [ ] At least one caught normal panic unwind after destructive drop DDL
      restores the complete starting state.
- [ ] No in-process RAII rollback claim is made for `panic=abort`.
- [ ] Complete snapshots include main definitions, index flags/terms,
      `user_version`, file/payload rows and bytes, and collision objects where
      present.
- [ ] An internal outer-transaction test proves nested
      `new_unchecked(..., Immediate)` rejection occurs before schema/data
      mutation.
- [ ] Fault-free retry after each injected failure succeeds.

## Released-name compatibility

- [ ] A v4/v5 database with released mixed-case, dollar-sign, and Unicode
      `lc_user_*` names reopens without applying new-name grammar during RFC
      010 classification.
- [ ] Each valid legacy name is listed, hinted through `run()` and `dry_run()`,
      and dropped safely.
- [ ] An idempotent create call naming an existing legacy object returns the
      catalog spelling without DDL.
- [ ] After legacy removal, the same non-grammar suffix is rejected for absent
      creation.
- [ ] Existing immutable RFC 010 v4 fixture digest, migration, definition, and
      usability assertions remain unchanged and passing.
- [ ] Historical compatibility evidence is not weakened by generating odd
      names only through the newly restricted implementation.

## Read-only and runtime parity

- [ ] Create/drop on a read-only engine return `ReadOnly` before validation or
      metadata-dependent policy errors.
- [ ] List and authorized query hints remain read-only operations.
- [ ] Every enabled async runtime wrapper delegates to the same synchronous
      grammar, authorization, error, and transaction behavior.
- [ ] Async create/drop/list and query dry-run coverage observes the same
      success and rejection contracts.
- [ ] No mutex panic, watcher, or read-only-open scope from RFC 012 is pulled
      into this implementation.

## Documentation and historical RFC correction

- [ ] Public `create_path_index` docs state suffix grammar and 64-byte limit.
- [ ] Public `drop_path_index` docs explain absent-main `false`, owned legacy
      cleanup, and main-only behavior.
- [ ] Public `list_path_indexes` docs explain structural filtering and
      snapshot-bounded validity.
- [ ] `QueryBuilder::index_hint` docs say it takes the full name, validates at
      each terminal, and returns the revised safety error when unauthorized.
- [ ] Live docs explain that SQLite `INDEXED BY` is a requirement despite the
      public “hint” name and never silently falls back.
- [ ] Live docs list the known built-in hint names and explain their stability.
- [ ] Live docs explain that valid legacy names remain usable/removable but
      cannot necessarily be recreated.
- [ ] `CHANGELOG.md` records the security correction and behavior narrowing
      under the coming-release section without marking it released.
- [ ] RFC 002 receives a short historical supersession note linking RFC 011
      only when RFC 011 implementation reaches Implemented state.

## Implementation gates and review

- [ ] Focused identifier/encoder/metadata unit tests pass.
- [ ] Focused public hostile-input and collision integration tests pass.
- [ ] Focused snapshot-race and post-DDL failpoint tests pass.
- [ ] RFC 010 classifier, fixture-integrity, migration, and public-boundary
      suites pass without accepted-shape regression.
- [ ] Existing query/index and async-runtime suites pass.
- [ ] `cargo fmt --all --check` passes.
- [ ] Applicable package/workspace tests and feature gates pass on the exact
      implementation commit.
- [ ] Warnings-denied clippy and documentation gates required at this
      milestone pass, or any later-milestone gate not run is identified without
      being called passed.
- [ ] `mdbook build docs` passes and generated `docs/book/` is removed.
- [ ] `python3 scripts/source_integrity.py --require-tracked` passes.
- [ ] Applicable RFC 009 source/archive smoke gates pass.
- [ ] `git diff --check` passes.
- [ ] The implementation review package references the Accepted RFC, initial
      design review, focused design re-review, this QA checklist, exact
      implementation commit, fixture paths/hashes, generated artifacts, and
      every observed/unrun gate.
- [ ] Focused independent implementation review accepts the complete RFC 011
      implementation before B-03 is recorded closed.
- [ ] RFC 011 remains under `rfcs/accepted/` until implementation ships in the
      authorized release; it is not moved to `done/` merely because code is
      written or reviewed.
- [ ] M2 remains incomplete until B-03 implementation acceptance and all other
      milestone exit gates are satisfied.
- [ ] No release action is inferred from green implementation gates.
