# Q3b Handoff — Seven Purpose-Named Variants; `UnsupportedFeature` and `PayloadVersionMismatch` Removed (RFC 025 R3–R5, R10, Amendment 1)

RFC: `rfcs/accepted/025-error-taxonomy-and-input-validation.md`, Part B, **R3, R4, R5, R10** and **Amendment 1**.
Scope: authorized by the owner on 2026-10-29 (ROADMAP, Phase 24, "v0.22.0 scope"). Order: Q9 ✅ → Q3d ✅ → Q1c →
**this slice (Q3b)** → Q3c → Q2f → Q3e → Q10 → release preparation. Start when Q1c is committed locally.
Slices stay in **local commits**; do not push (the book deploys from `main`).
Format of the review request: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6.
Authorship: high-capability model. Authorized: the owner.

## 0. What this is

v0.22.0 is breaking. This slice changes the *type* of every error that is today `UnsupportedFeature(String)`,
so a caller can act on a condition without parsing English, and deletes the catch-all. **It rejects nothing
new and changes no behaviour except the variant and the message wording stated here**: which inputs succeed
and which fail is identical before and after (the announced rejections are Q3c). Two principles: *"Finally
clean, safe and secure, robust and sophisticated design"* and *public material must not confuse or mislead
users*. If a site or a name is not covered here, **stop and file a design request**.

## 1. The variants (R3), exactly

Add to `LocalFileCacheError` (`crates/localcache/src/error.rs`), after the existing variants, with rustdoc
that says what the condition is and what the caller can do:

```rust
#[error("invalid configuration: {option}: {reason}")]
InvalidConfiguration { option: &'static str, reason: String },

#[error("invalid argument: {argument}: {reason}")]
InvalidArgument { argument: &'static str, reason: String },

#[error("database schema is not current; a read-only open cannot migrate it; database was not modified")]
SchemaNotCurrent,

#[error("incompatible database schema: {0}")]
IncompatibleSchema(String),

#[cfg(feature = "encryption")]
#[error("no encryption key is configured")]
MissingEncryptionKey,

#[error("database state: {0}")]
DatabaseState(String),

#[cfg(feature = "watching")]
#[error("file watching failed: {0}")]
Watcher(String),
```

Remove `UnsupportedFeature` and `PayloadVersionMismatch` (R5). The enum stays `#[non_exhaustive]`.

**The name sets are fixed (R3).** Q3b uses only these names:
`option` ∈ {`database_path`, `read_only`, `encryption_key`, `size`}; `argument` ∈ {`new_key`, `path_glob`,
`index_name`, `records`}. (`max_entries`, `ttl` and `items` are Q3c's; do not add them now, they would be
dead code.) To make the set a compile-time fact, inside the library construct these two variants only through
two crate-private helpers taking a **crate-private enum** of the allowed names (`ConfigOption`, `ArgumentName`,
each with `as_str() -> &'static str`):

```rust
pub(crate) fn invalid_configuration(option: ConfigOption, reason: impl Into<String>) -> LocalFileCacheError
pub(crate) fn invalid_argument(argument: ArgumentName, reason: impl Into<String>) -> LocalFileCacheError
```

A new name then needs a code change in one place, which is the "needs a review" rule made mechanical. The two
CLI sites (below) cannot use a crate-private type; they write the literal `"records"` in the public field.

## 2. The mapping (R4), site by site

Re-run `git grep -n "UnsupportedFeature" -- crates ':!*tests*'` first and **report the list**. At `d4db76f` it
is the 29 library sites below plus 2 constructions and 1 match in the CLI. A site that is not in this table is
a design request, not a guess.

| Site (file, near line) | New variant |
|---|---|
| `cache/engine.rs:1352` read-only + in-memory | `invalid_configuration(ReadOnly, …)` |
| `cache/engine.rs:1363` encryption key length at open | `invalid_configuration(EncryptionKey, …)` |
| `read_pool.rs:114` size zero | `invalid_configuration(Size, …)` |
| `read_pool.rs:119` `:memory:` | `invalid_configuration(DatabasePath, …)`; the reason names **`SyncCacheEngine`** (not `ConnectionPool`; that type is removed in Q3e) |
| `cache/engine.rs:796` `rotate_encryption_key` key length | `invalid_argument(NewKey, …)` |
| `cache/glob.rs:251`, `:255` malformed / safety limit | `invalid_argument(PathGlob, …)` |
| `db/indexes.rs:313` identifier | `invalid_argument(IndexName, …)` |
| `cache/engine/portable.rs:65` base64 | `invalid_argument(Records, …)` |
| `cache/engine.rs:790` rotation without a key; `serialization.rs:125` decode of an encrypted entry without a key | `MissingEncryptionKey` |
| `db/schema.rs:61` read-only open of a non-current schema | `SchemaNotCurrent` |
| `db/schema/classifier.rs:644` unrecognized schema | `IncompatibleSchema` |
| `db/schema.rs:44`, `:173`, `:202`; `db/schema/configuration.rs:11`, `:19`, `:140`; `db/schema/migration.rs:542` | `DatabaseState` |
| `cache/watcher.rs` the eight (`:212`, `:275`, `:285`, `:316`, `:326`, `:523`, `:584`, `:597`) | `Watcher` |
| `serialization.rs:101` `encode_payload`'s unreachable tag | `Serialization` (an internal invariant, not a caller condition) |
| CLI `commands/read.rs:186` `export` JSON failure | `Serialization("json serialisation: …")` |
| CLI `commands/write.rs:72` `import` record line does not parse | `InvalidArgument { argument: "records", reason: "json parse error at line N: …" }` |
| CLI `commands/write.rs:198` `open_copy_source` | **a match, not a construction**: match `LocalFileCacheError::SchemaNotCurrent` instead of the message prefix; delete the `SCHEMA_NOT_CURRENT` constant and its comment |

**Messages (read this carefully).**
- The `Display` of each new variant is the one in § 1. Each site's string becomes the variant's payload
  (`reason`, or the `String`). **The `Display` must read once, not twice**: for example the old
  `unrecognized database schema at physical version N: …` becomes the payload of `IncompatibleSchema`, so
  the payload must not repeat "unrecognized database schema"; the old watcher strings `failed to create
  watcher: …` become the payload of `Watcher` without a second "failed". Give the final text for every site
  in a table in the request (old message, new `Display`).
- **Keep the load-bearing clauses verbatim:** "database was not modified" and "transaction will be rolled
  back" (R4). `SchemaNotCurrent` carries them in its fixed text.
- Exit statuses of the CLI do not change; only the stderr prefix does (`unsupported feature:` →
  `serialization error:` / `invalid argument: records: …`). Check how the CLI maps an error to an exit status;
  if it matches the removed variant anywhere, that is a finding.

**`#[cfg]`:** `MissingEncryptionKey` exists only with `encryption`; `Watcher` only with `watching`. The sites
that construct them are already inside those cfgs; the build with each feature off and with all on must
compile (`cargo make gate-features` covers the rows).

## 3. R10 — `source()`

Nothing to add. `Io` and `Database` already carry sources; `Watcher` and `DatabaseState` carry strings because
their origins are `notify` errors and ad-hoc checks, and making `notify::Error` a public source would expose a
dependency type. Confirm in the request that you added no `#[source]` anywhere else.

## 4. Tests

1. **One test per inventory row** asserting the new variant, with `option` or `argument` where it has one.
   Build them from the existing tests that assert `UnsupportedFeature` (the files that mention it:
   `engine/tests.rs`, `glob/tests.rs`, `indexes/tests.rs`, `schema/tests/atomicity.rs`,
   `tests/codec_lru.rs`, `tests/query.rs`, `tests/read_only_contract.rs`, `tests/read_pool.rs`,
   `tests/rfc013_input_safety.rs`, and the CLI's tests). Where an existing test pins message text that carries
   a load-bearing clause, keep that assertion. Report the list: test, old assertion, new assertion.
2. **The gate (R5, widened by Amendment 1):** `git grep -n "UnsupportedFeature\|PayloadVersionMismatch" --
   crates docs README.md` returns **nothing**. Paste the output and its exit code. (`CHANGELOG.md` and
   `ROADMAP.md` keep their history; `rfcs/` is a record.)
3. **The CLI:** `copy --upgrade-source` still works on an older-schema database (the existing CLI test, now
   exercising `SchemaNotCurrent`), and a source that is not a localcache database is **not** reported as
   "schema not current" (it is `IncompatibleSchema`). The two former `UnsupportedFeature` constructions
   produce the variants above; add a CLI test for the `import` line error if none exists, asserting the
   `invalid argument: records` text and the unchanged exit status.
4. **Feature combinations:** the `encryption`-off and `watching`-off builds still compile and their tests pass
   (the rows of `cargo make gate-features`).
5. **The `compile_fail,E0004` block at the top of `error.rs`** matches variants that exist today
   (`UnsupportedFeature(_)`, `PayloadVersionMismatch { .. }`). Rewrite it with variants that exist in **every**
   feature combination (for example `InvalidConfiguration { .. }`, `SchemaNotCurrent`), so that it still fails
   to compile *for the right reason* (E0004, the enum being `#[non_exhaustive]`), not E0599. Show the
   doctest passing.
6. Every other existing test passes; the compat fixtures pass **unmodified**.

## 5. Documentation and CHANGELOG

- `docs/src/errors.md`: the variant table (remove two rows, add seven, with "what you can do" for each), the
  `PayloadVersionMismatch` section, and every paragraph that names the removed variants (lines near 33–45,
  101, 126, 144, 173 at `d4db76f`), rewritten for what is true. Also `docs/src/querying.md:273`,
  `docs/src/migration.md:160` and `:181`, `docs/src/portability.md:144`. **Re-read each page in full** and fix
  anything else that is no longer true; say what you fixed. `docs/src/roadmap.md:89` is history, leave it.
- Rustdoc: every `[`LocalFileCacheError::UnsupportedFeature`]` link in doc comments (about a dozen) is updated
  to the variant the function now returns, and each function's "Errors" text says which. A caller reading the
  rustdoc of `rotate_encryption_key` must see `MissingEncryptionKey` and `InvalidArgument { argument: "new_key" }`.
- `CHANGELOG.md`, under `## [Unreleased]` / `### Changed`, a **Breaking** entry: `UnsupportedFeature` and
  `PayloadVersionMismatch` are removed; seven variants added; **a migration table** (old condition → new
  variant → what to match), including how to replace a match on the old message text; the CLI stderr prefix
  change; `Display` of schema, state and watcher errors reworded. Keep Q3d's and Q1c's entries.

## 6. Gates and the request

Same set as Q3d's handoff § 4: `cargo fmt --all --check`; `cargo clippy --workspace --all-targets
--all-features --locked -- -D warnings` (the `unwrap_used` lint is on); `cargo make gate-features`;
`cargo test --workspace --all-features --locked` (report the count; 542 before); `RUSTDOCFLAGS="-D warnings"
cargo doc --workspace --all-features --no-deps --locked`; `cargo make gate-versions`; `rustup run 1.85.0
cargo make gate-msrv`; `cargo make gate-deny`; `mdbook build docs`; compat fixtures unmodified.
File `.git-exclude/review-request/NNN-dev-q3b-…-<date>.md` (next free number) describing the **uncommitted**
tree; commit locally only after approval; do not push, tag, bump the version or touch `ROADMAP.md`. List every
required step, including those that pass, and every judgement call.

## 7. Not yours

The announced rejections `max_entries(0)`, TTL under one second, oversized batches/imports (Q3c); RFC 024 B1–B3
(Q2f); the removals of RFC 024 B4 / `ConnectionPool` (Q3e; note that Q3b already names `SyncCacheEngine` in one
reason string); the bounded-decode error (Q10); any behaviour change; `rusqlite`; MSRV.
