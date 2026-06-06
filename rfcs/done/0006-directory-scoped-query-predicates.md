# RFC 0006 — Directory-scoped Query Predicates

| Field    | Value |
|----------|-------|
| Status   | Implemented (v0.18.0) |
| Feature  | *(core, no feature flag)* |
| Touches  | `src/cache/query.rs`, `src/db/repository.rs` |
| Depends on | RFC 0002 (Implemented, v0.17.0) — shares the path-listing SQL builder |

## Summary

Add two path predicates to `QueryBuilder` that run **inside SQL** rather
than requiring caller-side post-filtering:

1. `path_in_dir(dir, recursive: bool)` — exact directory scoping, with a
   precise non-recursive variant ("this directory only, no subdirectories").
2. `path_glob(pattern)` — glob matching on the stored path, using the same
   pattern dialect as `ScanOptions::glob_pattern` (`*`, `?`, `{a,b}` braces).

Both compose with `path_like`, `index_hint`, `dry_run`, pagination, and
(under `json`) payload predicates.

## Background

Requested by a downstream adopter (arama, RFC 002 Q1) migrating an in-house
media cache to `localcache`.  Their lookup strategies need *exact*
non-recursive directory scoping ("current dir only") and recursive scoping
("current dir and subdirs").  Today the only SQL-side path filter is
`path_like` (SQL `LIKE`), which cannot express "no further separator after
the prefix"; their workaround is `LIKE` + facade post-filter by
`Path::parent()` equality — correct, but it over-fetches on caches sized
for an `Everywhere` strategy (every matching path is loaded, decoded, and
then discarded).  RFC 0002's `index_hint` reduces the scan cost but not the
over-fetch.

The same gap exists for glob matching: the engine ships a glob matcher
(`glob_to_regex`, `expand_braces` in `engine.rs`) for `scan_dir_filtered`,
but `QueryBuilder` cannot use it, so glob-shaped lookups also fall back to
LIKE-plus-post-filter.

## Requirements

- `path_in_dir` non-recursive: matches entries whose stored path's parent
  directory **equals** `dir` — no descendants.
- `path_in_dir` recursive: matches entries anywhere under `dir`.
- `dir` containing SQL LIKE metacharacters (`%`, `_`) or glob
  metacharacters must match **literally** — directory names like
  `100%_done` are common enough to matter.
- `path_glob` uses the `ScanOptions::glob_pattern` dialect (`*`, `?`,
  `{a,b}` brace alternation) for consistency within the crate, not raw
  SQLite `GLOB` syntax (which adds `[...]` classes and lacks braces).
- Predicates AND-combine with each other and with `path_like`.
- The filtering happens in the **path-listing SQL** (`repository::keys`),
  so non-matching entries are never loaded or decoded.
- `dry_run()` reflects the new predicates in its `EXPLAIN QUERY PLAN`
  output; `index_hint` applies as usual.
- No new dependencies; no feature flag.

## Design

### Public API

```rust
impl<'e, T> QueryBuilder<'e, T> {
    /// Restrict to entries whose stored path lives in `dir`.
    ///
    /// `recursive = false` matches only direct children of `dir`;
    /// `recursive = true` matches the entire subtree.
    ///
    /// `dir` is normalized like every other path input: canonicalized when
    /// it exists on disk, used verbatim otherwise (so queries over
    /// since-deleted directories still match their stored entries).
    /// LIKE metacharacters in `dir` are escaped — the match is literal.
    pub fn path_in_dir(self, dir: impl AsRef<Path>, recursive: bool) -> Self;

    /// Restrict to entries whose stored path matches a glob `pattern`.
    ///
    /// Same dialect as [`ScanOptions::glob_pattern`]: `*` (any sequence),
    /// `?` (one character), `{a,b}` (alternation).  Matched against the
    /// full stored path, case-sensitively.
    pub fn path_glob(self, pattern: impl Into<String>) -> Self;
}
```

New fields on `QueryBuilder`:

```rust
pub(crate) path_in_dir: Option<(String, bool)>, // (normalized dir prefix, recursive)
pub(crate) path_glob: Option<String>,
```

### SQL generation — `repository.rs`

`keys()` and `explain_query()` (already sharing their SQL shape since
RFC 0002) gain a small WHERE-fragment builder.  All fragments AND-combine:

```sql
SELECT path FROM files [INDEXED BY …]
WHERE namespace = ?1
  [AND path LIKE ?n]                                  -- path_like (as today)
  [AND path LIKE ?d ESCAPE '\'                        -- path_in_dir prefix
   [AND path NOT LIKE ?dd ESCAPE '\']]                -- non-recursive only
  [AND (path GLOB ?g1 [OR path GLOB ?g2 …])]          -- path_glob
ORDER BY path
```

**`path_in_dir`.**  Let `prefix = escape_like(dir) ++ SEP` where
`escape_like` backslash-escapes `\`, `%`, `_`, and `SEP` is
`std::path::MAIN_SEPARATOR` (paths are stored canonicalized, so separators
are platform-native by construction).  Then:

- recursive: `path LIKE prefix || '%' ESCAPE '\'`
- non-recursive: the above **AND**
  `path NOT LIKE prefix || '%' || SEP || '%' ESCAPE '\'`
  (i.e. no second separator after the prefix — the classic
  "LIKE 'd/%' AND NOT LIKE 'd/%/%'" idiom, with the prefix escaped).

A constant-prefix LIKE is eligible for SQLite's LIKE optimization, so
`path_in_dir` can use the `(namespace, path)` index — including
`lc_user_*` indexes via `index_hint`.

**`path_glob`.**  Reuse `expand_braces` (moved from `engine.rs` to a
shared location, or re-exported `pub(crate)`) to expand `{a,b}`
alternation into N alternatives, then translate each alternative's
`*`/`?` into SQLite `GLOB` syntax — which is a near-identity mapping
(`*` → `*`, `?` → `?`) with one caveat: `[` must be escaped for SQLite
GLOB, since our dialect treats it literally while SQLite GLOB treats it
as a character class.  Emit `(path GLOB ?1 OR path GLOB ?2 …)` —
parameterized, one bind per alternative.

This keeps the user-facing dialect identical to `ScanOptions` while
executing natively in SQLite.  GLOB with a literal prefix also benefits
from the LIKE/GLOB index optimization.

### `dir` normalization

`path_in_dir` normalizes `dir` with the existing `normalize_path`,
falling back to the path as given on `FileNotFound` — the same fallback
`contains()` and `remove()` use, so queries over deleted directories
behave consistently with the rest of the API.

### Async / pool

`AsyncCacheEngine::query_run` and `query_dry_run` take builder closures,
so both predicates work through them with no signature changes.
`ConnectionPool` is unaffected.

## Test plan

- Fixture tree (`tempfile`): `root/{a.txt, b.txt, sub/{c.txt, sub2/d.txt}}`,
  all cached.
- `path_in_dir(root, false)` returns exactly `{a, b}` — excludes `c`, `d`.
- `path_in_dir(root, true)` returns all four.
- `path_in_dir(root/sub, false)` returns exactly `{c}`.
- Directory name containing `%` and `_`: literal match, no wildcard leak.
- Nonexistent directory: empty result, no error.
- `path_glob("*.txt")` matches all; `path_glob("*/sub/*")` matches `c`, `d`;
  brace pattern `"*.{txt,md}"` expands correctly; `[` in a pattern matches
  a literal `[` in a filename.
- Combination: `path_in_dir(root, true)` + `path_glob("c.*")` returns `{c}`.
- `dry_run()` output includes the new clauses; with `index_hint`, the plan
  names the hinted index.
- Equivalence test: results of `path_in_dir(dir, false)` equal the
  LIKE-plus-`Path::parent()`-post-filter reference implementation over a
  randomized fixture tree.

## Security considerations

All user-supplied values (`dir`, glob alternatives) are bound as SQL
parameters; the only string interpolation is the fixed `ESCAPE '\'`
clause and `GLOB` keyword.  No injection surface beyond the existing
`path_like`.

## Open questions

1. **Case sensitivity on Windows.**  Stored paths are canonicalized (and
   thus carry on-disk casing), and both LIKE-with-prefix and GLOB are
   case-sensitive in SQLite by default, while NTFS lookups are not.  Since
   every write path canonicalizes, stored casing is consistent and
   prefix matches against a canonicalized `dir` are exact; this matches
   the crate-wide convention ("always go through the API, which
   canonicalizes").  Document rather than special-case?  (Proposed: yes.)
2. Should `path_in_dir` accept multiple calls (OR semantics over several
   directories)?  Deferred — single-call AND semantics covers the known
   use cases; multi-dir can be a later additive change.
