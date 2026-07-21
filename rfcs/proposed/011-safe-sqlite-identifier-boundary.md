# RFC 011 — Safe SQLite Identifier Boundary

| Field | Value |
|---|---|
| Status | Proposed |
| Feature | *(core database safety; no Cargo feature)* |
| Touches | `crates/localcache/src/cache/engine.rs`, `crates/localcache/src/cache/query.rs`, `crates/localcache/src/db/repository.rs`, shared index-metadata helpers, query/index tests, API documentation |
| Finding | Architect review B-03 |
| Milestone | Phase 21 M2 |

## Summary

Treat every SQLite index name received through a public API as untrusted.
New path-index suffixes receive a small ASCII grammar, every identifier placed
in SQL is standard double-quoted by one shared helper, and no caller-derived
text reaches `execute_batch`.

An index hint or existing-index operation must also resolve through a bound
catalog lookup and match an allowlist of localcache-owned index shapes before
its quoted identifier is used. This second boundary preserves safe use and
removal of structurally valid indexes created by released versions even when
their names fall outside the new creation grammar.

This RFC corrects and supersedes the security assumption in
[RFC 002](../done/002-query-index-hints.md): an identifier is not safe merely
because it came from calling code rather than a request handler. Bind
parameters protect values, not SQL grammar positions such as an index name.

This RFC does not authorize implementation while Proposed, does not change the
database schema version, does not complete M2 by itself, and does not authorize
a release.

## Motivation

`CacheEngine::create_path_index` currently prefixes its argument and inserts
the result into a `CREATE INDEX` statement passed to `execute_batch`.
`drop_path_index` does the same for `DROP INDEX`. `QueryBuilder::index_hint`
stores an arbitrary string which the repository inserts after `INDEXED BY`.

Those are SQL syntax positions and cannot be represented by a bound value.
The current prefix is not a security boundary: punctuation in the suffix can
terminate or reshape the intended statement. The independent architecture
review demonstrated the impact with this suffix:

```text
x ON files(namespace,path); DROP TABLE files; --
```

The resulting batch removed the `files` table. Quotes, comments, whitespace,
and other parser tokens expose equivalent classes of ambiguity. The
query-hint path is also unsafe even though its terminal uses statement
preparation rather than `execute_batch`; arbitrary text is still inserted into
the statement before SQLite parses it.

The correction must not make a different compatibility mistake. Released
versions successfully created ordinary mixed-case, dollar-sign, and Unicode
`lc_user_*` indexes. RFC 010 intentionally accepts those databases by semantic
index shape rather than retroactively applying a new spelling grammar. RFC 011
therefore separates **creation policy** from **safe existing-object policy**.

## Goals

1. Prevent caller-controlled index text from changing SQL statement structure.
2. Define one deterministic grammar for newly created public path indexes.
3. Allow query hints only for known, structurally valid localcache indexes.
4. Preserve safe discovery, use, and removal of valid released user indexes.
5. Keep synchronous and asynchronous APIs behaviorally aligned.
6. Prove the boundary with hostile inputs and before/after database assertions.

## Non-goals

- General-purpose creation or hinting of arbitrary application indexes.
- Accepting caller-provided SQL fragments, quoting styles, schema qualifiers,
  collation clauses, expressions, or sort directions.
- Renaming or dropping valid legacy indexes merely because their spelling is
  outside the new creation grammar.
- Changing the v5 schema, `PRAGMA user_version`, or RFC 010's open-time schema
  classification policy.
- Repairing an already exploited, structurally unrecognized database.
- Completing RFC 012's read-only connection and mutation boundary.
- Treating identifier validation as a substitute for bound parameters for SQL
  values.

## Terminology

- **Suffix**: the string passed to `create_path_index` or `drop_path_index`.
- **Public index name**: `lc_user_` followed by a suffix.
- **New-name grammar**: the syntax permitted when creating a public index after
  this RFC is implemented.
- **Legacy spelling**: a structurally valid public index name already present
  in a released v4/v5 database but not admitted by the new-name grammar.
- **Identifier equivalence**: SQLite's identifier comparison behavior. ASCII
  letters are case-insensitive; non-ASCII spelling is not Unicode-folded.
- **Owned path index**: an index which has the public prefix and the exact
  released non-unique, non-partial ordinary index shape on
  `files(namespace ASC BINARY, path ASC BINARY)`.
- **Allowed query index**: either an owned path index or a known built-in v5
  `files` index whose complete expected shape has been verified.

## Requirements

### R1 — New-name grammar

`create_path_index(suffix)` may create a previously absent index only when all
of these conditions hold:

- its UTF-8 byte length is in `1..=64`;
- every byte is one of ASCII `A..Z`, `a..z`, `0..9`, or `_`; and
- the caller supplies the suffix only, not the `lc_user_` prefix.

The full new name is exactly `lc_user_{suffix}`. Case is preserved; no Unicode
normalization, trimming, case folding, or replacement is performed. Because
the fixed prefix begins with letters, the suffix may begin with a digit or
underscore without making the full identifier ambiguous.

The grammar intentionally excludes whitespace, control characters, Unicode,
`$`, quotes, semicolons, dots, brackets, backticks, comment markers, and every
other punctuation character. A suffix such as `select` is accepted: the full
name is not a bare keyword, and R2 still quotes it. This reserved-word case is
part of the regression matrix.

The 64-byte limit bounds generated SQL and error-path work independently of
SQLite build-time SQL-length limits. The limit applies only to new creation;
it must not make an otherwise valid historical database fail to open.

### R2 — One identifier encoder and one statement

All dynamic identifiers use one private encoder with this exact contract:

1. prefix and suffix validation or catalog authorization occurs first;
2. each `"` in the authorized identifier is replaced by `""`; and
3. the result is enclosed in ASCII double quotes.

Even identifiers admitted by the ASCII grammar are quoted. Quoting is a
separate defense, handles future SQLite keywords, and is required for the
catalog-authorized legacy path. Single quotes, square brackets, and backticks
must not be emitted as identifier delimiters.

`CREATE INDEX` and `DROP INDEX` use `Connection::execute` (or the equivalent
single-statement API), never `execute_batch`. The dynamic identifier is the
only generated SQL fragment. Table names, columns, ordering, and all other SQL
tokens remain static.

No helper may accept a pre-quoted identifier or arbitrary SQL. Its input and
output types must make it difficult to confuse a suffix, a full catalog name,
and encoded SQL text.

### R3 — Catalog-backed ownership and allowlists

Catalog checks use static SQL with bound values. Prefix discovery must use an
exact prefix expression such as `substr(name, 1, 8) = 'lc_user_'`; the current
`LIKE 'lc_user_%'` is forbidden because `_` is a wildcard.

The runtime structural predicate for an owned path index is the same semantic
predicate used by RFC 010's v4/v5 classifier:

- one ordinary schema index with non-NULL definition on `main.files`;
- exact `lc_user_` prefix;
- `unique = 0`, `origin = 'c'`, and `partial = 0`;
- exactly `namespace ASC BINARY` then `path ASC BINARY` as key terms;
- no expression, hidden extra key, changed collation, descending term, or
  predicate; and
- ordinary row-locator metadata only in non-key auxiliary rows.

Implementation must share or reuse this predicate rather than maintain a
weaker second interpretation. Catalog SQL and names are inspection inputs; the
caller-provided spelling, after catalog authorization and identifier encoding,
is what is placed in executable SQL. Database-derived `sqlite_schema.sql` is
never executed.

An allowed query index is one of:

- an owned path index; or
- an exact built-in v5 `files` index (`idx_files_namespace_path` or
  `idx_files_lru`) matching RFC 010's complete expected shape.

SQLite autoindexes, indexes on other tables, TEMP or attached-schema objects,
lookalike prefixes, and arbitrary externally-created indexes are never allowed
as query hints.

Catalog resolution uses SQLite identifier equivalence, not Rust Unicode case
folding. The resolver must return zero or one match; ambiguity or a shape
mismatch fails closed. It must not use caller-controlled schema qualifiers.

### R4 — Public operation contracts

#### `create_path_index`

The public signature and successful return type remain unchanged.

1. Enforce the write guard.
2. In an `Immediate` transaction, prefix the suffix in memory and resolve any
   identifier-equivalent existing schema object through a bound lookup.
3. If an owned path index already exists, commit no schema change and return
   its catalog spelling. This preserves released idempotence for legacy names
   and ASCII case variants without placing the name in SQL.
4. If a conflicting object exists, roll back and fail closed.
5. If no object exists, validate the suffix using R1, create exactly the owned
   path-index shape with one quoted single statement, validate the result, and
   commit.
6. If validation or post-creation validation fails, roll back and fail closed.

`IF NOT EXISTS` alone is not an ownership check and must not mask a table,
view, wrong-shape index, or concurrent conflict.

#### `drop_path_index`

The public signature and `bool` meaning remain unchanged.

1. Enforce the write guard.
2. Prefix the caller's suffix in memory and resolve it with a bound catalog
   lookup inside an `Immediate` transaction.
3. If no identifier-equivalent object exists, return `Ok(false)` without DDL.
4. If the object is an owned path index, encode the caller-provided full name,
   drop it with one statement, verify absence, commit, and return `Ok(true)`.
5. If a matching object exists but is not owned, fail closed without dropping
   it.

Drop deliberately permits a catalog-authorized legacy spelling outside R1.
This is a cleanup and compatibility path, not permission to create that name.
An arbitrary hostile suffix which does not resolve to an owned object cannot
reach executable SQL.

#### `list_path_indexes`

Return every structurally valid owned path index, including legacy spellings,
in catalog-name order. Do not list a name based on prefix alone. If a matching
prefix object changes concurrently into an invalid shape, return an error
rather than advertise it as safe.

#### `QueryBuilder::index_hint`

The fluent setter remains non-fallible and stores opaque caller text. Both
terminal paths, `run()` and `dry_run()`, resolve the full supplied name through
the R3 allowlist immediately before constructing their SQL. The authorized
caller spelling is then encoded and inserted after `INDEXED BY`.

A valid legacy public name may be hinted because its existing catalog object,
not its spelling alone, grants authorization. A new, nonexistent name does not
receive that exception. A catalog change after resolution can at worst make
statement preparation return an error; encoding prevents it from changing SQL
structure.

### R5 — Error and compatibility contract

No public method signature changes and no new `LocalFileCacheError` variant is
added in this corrective release.

- A rejected creation name, unauthorized hint, ownership conflict, ambiguity,
  or structural mismatch returns `LocalFileCacheError::UnsupportedFeature`
  with the stable, non-echoing message
  `SQLite index identifier is invalid or is not an allowed localcache index`.
- A syntactically valid but absent `drop_path_index` target remains
  `Ok(false)`. A hostile absent suffix also returns `Ok(false)` because no SQL
  is constructed for it.
- SQLite I/O, locking, preparation, and execution failures remain
  `LocalFileCacheError::Database`.
- `ReadOnly` takes precedence for create/drop exactly as it does today.

This intentionally changes RFC 002's statement that a nonexistent hint is
reported as `Database`. The stable contract after RFC 011 is the safety error
above because allowlist rejection occurs before statement preparation. The
change avoids adding a public enum variant in v0.20.1 while distinguishing
policy rejection from SQLite operational failure.

Errors must not echo the rejected identifier. This prevents control characters
or attacker-chosen text from being copied into ordinary logs by the library.

Released database compatibility is preserved as follows:

- RFC 010 classification continues to accept every structurally valid v4/v5
  public index spelling; R1 is not applied during database open.
- `list_path_indexes` continues to expose those valid catalog names.
- `index_hint` and `drop_path_index` can use them only after the exact R3
  ownership check and R2 encoding.
- `create_path_index` applies R1 before creating every absent object. A call
  naming an existing owned legacy index remains an idempotent success, but the
  same spelling cannot be recreated after that index is removed.

Documentation must call out this security-driven creation restriction and the
legacy compatibility path in the coming-release notes.

### R6 — Transaction, race, and mutation guarantees

Create and drop use an `Immediate` transaction for catalog resolution, DDL,
postcondition validation, and commit. A returned error or panic-unwind before
commit rolls back the operation. Because the released methods take `&self`, a
rusqlite RAII transaction created with `Transaction::new_unchecked` and
`TransactionBehavior::Immediate` is permitted: “unchecked” there means Rust's
mutable-borrow nested-transaction exclusion is checked at runtime instead.
Raw `BEGIN`/`COMMIT` batch text is forbidden. A nested-transaction error must
occur before mutation, and implementation must not catch an error and commit
partial work.

Query hint resolution is read-only. A second connection can alter schema after
resolution and before statement preparation; that race may produce a normal
`Database` error, but cannot authorize another identifier or statement because
the encoded caller spelling is already fixed. No retry silently switches to
the automatic query planner.

Invalid create, drop, hint, and list inputs must not change tables, rows,
indexes, `user_version`, journal mode, or connection runtime settings.

### R7 — Documentation and prior-RFC correction

Update public API documentation and the query/index guide to state:

- the exact new suffix grammar and byte limit;
- `create_path_index` takes a suffix while `index_hint` takes the full name;
- `list_path_indexes` is the discovery source for public hints;
- known built-in hint names and their stability status;
- terminal-time allowlist validation and the revised error variant;
- legacy names may be listed, hinted, and dropped but cannot necessarily be
  recreated; and
- identifier arguments must be treated as untrusted even when only trusted
  application code normally calls the API.

RFC 002 remains historical and is not edited to pretend its original security
reasoning was correct. It receives a short supersession note linking to RFC
011 when RFC 011 reaches Implemented state, and the live documentation follows
RFC 011 immediately with implementation.

## Detailed design

### Typed internal boundary

The implementation should introduce a small private module, or an equivalent
private type boundary, containing concepts equivalent to:

```rust,ignore
struct NewPathIndexName { /* validated full caller spelling */ }
struct AuthorizedIndexName { /* catalog-authorized caller spelling */ }
struct QuotedIdentifier(String);

fn new_path_index_name(suffix: &str) -> Result<NewPathIndexName, ...>;
fn resolve_owned_path_index(conn: &Connection, full: &str)
    -> Result<Option<AuthorizedIndexName>, ...>;
fn resolve_query_index(conn: &Connection, full: &str)
    -> Result<AuthorizedIndexName, ...>;
fn quote_identifier(name: &AuthorizedIndexName) -> QuotedIdentifier;
```

The names are illustrative, not mandated. The important invariant is that SQL
builders cannot receive a raw `&str` where an authorized identifier is
required. The quote helper also receives direct unit tests with embedded
double quotes even though R1 excludes them, because legacy authorization uses
the general encoder.

### Query construction

`build_path_sql` becomes fallible or receives an already authorized and
encoded optional hint. `keys` and `explain_query` must call the same resolver,
so `run()` and `dry_run()` cannot drift. Parameter values for namespace and
path predicates remain bound exactly as today.

The only two table-source forms are conceptually:

```sql
FROM files
FROM files INDEXED BY "authorized_identifier"
```

No fallback is permitted when an explicit hint is rejected or SQLite cannot
use it. `INDEXED BY` is a requirement, not a planner suggestion, so silently
discarding it would hide application and schema errors.

### Shared semantic metadata

RFC 010 already reads `sqlite_schema`, `pragma_index_list`, and
`pragma_index_xinfo` using bound values and validates the exact public-index
shape. RFC 011 should extract the reusable row types and semantic predicate
only as far as necessary for runtime ownership checks. It must not weaken or
redefine open-time classification, parse database names into SQL, or broaden
the set of schema objects RFC 010 accepts.

## Test plan

### Grammar and encoding

- Accept one-byte, 64-byte, mixed-case, numeric, underscore, and reserved-word
  suffixes; create, list, hint with both terminal paths, and drop them.
- Reject empty and 65-byte suffixes.
- Reject semicolons, single and double quotes, `--`, `/* */`, ASCII whitespace,
  newlines, tabs, control characters, dots/schema qualifiers, brackets,
  backticks, dollar signs, and representative Unicode.
- Unit-test standard double-quote doubling for catalog-authorized legacy text.

### Exploit regression

Exercise the architect's exact suffix and variants for create, drop, `run`,
and `dry_run`. Snapshot schema objects, `user_version`, file/payload row counts,
and representative payload bytes before each operation. Assert the expected
safety error or `false`, exact snapshot equality afterward, and continued
public reads/writes. Specifically assert that `files` and `payloads` still
exist and that no injected table or index appears.

### Catalog authorization

- Allow exact current built-in indexes and owned public path indexes.
- Reject nonexistent hints, SQLite autoindexes, unrelated indexes, attached or
  TEMP objects, wrong-table indexes, wrong terms/order/collation, unique,
  expression, descending, and partial indexes.
- Prove prefix lookalikes are not selected by wildcard behavior.
- Mutate schema from a second connection after engine open and prove create,
  drop, list, and hint fail closed on conflicts without unrelated mutation.
- Prove ASCII case-equivalent create is idempotent and returns catalog
  spelling; prove non-ASCII names are not Unicode-case-folded.

### Released compatibility

- Reopen a v4/v5 database containing the released mixed-case, dollar-sign, and
  Unicode public-index spellings without applying R1 during classification.
- List each legacy name, use it through both query terminals, and drop it
  safely.
- Prove the same non-ASCII or dollar-sign suffix is rejected for new creation
  after removal.
- Retain the immutable released v4 fixture and RFC 010 migration assertions.

### API parity and gates

- Cover synchronous and every enabled async runtime wrapper.
- Verify read-only precedence for create/drop and read-only hint/list behavior.
- Run focused query/index and schema-classifier tests, then workspace tests,
  formatting, clippy with warnings denied, documentation, and the applicable
  RFC 009 source/archive smoke gates.
- Obtain focused independent security review before B-03 or M2 is closed.

No test may construct hostile SQL by interpolating the test input into a setup
batch. Test-only legacy objects use a fixed quoted fixture or bound/catalog-safe
setup so the regression harness does not reproduce the production defect.

## Security considerations

This RFC treats public Rust API input as untrusted regardless of the caller's
current architecture. A downstream application can pass HTTP fields, model
output, configuration, plugin metadata, or database-derived text into these
methods; the library cannot infer a trust boundary from the call site.

The controls are deliberately layered:

1. a narrow grammar limits newly persisted names;
2. catalog authorization limits existing-object operations;
3. standard identifier encoding fixes the parser boundary;
4. single-statement execution removes batch expansion;
5. structural ownership prevents dropping or forcing unrelated indexes; and
6. transactions and postconditions close check/use races for DDL.

None of these controls weakens the requirement to bind SQL values. Conversely,
binding catalog lookup values does not make the later identifier position
bindable.

## Compatibility and rollout

This is a security correction in v0.20.1. The source API signatures remain
stable, but two behaviors intentionally narrow:

- newly created suffixes must satisfy R1; and
- rejected hints return `UnsupportedFeature` instead of RFC 002's documented
  `Database` variant.

Those changes must be prominent in `CHANGELOG.md` and migration/API docs before
the release. Existing valid database objects are not renamed or deleted. No
schema migration or `user_version` change is needed.

Implementation should remain one focused security slice. An optional hostile-
input QA checklist may be added after design acceptance if implementation is
delegated or independent reproduction would materially improve the review
gate; no implementation handoff is required merely for ceremony.

## Alternatives considered

### Quote arbitrary new names without a grammar

Correct quoting prevents injection, but it retains an unnecessarily broad
persistent naming surface and makes documentation, diagnostics, and future
tooling harder. Rejected for new creation. Catalog-authorized legacy operation
retains the minimum compatibility exception.

### Validate without quoting

An ASCII grammar can currently make generated names parse unambiguously, but
quoting is cheap, protects reserved-word/future-parser cases, and is required
for legacy spellings. Rejected as a single-control design.

### Permit only names returned by `list_path_indexes`

That is close to the chosen query policy but does not address creation, drop,
case equivalence, built-in hints, ownership shape, or a schema change between
listing and use. Callers also must not be required to maintain an application-
side allowlist snapshot.

### Add `InvalidIndexName` to the public error enum

This is semantically attractive, but adding a variant to an exhaustive public
enum is a source-compatibility concern for a corrective patch release. The
existing safety-precondition `UnsupportedFeature` variant is used with one
stable message. A future breaking release may introduce a dedicated typed
variant and mark the enum non-exhaustive.

### Remove index hints

Removing the API would eliminate one sink but would break a released feature
and would not fix create/drop. Rejected.

## References

- [SQLite binding values to prepared statements](https://www.sqlite.org/c3ref/bind_blob.html)
- [SQLite keyword and identifier quoting guidance](https://www.sqlite.org/lang_keywords.html)
- [SQLite `INDEXED BY`](https://www.sqlite.org/lang_indexedby.html)
- [SQLite schema introspection PRAGMAs](https://www.sqlite.org/pragma.html#pragma_index_list)
- [SQLite `CREATE INDEX`](https://www.sqlite.org/lang_createindex.html)
- [RFC 002 — Query Index Hints and Explain Plan](../done/002-query-index-hints.md)
- [RFC 010 — Transactional, Payload-Preserving Schema Migrations](../accepted/010-transactional-payload-preserving-schema-migrations.md)

## Acceptance and completion gates

The design may move from Proposed to Accepted only after independent review
confirms the grammar, quoting algorithm, catalog/ownership predicate, released-
name compatibility, error behavior, and RFC 010 boundary, followed by explicit
owner approval.

Implementation may begin only after that transition. B-03 closes only after
the hostile-input matrix and focused independent implementation review are
accepted. M2 additionally requires its combined migration-integrity and SQL-
safety exit gate; accepting RFC 011 alone does not complete M2 or authorize a
release.
