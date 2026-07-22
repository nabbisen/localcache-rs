# RFC 013 — Panic-free Path, Glob, and CLI Text Handling

| Field | Value |
|---|---|
| Status | Accepted |
| Feature | *(core; CLI parity; no new production feature flag)* |
| Touches | shared glob compiler/matcher, path-key resolution, query preparation, scan walking, CLI text formatting, path/glob documentation and tests |
| Finding | Architect review B-05 and related deleted-path / CLI Unicode findings |
| Milestone | Phase 21 M3 |

## Summary

Replace the current byte-slicing glob implementation with one bounded,
Unicode-scalar-safe compiler shared by directory scans and SQL path queries.
Malformed or excessively expanding brace patterns return a stable existing
error variant instead of panicking, being accepted inconsistently, or reaching
filesystem/database work.

At the same time, make the existing RFC 008 deleted-path promise precise and
safe. Database rows retain their valid UTF-8 stored keys: normal `set` writes
use canonical absolute keys, while imported records may already contain a
portable key. When a source has disappeared, `get`, `contains`, `remove`, and
`explain` may use only an exact UTF-8 key supplied by the caller. The
basename/suffix search is removed because it can delete the wrong entry when
different directories contain the same filename. Freshness APIs retain their
source-aware `None`/`Missing` semantics.

Finally, make CLI path truncation operate on Unicode scalar values rather than
UTF-8 byte offsets and render schema-v5 modification times as nanoseconds, not
seconds. These are corrective patch-release changes. They introduce no schema,
payload, public method signature, production dependency, or release authority.

This RFC is the remaining implementation theme in Phase 21 M3. Accepted status
authorizes implementation under this design. B-05 and M3 remain open until the
implementation and the combined RFC 012/RFC 013 milestone evidence receive one
focused implementation review.

## Motivation

The current scan glob matcher mixes two coordinate systems. It counts Unicode
scalar values with `chars().count()` and `find_question`, then uses those
counts and match positions as UTF-8 byte offsets in string slices. A pattern
containing `?` and `*` can therefore slice a valid filename such as `é` between
its bytes and panic. Brace handling has a second panic: unmatched closing
braces decrement an unsigned nesting depth below zero.

The issue is wider than one helper because RFC 006 promises that
`ScanOptions::glob_pattern` and `QueryBuilder::path_glob` share one dialect.
Today they share an infallible brace expander, but scans use a hand-written Rust
matcher while queries pass expanded strings to SQLite `GLOB`. Malformed braces
are sometimes treated literally, sometimes stripped, and may panic depending
on their position. The non-fallible `QueryBuilder::path_glob` setter also has no
place to return a parse error; validation must occur at `run()` or `dry_run()`.

RFC 008 documents a uniform deleted-file fallback which the implementation
does not provide:

- `get` returns `FileNotFound` before looking in the database;
- `get_if_fresh` and `check_status` report the source as missing;
- `contains` tries only the raw string; and
- `remove` scans stored keys by suffix and basename, allowing an ambiguous
  relative name to select the wrong row.

Normal `set` writes store only the canonical key, not the originally supplied
alias. `import_entries` may instead persist the valid UTF-8
`ExportRecord::path` as its stored key, but it likewise creates no mapping back
to a source alias. After a file and any symlink have disappeared, localcache
cannot reconstruct which relative or aliased spelling produced a key.
Exact-key fallback is the only deterministic behavior that neither guesses
nor requires a schema change.

The CLI contains two related presentation defects. Its suffix truncation
counts characters but slices bytes, so listing a sufficiently long non-ASCII
path can panic. Its `inspect` command passes nanosecond `mtime` values to a
seconds formatter, producing dates millions of years in the future instead of
the actual modification time.

These failures violate the external error-only design, RFC 006's one-dialect
promise, RFC 008's path compatibility guarantee, and Phase 21 M3's requirement
for negative Unicode and property evidence.

## Goals

1. Make every valid UTF-8 glob and candidate string panic-free.
2. Define one exact glob grammar and case policy for scans and queries.
3. Reject malformed or resource-excessive patterns through existing `Result`
   terminals before walking files or starting a database query.
4. Preserve nested and multiple brace expansion within explicit safety bounds.
5. Remove lossy string conversion from database path identity decisions.
6. Make deleted-source behavior deterministic across direct, batch, pool, read
   pool, and async surfaces.
7. Eliminate ambiguous basename/suffix deletion.
8. Make CLI truncation Unicode-scalar-safe for every width, including zero and
   one.
9. Render CLI modification timestamps with schema-v5 nanosecond units.
10. Close B-05 and complete M3 with bounded exhaustive/property-style tests
    and one combined implementation acceptance gate.

## Non-goals

- Changing the SQLite schema, stored path representation, payload format, or
  path canonicalization performed at write time.
- Supporting non-UTF-8 database path keys. SQLite schema v5 stores paths as
  `TEXT`; non-UTF-8 filesystem names return `InvalidPath`.
- Recovering a deleted file's former relative, symlink, or differently cased
  alias. Those aliases are not stored.
- Unicode normalization, locale-aware case folding, grapheme-cluster matching,
  or display-column measurement. Matching is exact by Unicode scalar value;
  CLI truncation counts scalar values rather than terminal cell width.
- Adding glob escapes, character classes, `**` semantics, or platform-specific
  separator rules. RFC 006's `*`, `?`, and brace dialect remains authoritative.
- Changing public method signatures or adding a public error variant.
- Fixing the unused CLI `import --overwrite` option, partial-hash diagnosis,
  async panic/poison handling, watcher failure delivery, or broad module-size
  debt. Those are unrelated to B-05/M3 and remain later work.
- Moving RFC 006 or RFC 008 out of `done/`. Their shipped historical decisions
  remain records; implementation may add a short correction cross-reference.
- Release preparation, version changes, packaging, tagging, publication, or
  hosted release work.

## Terminology

- **Unicode scalar value**: one Rust `char`. A base letter and combining mark
  are two scalar values; no normalization is performed.
- **Stored key**: the exact valid UTF-8 value in `main.files.path`; normally
  the canonical key produced by `set`, but an imported record may supply a
  portable key directly.
- **Canonical key**: the valid UTF-8 absolute path returned by
  `Path::canonicalize` and stored by normal `set` operations.
- **Exact missing-source key**: the caller's valid UTF-8 path string used
  byte-for-byte after canonicalization reports `FileNotFound`; it can match
  either a normal canonical stored key or an imported stored key.
- **Glob compilation**: brace validation/expansion plus conversion to internal
  wildcard tokens and SQLite-safe alternatives.
- **Glob terminal**: `scan_dir_filtered`, `preload`, `QueryBuilder::run`, or
  `QueryBuilder::dry_run`, where an existing `Result` can report invalid input.
- **Candidate**: a valid UTF-8 filename evaluated by a scan glob or a stored
  valid UTF-8 full path evaluated by a query glob.

## Requirements

### R1 — One shared glob grammar

Both `ScanOptions::glob_pattern` and `QueryBuilder::path_glob` use this exact
case-sensitive grammar on every platform:

- `*` matches zero or more Unicode scalar values;
- `?` matches exactly one Unicode scalar value;
- `{a,b}` selects alternatives;
- multiple brace groups form their Cartesian product;
- nested groups are supported;
- commas outside braces are literal;
- `[` and `]` are literal characters, not character-class delimiters; and
- every other Unicode scalar value matches itself exactly.

`*` in a query may match path separators because RFC 006 applies query globs
to the complete stored path. A scan applies the same grammar only to the file
name, so no separator is present in its candidate. Consecutive `*` tokens are
semantically equivalent to one `*`.

There is no normalization or case folding. For example, `é` and `e` followed
by U+0301 are different candidates, and `A` does not match `a` on Windows.
This corrects the current `ScanOptions` documentation, which claims
case-insensitive Windows behavior that the implementation has never provided,
and aligns it with RFC 006 and SQLite `GLOB`'s case-sensitive contract.

Balanced brace groups preserve current accepted behavior, including singleton
groups and empty alternatives. Unmatched `{` or `}` is malformed. NUL is
rejected because filesystem path components cannot contain it and SQLite text
matching treats embedded NUL specially.

### R2 — Bounded, stable error contract

Glob compilation must enforce named internal constants:

- maximum input length: 16,384 UTF-8 bytes;
- maximum brace nesting: 32 levels; and
- maximum expanded alternatives: 256.

These bounds are far above documented ordinary use while preventing recursive
stack exhaustion, Cartesian-product memory growth, and oversized variable SQL
statements. The compiler must check growth before allocation/multiplication and
must never depend on debug overflow checks for safety.

No public error variant or signature is added. Malformed syntax returns:

```text
UnsupportedFeature("invalid glob pattern: malformed brace syntax")
```

NUL or a safety-limit violation returns:

```text
UnsupportedFeature("invalid glob pattern: safety limit exceeded")
```

The messages are stable and do not echo caller input. Existing valid patterns
within the bounds retain their results.

For scans, compilation occurs after the root has been confirmed as a directory
but before `read_dir` or candidate processing. For queries, it occurs before a
SQLite transaction, index authorization, SQL construction, payload loading, or
LRU update. `run()` and `dry_run()` return the same error for the same pattern.

### R3 — Unicode-safe scan matching

Move brace parsing and wildcard matching out of `cache/engine.rs` into one
private logical module such as `cache/glob.rs`, with its unit tests in
`cache/glob/tests.rs`.

The compiler represents each expanded alternative as wildcard/literal tokens.
The matcher operates only on token indices and a collected/iterated sequence
of Unicode scalar values. It must not use a scalar count as a byte offset, form
unchecked string slices, recurse based on candidate length, or call `unwrap`
for a state reachable from public input.

An iterative star-backtracking matcher is sufficient:

```text
pattern cursor, candidate cursor
last-star pattern position, candidate retry position

literal or ? matches -> advance both
* -> remember star and current candidate, advance pattern
mismatch with prior * -> advance retry by one scalar and resume after star
mismatch without prior * -> false
end -> skip remaining stars; match only if pattern is exhausted
```

The compiler also produces SQLite-safe pattern strings from the same expanded
alternatives. Literal `[` becomes `[[]`; wildcard `*`/`?` remains unchanged.
No caller value is interpolated into SQL.

During a filesystem scan, a filename or extension that cannot be represented
as UTF-8 must return `LocalFileCacheError::InvalidPath` with the candidate path.
It must not be converted to `""`, converted lossily, silently skipped, or
allowed to collide with another path.

### R4 — Query terminal validation and parity

`QueryBuilder::path_glob` remains a non-fallible fluent setter for source
compatibility. It stores the raw pattern. The shared compiler runs at both
terminals and supplies the bounded SQLite alternatives only after successful
validation.

`QueryBuilder::path_in_dir` likewise retains its signature but stores the
`PathBuf` until terminal preparation. At the terminal:

- an existing directory is canonicalized;
- `FileNotFound` uses the exact raw directory path;
- other I/O failures propagate rather than being silently treated as a raw
  path; and
- a non-UTF-8 canonical/raw directory returns `InvalidPath` rather than using
  `to_string_lossy`.

One private prepared-path-filter structure should be shared by normal
`run()`, `dry_run()`, `ConnectionPool`, `ReadPool`, and async query helpers so
validation and SQL alternatives cannot drift. The existing parameterized SQL
and RFC 011 index authorization snapshot remain unchanged after preparation.

For every accepted pattern, Rust scan matching and SQLite full-path matching
must agree on wildcard behavior for the same valid UTF-8 candidate after the
documented filename/full-path scope difference is removed in the test setup.

### R5 — Exact deleted-source key resolution

Path identity must never use `to_string_lossy`. A private path-key resolver in
`path.rs` distinguishes:

```text
Existing { canonical_path, canonical_utf8_key }
Missing  { supplied_path, exact_supplied_utf8_key }
```

Canonicalization errors other than `FileNotFound` propagate. Either key form
must be valid UTF-8 or return `InvalidPath`.

The public behavior becomes:

| Method | Existing source | Missing source |
|---|---|---|
| `get` | canonical lookup; return payload on hit | exact supplied-key lookup; return payload on hit, `None` on miss |
| `contains` | canonical lookup | exact supplied-key lookup |
| `remove` | delete canonical key | delete exact supplied key only |
| `explain` | canonical lookup and source diagnostics | exact supplied-key lookup; `file_exists=false` while accurately reporting `entry_exists` |
| `get_if_fresh` | existing freshness behavior | `None` because a missing source cannot be fresh |
| `check_status` | existing freshness behavior | `CacheStatus::Missing` |
| `touch` | existing update behavior | `false`; no missing-source warming |

Batch, pool, read-pool, and async forwarding inherit these outcomes.

Remove the `remove` suffix/basename scan completely. A missing relative path,
former symlink, or basename does not match a stored key unless its UTF-8 string
is exactly that key. Callers needing post-deletion access should retain the
stored path returned in `CacheEntry`, `EntryInfo`, `keys`, or query results.
For normal `set` writes this is the canonical path; imported records retain
their supplied stored key. This is a deliberate safety correction to RFC
008's overly broad “raw path” wording, not a new alias-recovery feature.

`cleanup_missing_files` remains unchanged: it iterates exact stored keys and
tests `Path::exists()` without canonicalizing them.

### R6 — UTF-8 path boundary

Schema v5 stores `main.files.path` as SQLite `TEXT`, so valid UTF-8 is the
durable identity boundary. Every conversion used to query, insert, delete,
scan, or build a directory predicate must be fallible and exact.

`Path::display`/lossy formatting remains permitted only for diagnostics and
human output after identity has been decided. It must not feed a database key,
comparison, glob candidate, or deletion choice.

On Unix, public tests must create a real non-UTF-8 filename and prove direct
storage and scan filtering return `InvalidPath` without database mutation or
panic. Platform-specific inability to create such a filename is handled with
`#[cfg(unix)]`; no unsupported encoding is fabricated on Windows.

### R7 — CLI Unicode and time rendering

Move presentation helpers into a small private CLI module such as
`crates/cli/src/text.rs`, with tests in `text/tests.rs`.

Suffix truncation is defined in Unicode scalar values:

- if `max == 0`, return an empty string;
- if the input contains at most `max` scalars, return it unchanged;
- if `max == 1`, return only `…`; and
- otherwise return `…` plus the final `max - 1` scalars.

It must not index a string by a computed byte offset. This definition preserves
the current suffix-oriented list display without adding a terminal-width
dependency or claiming grapheme/display-cell accuracy.

Time formatting must distinguish Unix seconds from Unix nanoseconds. Schema-v5
`FileMetadata::mtime` and `MetadataDiff::{stored_mtime,current_mtime}` use a
nanosecond formatter; `updated_at`, `last_accessed_at`, and watcher event time
continue to use seconds.

The seconds formatter must be total over the complete `i64` domain, including
values supplied by imported rows. It splits days and time-of-day with signed
Euclidean division and performs civil-date arithmetic in a sufficiently wide
signed intermediate. Years use the proleptic Gregorian calendar and an
untruncated signed decimal representation, padded to at least four digits for
the ordinary `0000..=9999` range. No seconds input may panic, wrap, or be cast
to an unsigned timestamp.

The nanosecond formatter uses Euclidean division by 1,000,000,000 and renders
UTC with nine fractional digits:

```text
YYYY-MM-DD HH:MM:SS.nnnnnnnnn
```

The shared civil-date calculation must accept both the complete `i64` seconds
domain and the complete signed range reachable from an `i64` nanosecond
timestamp. No intermediate may overflow or cast a negative timestamp to an
unsigned value. CLI documentation examples must show the fractional
modification-time form.

### R8 — Compatibility and documentation

No public Rust signature changes. Adding stricter malformed-pattern rejection
and removing ambiguous suffix deletion are intentional corrective behavior.

Update:

- `ScanOptions` and `QueryBuilder::path_glob` Rust documentation;
- the API path contract and error guide;
- glob examples in the portability/query documentation;
- CLI `list`/`inspect` documentation;
- `CHANGELOG.md` under `Unreleased`; and
- short correction notes in RFC 006 and RFC 008 pointing to RFC 013 without
  rewriting their shipped historical design.

Documentation must say “Unicode scalar value,” “case-sensitive on every
platform,” “exact stored key after deletion,” and “valid UTF-8 database key.”
It must not claim grapheme, normalization, arbitrary alias recovery, or
non-UTF-8 storage support.

### R9 — Lifecycle and M3 gate

Implementation requires prior independent design acceptance and explicit
owner authorization. RFC 013 remains under `rfcs/accepted/` after implementation
acceptance and moves to `done/` only when shipped under RFC 000.

One focused implementation review is sufficient absent material corrective
findings. That review must evaluate both RFC 013 and the combined M3 exit gate:

- RFC 012's read-only mutation boundary remains accepted;
- B-05's panic paths are closed;
- deleted-path behavior matches the clarified contract;
- Unicode/bounded exhaustive properties pass; and
- public documentation agrees.

Acceptance may close B-05 and mark M3 complete. It does not authorize M4,
release housekeeping, a release candidate, tag, push, publication, hosted
release, or movement of RFC 012/013 to `done/`.

## Detailed design

### Private module boundaries

The intended structure is:

```text
crates/localcache/src/cache/glob.rs
crates/localcache/src/cache/glob/tests.rs
    parser, bounds, expansion, token matcher, SQLite translation

crates/localcache/src/path.rs
crates/localcache/src/path/tests.rs
    exact UTF-8 key resolution and platform-specific invalid-path cases

crates/cli/src/text.rs
crates/cli/src/text/tests.rs
    scalar-safe truncation and signed seconds/nanoseconds formatting
```

Integration tests remain responsible for public cross-surface behavior. This
is a risk-reducing extraction from oversized engine/CLI files, not a general
module reorganization.

### Brace compiler

Brace syntax is parsed with `char_indices` or another boundary-aware cursor.
Every closing brace checks a nonzero depth before decrement. Every opening
brace checks the depth limit before descent. Expansion count is accumulated
with checked arithmetic and compared with the alternative limit before output
allocation.

Parser recursion may follow syntactic brace nesting only and is constrained by
the explicit nesting bound. Expansion across sequential groups must be
iterative; it must not add one call-stack frame per group, because a long
sequence of singleton groups can stay within the alternative limit. The
compiler returns one owned parsed object containing both matcher tokens and
SQLite alternatives so callers do not parse twice.

### SQL translation

SQLite documents `GLOB` as case-sensitive and its implementation consumes
UTF-8 characters for `?`. SQLite also documents quadratic behavior for
pathological patterns and a configurable pattern-length limit. RFC 013's
smaller public bounds and alternative cap provide a product-level limit before
the database is involved.

Each translated alternative remains a bind parameter:

```sql
AND (path GLOB ? OR path GLOB ? ...)
```

Literal `[` is encoded as `[[]` because RFC 006 deliberately excludes SQLite
character classes. No pattern content enters SQL syntax.

### Error precedence

For `scan_dir_filtered`, a nonexistent/non-directory root retains its existing
I/O error before glob compilation. Once the root is valid, an invalid glob
precedes directory iteration and candidate-specific errors.

For query `run`/`dry_run`, path/glob preparation precedes database work.
Payload predicates, pagination, and sorting do not change malformed-pattern
errors. If both `path_in_dir` and `path_glob` are invalid, path filters are
prepared in method-field order: directory resolution first, glob compilation
second. This ordering is private but deterministic and tested.

For read-only engines, RFC 012 affects mutating methods only; these read/query
validation errors are unchanged by database authority.

### Deleted-path examples

Exact canonical access succeeds:

```rust
let canonical = file.canonicalize()?;
engine.set(&canonical, &payload)?;
std::fs::remove_file(&canonical)?;

assert!(engine.get(&canonical)?.is_some());
assert!(engine.contains(&canonical)?);
assert_eq!(engine.check_status(&canonical)?, CacheStatus::Missing);
assert!(engine.remove(&canonical)?);
```

Ambiguous aliases do not guess:

```text
/a/report.txt  (stored, source deleted)
/b/report.txt  (stored, source deleted)

remove("report.txt") -> false
```

Neither row is removed. The caller must pass one complete stored key.

## Test plan

### Compiler and matcher

- Exact tests for empty patterns; only `*`; only `?`; consecutive stars;
  Unicode literals; composed/decomposed accents; emoji; CJK; combining marks;
  literal brackets; commas; singleton/empty/nested/multiple brace groups.
- Exact malformed tests for leading/unmatched `}`, unmatched `{`, mixed nested
  imbalance, NUL, depth 33, 16,385 bytes, and 257 expanded alternatives.
- Boundary acceptance at exactly 16,384 bytes, depth 32, and 256 alternatives.
- Prove every rejection returns one of the two exact stable messages without
  input echo.
- Bounded exhaustive/property-style generation over an alphabet containing
  ASCII literals, `*`, `?`, braces, comma, `é`, a combining mark, CJK, and an
  emoji. Every generated string is compiled under `catch_unwind`; success is
  compared with a simple reference matcher and error is one stable variant.
  No external property-test dependency is required.
- Long-candidate and many-star cases establish bounded completion without
  recursion or panic.

### Scan/query equivalence

- Create a valid UTF-8 filename corpus with ASCII, composed/decomposed accent,
  emoji, CJK, brackets, and mixed wildcard positions.
- For every accepted pattern corpus item, compare scan filename results with
  query full-path results using an equivalent prefixed pattern.
- Assert `?` matches one scalar (emoji included), not one byte and not one
  grapheme cluster.
- Assert case-sensitive parity on all platforms.
- Assert malformed and over-limit patterns return the same exact error from
  scan, `run`, `dry_run`, `ConnectionPool`, `ReadPool`, Tokio, async-std, and
  smol thin wrappers where enabled.
- Prove invalid query patterns start no mutation/LRU update and invalid scan
  patterns invoke no preload factory.
- Retain RFC 006 path filters, hint authorization, dry-run, and brace-result
  regressions.

### Path behavior

- Existing absolute, relative, symlink, and case-insensitive-platform aliases
  retain canonical lookup behavior while the source exists.
- After deletion, exact canonical-key `get`, `contains`, `explain`, and
  `remove` succeed as specified; `get_if_fresh` is `None`, `check_status` is
  `Missing`, and `touch` is `false`.
- Batch, connection-pool, read-pool, and enabled async results match direct
  behavior.
- Two deleted files with the same basename prove relative/basename removal
  deletes neither and exact removal deletes only the selected key.
- A former symlink alias and relative alias do not guess after deletion.
- Unix non-UTF-8 direct set/get and scan candidates return `InvalidPath`
  without database changes or panic.
- `path_in_dir` propagates non-`NotFound` canonicalization errors and returns
  `InvalidPath` for a non-UTF-8 exact/canonical path at both terminals.
- RFC 008 fixture/wire tests and `cleanup_missing_files` semantics remain green.

### CLI

- Unit table for truncation at widths 0, 1, exact, and shorter-than-input over
  ASCII, `é`, decomposed accents, CJK, and emoji; output always contains at
  most `max` scalars and valid UTF-8.
- Bounded exhaustive truncation inputs prove no panic and exact suffix
  preservation.
- CLI `list` over a stored path longer than 55 scalars with non-ASCII near the
  truncation boundary exits successfully and prints the expected suffix.
- Fixed timestamp vectors cover epoch, subsecond, pre-epoch, a normal modern
  timestamp, both `i64` seconds endpoints, and both `i64` nanosecond
  endpoints. Assertions cover exact output as well as absence of panic.
- CLI `inspect` over a fixed database row prints a realistic UTC nanosecond
  modification timestamp rather than interpreting nanoseconds as seconds.
- CLI `scan --glob` returns the stable malformed-pattern error without panic.

### Regression gates

- Default and all-feature package/workspace tests and all targets.
- Tokio plus focused async-std and smol path/glob parity.
- Warnings-denied all-target/all-feature clippy.
- Warnings-denied rustdoc, mdBook, formatting, and source-integrity checks.
- Existing RFC 010 migration/classifier and RFC 011 index/query evidence.
- Existing RFC 012 read-only contract and CLI authority evidence.
- One focused independent implementation/M3 review before B-05 or M3 closes.

## Security considerations

Caller-supplied glob patterns are untrusted resource-control inputs even though
they are SQL bind values. Native SQLite limits SQL injection, but it does not
prevent application-side expansion blowups or expensive pattern matching.
Length, depth, and alternative bounds prevent the known memory/stack risks
before filesystem/database work. Bound checks use checked arithmetic.

String slicing at unverified byte offsets is forbidden. All matching uses
Unicode scalar values owned by valid Rust strings. Non-UTF-8 filesystem names
fail explicitly rather than being collapsed through lossy conversion.

Deleted-path removal is an integrity boundary. Suffix/basename guessing can
delete a row the caller did not identify. Exact stored-key deletion prevents
that confused-deputy behavior while preserving deterministic cleanup.

CLI formatting handles database-derived/user-visible strings without panics.
It does not add terminal control-sequence sanitization; that separate display
policy is not implicated by B-05 and would require its own compatibility
decision.

## Compatibility and rollout

The public method signatures, error enum, schema, and payload encoding remain
unchanged. Valid documented patterns continue to work. Behavior changes are:

- valid Unicode patterns stop panicking;
- unmatched braces and excessive patterns return `UnsupportedFeature`;
- scan matching is explicitly case-sensitive on every platform, matching its
  actual prior behavior and RFC 006;
- non-UTF-8 scan candidates return `InvalidPath` instead of being treated as
  an empty/lossy string;
- `get` can retrieve a deleted-source entry by exact stored key;
- `remove` no longer guesses by basename/suffix;
- query directory errors are no longer silently swallowed;
- CLI list truncation is Unicode-scalar-safe; and
- CLI inspect renders `mtime` in nanoseconds correctly.

These are appropriate for v0.20.1 because they repair documented safety and
compatibility contracts without a storage migration or source-signature
change. The changes must be called out under `Unreleased` and in the path/glob
guides.

No implementation handoff is proposed. The private module boundaries,
algorithms, limits, error messages, compatibility table, and test matrix above
are sufficient for one implementer. Add a handoff only if implementation is
delegated or property-test work is split across developers.

## Alternatives considered

### Add a third-party glob crate

A mature implementation could eliminate custom matcher risk, but common glob
crates add escape, character-class, `**`, separator, or platform behaviors
which do not match RFC 006. Adapting and constraining them would still require
a parser and equivalence layer. A small token matcher for only `*` and `?`
keeps the established dialect and adds no production dependency.

### Change `path_glob` to return `Result<QueryBuilder, _>`

This would report errors at the setter but break fluent source compatibility
in a corrective patch. Deferred terminal validation preserves the public API
and aligns with query index-hint validation, which also occurs at terminals.

### Treat unmatched braces as literals

This preserves some accidental behavior but leaves malformed input ambiguous
between scan and query implementations. RFC 006 documents braces as syntax;
balanced syntax plus structured errors is clearer and testable.

### Add a new `InvalidGlob` error variant

The enum is publicly matchable and not `non_exhaustive`; adding a variant in a
patch can break exhaustive downstream matches. The existing
`UnsupportedFeature(String)` pattern is already used for stable public input
safety errors and avoids an enum change.

### Normalize Unicode or match grapheme clusters

Filesystem and SQLite keys are exact strings. Normalization/case folding could
make distinct Unix filenames collide and would not match SQLite `GLOB` without
a custom SQL function. Unicode scalar matching fixes the panic while retaining
identity semantics.

### Preserve basename/suffix removal

It is convenient for a relative path after deletion, but cannot disambiguate
two stored canonical keys with the same basename. Returning `false` for a
non-exact key is safer than deleting an arbitrary row. A schema storing aliases
would be a separate feature and migration.

### Add a terminal-width dependency for CLI truncation

Display-cell width would improve alignment for some scripts and terminals but
does not solve path identity or glob safety and introduces locale/emoji width
policy. Scalar-safe truncation is the bounded corrective change.

## References

- [RFC 006 — Directory-scoped Query Predicates](../done/006-directory-scoped-query-predicates.md)
- [RFC 008 — Compatibility Guarantees](../done/008-compatibility-guarantees.md)
- [RFC 012 — Read-only Schema and Mutation Contract](../accepted/012-read-only-schema-and-mutation-contract.md)
- Originating reviews:
  `.git-exclude/reviewed/architect-preparation-review-2026-07-17.md` and
  `.git-exclude/reviewed/architect-preparation-review-2026-07-18.md`
- SQLite [GLOB expression](https://www.sqlite.org/lang_expr.html#like)
- SQLite [`sqlite3_strglob`](https://www.sqlite.org/c3ref/strglob.html)
- SQLite [LIKE/GLOB pattern limits](https://www.sqlite.org/limits.html#max_like_pattern_length)

## Design review and acceptance gates

The independent design review at
`.git-exclude/reviewed/architect-rfc-013-design-review-2026-07-22.md` returned
**Accept with notes** and no blocking findings. The owner explicitly approved
the Proposed-to-Accepted transition on 2026-07-22.

Both non-blocking notes are incorporated above: general path statements
distinguish exact stored keys from the canonical keys normally produced by
`set`, preserving imported portable keys, and the CLI seconds formatter plus
its tests cover the complete signed `i64` domain. No separate handoff or
further design review is required unless implementation is delegated or the
design changes materially.

B-05 closes only after the implementation matrix and focused independent
implementation review are accepted. That review also evaluates the combined
RFC 012/RFC 013 evidence before M3 may complete. Design acceptance authorizes
neither release work nor a release lifecycle transition.
