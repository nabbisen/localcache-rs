# RFC 006 — Directory-scoped Query Predicates

| Field    | Value |
|----------|-------|
| Status   | Implemented (v0.18.0) |
| Feature  | *(core, no feature flag)* |
| Touches  | `src/cache/query.rs`, `src/db/repository.rs` |
| Depends on | RFC 002 (Implemented, v0.17.0) — shares the path-listing SQL builder |
| Errata | **Erratum 1** (architect, 2026-10-29): the SQL premise for `path_in_dir` is false, and the corrected SQL is specified at the end of this RFC. Implemented by Phase 24 Q7, in v0.21.6 |

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
then discarded).  RFC 002's `index_hint` reduces the scan cost but not the
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
RFC 002) gain a small WHERE-fragment builder.  All fragments AND-combine:

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
**[Erratum 1, 2026-10-29: false. With SQLite's default settings `LIKE`
ignores ASCII case and is optimized only on a `NOCASE`-collated index, so on
this binary-collation column it uses no index range, and it also matches
case-variant directories. The corrected SQL is under "Erratum 1" at the end.]**

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
  randomized fixture tree. **[Erratum 1: a reference built on `LIKE` shares
  the defect. The reference must be a plain string comparison, and the tree
  must contain case variants. See the end of this RFC.]**

## Security considerations

All user-supplied values (`dir`, glob alternatives) are bound as SQL
parameters; the only string interpolation is the fixed `ESCAPE '\'`
clause and `GLOB` keyword.  No injection surface beyond the existing
`path_like`.

## Open questions

1. **Case sensitivity on Windows.**  Stored paths are canonicalized (and
   thus carry on-disk casing), and both LIKE-with-prefix and GLOB are
   case-sensitive in SQLite by default **[Erratum 1: false for `LIKE`, which
   ignores ASCII case; `GLOB` is case-sensitive]**, while NTFS lookups are not.  Since
   every write path canonicalizes, stored casing is consistent and
   prefix matches against a canonicalized `dir` are exact; this matches
   the crate-wide convention ("always go through the API, which
   canonicalizes").  Document rather than special-case?  (Proposed: yes.)
2. Should `path_in_dir` accept multiple calls (OR semantics over several
   directories)?  Deferred — single-call AND semantics covers the known
   use cases; multi-dir can be a later additive change.

## Correction in RFC 013

[RFC 013](../done/013-panic-free-path-glob-and-cli-text-handling.md)
defines the shared glob dialect precisely as case-sensitive Unicode-scalar
matching, adds bounded brace validation/expansion, and defers fallible query
preparation to `run()`/`dry_run()`. This corrects implementation details
without changing this RFC's shipped `*`, `?`, and `{a,b}` public scope.

## Erratum 1 (architect, 2026-10-29) — `path_in_dir` is exact, and an index range serves it

**Authority.** The owner authorized the fix as a patch on 2026-10-29 (ROADMAP, Phase 24 Q7). This
erratum specifies it. The implementation handoff follows v0.21.5's release; v0.21.5 ships the
behaviour described under "What is wrong".

### What is wrong

Two statements in this RFC are false, and the implementation followed them:

1. "A constant-prefix LIKE is eligible for SQLite's LIKE optimization." SQLite optimizes `LIKE` only
   when the column's collation matches the operator's case rule. `LIKE` ignores ASCII case unless
   `PRAGMA case_sensitive_like` is on, so the index would have to be `NOCASE`. `files.path` is
   binary-collated. The plan for `path_in_dir` is therefore `SEARCH … (namespace=?)`, with no range
   on `path`. That is the cause of the Phase 23 register entry "`path_in_dir` non-recursive cannot
   construct an indexable prefix", and of the roughly 46× growth to 1M entries in
   `docs/src/performance.md`.
2. Open question 1: "both LIKE-with-prefix and GLOB are case-sensitive in SQLite by default". `GLOB`
   is. `LIKE` is not, for ASCII letters.

**Consequence, reproduced through the public API on Linux on 2026-10-29** (a scratch crate against
`24544bc`, `.git-exclude/tmp/arch-probe-2026-10-29/`, mode `case`). With `Docs/a.txt` and `docs/b.txt`
both cached, `query().path_in_dir(<root>/Docs, true)` and `(…, false)` both return both entries.
That breaks Requirement 1 (a non-recursive match is an entry whose parent directory *equals* `dir`)
and the crate's rule that path matching is case-sensitive (`path_glob`; the exact-key rule for deleted
sources in `docs/src/api.md`).

### The corrected SQL

Let `prefix` be the directory string with a trailing `MAIN_SEPARATOR`, as today. Let `n` be its length
in Unicode scalar values, and `upper` the prefix with its last character replaced by the next code
point (`/` becomes `0`, `\` becomes `]`). In `path_filter_clauses`:

| Form | Clauses | Bound parameters, in order |
|---|---|---|
| recursive | `path >= ? AND path < ?` | `prefix`, `upper` |
| non-recursive | `path >= ? AND path < ? AND instr(substr(path, CAST(? AS INTEGER)), ?) = 0` | `prefix`, `upper`, `n + 1` (as text), `sep` |

- **Exact.** The collation is binary, so comparison is byte-wise on UTF-8. The separator is a single
  ASCII byte, so a string lies in `[prefix, upper)` if and only if it starts with `prefix`.
- **The non-recursive clause** keeps a row only if the text after the prefix contains no further
  separator. `substr` and `instr` count characters on TEXT, which is why `n` counts scalar values.
- **No escaping.** Range comparisons treat `%`, `_` and `\` literally, so `escape_like` has no caller
  left in `path_in_dir` and is deleted if nothing else uses it.
- **The SQL text is fixed.** Every value is bound, so the prepared-statement shape does not vary with
  the directory.
- **Indexed.** The plan is `(namespace=? AND path>? AND path<?)` for the covering path listing and for
  the wider candidate queries of RFC 021. `index_hint` keeps working: a `lc_user_*` index on
  `(namespace, path)` serves the same range.
- **`path_like` and `keys(Some(pattern))` stay `LIKE`,** as documented. Their docs gain one sentence:
  ASCII letters match case-insensitively.

**Verified by the architect** (`.git-exclude/tmp/q7-sql-prototype-2026-10-29.py`, log beside it, exit
0, SQLite 3.54.0):
- the current clauses return `/data/DOCS/d.txt`, `/data/Docs/a.txt` and `/data/docs/b.txt` for a
  non-recursive query on `/data/Docs/`; the proposed clauses return only `/data/Docs/a.txt`;
- 142 randomized (separator, directory, recursive) cases equal a plain string reference, for both `/`
  and `\`, with `%`, `_`, `\`, `]`, `é`, case variants, and sibling names that share the directory name
  as a prefix (`dir` and `dir2`);
- the plan shows the index range for both forms, covering and wider.

### What changes for callers

Not breaking. No signature, schema or wire-format change. A patch release.
- **Results change in exactly two cases.** (a) Case-variant directories on a case-sensitive filesystem
  stop being returned. That is the defect. (b) A directory that no longer exists, queried with a
  different ASCII casing from its stored key, no longer matches. `get`, `contains`, `remove` and
  `explain` already require the exact stored spelling after deletion, so this brings `path_in_dir` in
  line with them. An existing directory is canonicalized to its on-disk casing first, so it is
  unaffected on case-insensitive filesystems.
- **Cost moves from the namespace to the subtree.** A non-recursive query on a directory with a very
  large subtree still reads that subtree's rows, then discards the deeper ones. It is never worse than
  today, which reads the namespace.

### Tests (failing before the fix)

1. **Case variants, through the public API.** Cache a file in `Docs/` and one in `docs/`. For both
   `recursive` values, `path_in_dir(<root>/Docs)` returns only the first. This must fail on v0.21.5.
   Self-skip when the filesystem folds case, as the mtime tests self-skip on coarse clocks: create both
   directories and check they are distinct.
2. **A deleted directory.** The exact spelling matches. A different ASCII casing does not.
3. **Randomized equivalence** against a plain string reference, over trees with `%`, `_`, `\`, non-ASCII
   names, case variants, and `dir`/`dir2` siblings. The architect's prototype is the model.
4. **Plan tests.** The path-listing plan (`dry_run`) and the candidate-query plans contain a range on
   `path`, for both forms.
5. Every existing RFC 006 test passes. Only an assertion that matches `LIKE` text in a plan may change.

### Re-measurement

The `path_in_dir` rows of `crates/localcache/benches/scale_profile.rs`, as a paired comparison of two
builds, interleaved, at a matched path length, with `LOCALCACHE_SCALE_QUERY_REPEATS=5` (the harness's
limitation 5) and the untouched rows as controls. Expected: the non-recursive row stops growing with
the namespace, because the result set is 1000 rows at every scale. `docs/src/performance.md` is
rebuilt from one profile session, and its "Namespaces are the main scaling lever" paragraph is
re-read against the new figures.

### Alternatives considered

- **`PRAGMA case_sensitive_like = ON`.** It would make `LIKE` exact and index-eligible, but it is
  connection-wide, so it silently changes `path_like` for every caller, and the pragma is deprecated.
- **`GLOB` with an escaped prefix.** It is case-sensitive and index-eligible, as the literal-prefix
  `path_glob` already shows. It needs `*`, `?` and `[` escaped in the directory, and the bounds stay
  implicit. The explicit range needs no escaping and shows its bounds in the plan.
- **Filtering in Rust after a `LIKE` scan.** That is the over-fetch this RFC exists to remove.

### Rollback

Revert the clause builder. Nothing is stored differently.
