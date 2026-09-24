# RFC 025 — Error Taxonomy and Input Validation: Errors Callers Can Act On

| Field | Value |
|---|---|
| Status | Accepted (owner, 2026-09-24), with all four requested decisions as recommended: `UnsupportedFeature` removed; `PayloadVersionMismatch` removed; opaque `DatabaseError` in v0.22.0; Part A (TTL fixes) in v0.21.5 |
| Feature | *(core; `encryption`, `watching` for their variants)* |
| Touches | `crates/localcache/src/error.rs` and every site in the inventory below; `crates/localcache/src/cache/engine.rs` (`is_expired`), `crates/localcache/src/cache/engine/diagnose.rs`, `crates/localcache/src/cache/builder.rs`, `crates/localcache/src/cache/options.rs`, `crates/localcache/src/read_pool.rs`, the wrappers' `open`, `crates/cli/src/commands/write.rs`, tests, `docs/src/errors.md`, `CHANGELOG.md` |
| Finding | Phase 24 plan, milestone Q3; RFC 018 R6 (the split it deferred); RFC 023 R8.3; RFC 024 B3/B4; the Phase 24 register; **two TTL defects reproduced 2026-09-24** |
| Milestone | Phase 24 Q3 |
| Breaking | **Part A (v0.21.5): no**, two correctness fixes. **Part B (v0.22.0): yes**: variant changes, newly rejected input, removals |
| Authorship | High-capability model; **reviewed by the owner** (arrangement of 2026-08-01) |
| Handoffs | [`../handoffs/025-error-taxonomy-and-input-validation/`](../handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md): the Part A (Q3a) handoff and QA checklist. Part B's handoff follows when v0.21.5 ships |

## Summary

An error is useful when a caller can tell **what went wrong and what to do**. Today
`LocalFileCacheError::UnsupportedFeature(String)` carries **29** unrelated production failures:
- a bad key length;
- a malformed glob;
- an old schema opened read-only;
- a file-watcher OS error;
- a migration precondition;
- and more.

RFC 018 named this "a variant that means five unrelated things means nothing". It fixed poisoning
and JSON, and deferred the rest, having made the enum `#[non_exhaustive]` so that the split would
be additive. This RFC completes it. It also settles the reserved `PayloadVersionMismatch`, and adds
the input validation the patch line announced.

While measuring the TTL rules, the architect found and reproduced **two correctness defects**:
- **a clock stepped back by one second expires every entry;**
- `explain()` reports `ttl_remaining_secs: Some(0)` for an entry it calls `Fresh`.

They are fixable without breaking anything, so they are **Part A**, in v0.21.5.

- **Part A (v0.21.5):** the two TTL fixes, under one expiry rule.
- **Part B (v0.22.0):**
  - seven purpose-named variants replace `UnsupportedFeature`, which is removed;
  - `PayloadVersionMismatch` is removed;
  - `max_entries(0)`, a TTL under one second, and an over-bound `batch_set` or import are
    rejected;
  - `Database` stops exposing `rusqlite::Error`;
  - RFC 024's removals are executed.

## Motivation

### The inventory: every production `UnsupportedFeature` site

From `git grep "UnsupportedFeature("` over `crates/localcache/src`, excluding tests, at `c9fc5ff`:

| Site | Message (abridged) | What it is |
|---|---|---|
| `cache/engine.rs` `open` | read-only mode does not support in-memory databases | invalid **configuration** |
| `cache/engine.rs` `open` | encryption key must be exactly 32 bytes | invalid **configuration** |
| `read_pool.rs` `open` | ReadPool size must be >= 1 | invalid **configuration** |
| `read_pool.rs` `open` | ReadPool does not support ':memory:' | invalid **configuration** |
| `cache/engine.rs` `rotate_encryption_key` | new encryption key must be exactly 32 bytes | invalid **argument** |
| `cache/glob.rs` (×2) | invalid glob pattern: malformed brace syntax / safety limit exceeded | invalid **argument** |
| `db/indexes.rs` | SQLite index identifier is invalid or not allowed | invalid **argument** |
| `cache/engine/portable.rs` `import_entries` | base64 decode error for '…' | invalid **argument** (a malformed record) |
| `cache/engine.rs` `rotate_encryption_key` | requires an existing encryption key | **missing key** |
| `serialization.rs` `decode_payload` | entry is encrypted but no encryption key was provided | **missing key** |
| `db/schema.rs` read-only open | requires the current database schema; … not modified | **schema not current**. The CLI now matches this by message text (Q2e) |
| `db/schema/classifier.rs` | unrecognized database schema at physical version …; not modified | **incompatible schema** (newer or foreign) |
| `db/schema.rs` (×3) | query-only / foreign-key enforcement could not be enabled; database changed during validation | **database state** |
| `db/schema/configuration.rs` (×3) | migration requires a rollback-capable/WAL journal; requires `synchronous=FULL`; runtime configuration failed | **database state** |
| `db/schema/migration.rs` | migration precondition or postcondition failed | **database state** |
| `cache/watcher.rs` (×8) | failed to create watcher; watch/unwatch/watch_dir/unwatch_dir failed | **file watching** (an OS error) |
| `serialization.rs` `encode_payload` | no encrypted tag for base tag '…' | **internal invariant**: the encoder produces only the tags matched above |

A caller who wants to retry a busy database, fix a configuration, or tell "old schema" from
"foreign file" must parse English today. The CLI's `copy --upgrade-source` hint already does
exactly that, with a comment admitting it.

### The reserved variant

`PayloadVersionMismatch` has been "reserved, and not currently returned by any operation" since it
was documented. A version mismatch is a **freshness** outcome: `check_status` returns `Stale`, and
`get_if_fresh` returns `None`. It is not an error. A variant that can never occur misleads anyone
who writes a match arm for it.

### Two TTL defects (reproduced)

`is_expired` (`crates/localcache/src/cache/engine.rs`) computes
`now.saturating_sub(updated_at) as u64 >= ttl.as_secs()`. `explain`
(`crates/localcache/src/cache/engine/diagnose.rs`) computes `ttl.as_secs() as i64` separately.

Reproduced end to end against the crate, with a scratch consumer
(`.git-exclude/tmp/rfc025-ttl/e2e/`, `run.log` with its exit code). The arithmetic is also checked
alone (`.git-exclude/tmp/rfc025-ttl/out.txt`).

| Case | Observed | Expected |
|---|---|---|
| TTL 3600 s; the clock steps back 1 s after the write (stored `updated_at` = now + 1) | `get_if_fresh` → `None`; `check_status` → `Stale`. The negative age wraps to about 1.8×10¹⁹ as `u64` | fresh: the entry is one second old at most |
| The same entry: `explain()` and `cleanup_expired()` *(verified at acceptance, `run2.log`)* | `explain` reports `status: Stale` **and** `ttl_remaining_secs: Some(3601)` (a self-contradiction); `cleanup_expired` **deletes** it | fresh, 3600 s remaining, and not deleted |
| TTL `Duration::MAX` | reads: fresh; `explain().status` = `Fresh`; **`explain().ttl_remaining_secs` = `Some(0)`**. The `as i64` wraps to −1 | the remaining time saturates at a large value, never 0 |

The first defect matters in practice. NTP corrections, VM resumes, and manual clock changes all
step clocks back, and every entry written in the last few seconds then vanishes. The second makes
the diagnostic contradict itself. Both come from two sites doing the same arithmetic differently.

### Input the patch line announced

v0.21.4's summary announced that `max_entries(0)`, a `batch_set` larger than `max_entries`, and a
TTL under one second would be rejected from v0.22.0. RFC 024 B3 extended the bound to imports and
left the rejection variant to this RFC.

### `rusqlite::Error` in the public API

`Database(#[from] rusqlite::Error)` makes every `rusqlite` major a breaking change (RFC 023 R8.3).
v0.22.0 already moves to `rusqlite 0.40.2` (RFC 023 R9), so the error type changes in that release
either way. Making it opaque in the same release means that change is the last of its kind.

## Goals

1. Every error variant means one thing, and a caller can act on it without parsing its message.
2. One expiry rule, used by every site that decides or reports expiry.
3. The announced rejections, and nothing more, become errors in v0.22.0.
4. No dependency type in the public error type.

## Non-goals

- **`source()` chaining for every variant.** It is kept where it is free (R10). It is not a goal
  in itself.
- **Key zeroization and `aes-gcm 0.11`.** Assessed here, as the register asked, because this RFC
  touches key errors. Neither needs a public API change: the builder keeps accepting the key as
  today, and our internal copy could be zeroized at any time. So neither is tied to v0.22.0's
  breaking window, and both **stay in the register**, to be decided on their own merits.
- **Rejecting anything not announced.** An empty namespace, for example, is simply a name. The
  same goes for a fractional TTL of one second or more (R8).

## Part A — v0.21.5 (not breaking)

### R1 — One expiry rule

Add one private function, the only place TTL arithmetic happens:

```rust
/// Seconds the entry has left, saturating: `None` when there is no TTL.
fn ttl_remaining(now: i64, updated_at: i64, ttl: Option<Duration>) -> Option<u64>
```

- **Age is clamped at zero.** An `updated_at` in the future (the clock stepped back) means age 0,
  never a wrapped huge value.
- The TTL's seconds and the remaining time use saturating arithmetic. There is no `as` cast between
  signed and unsigned anywhere on this path.
- `is_expired` becomes `ttl_remaining(…) == Some(0)`. `explain` reports `ttl_remaining_secs` from
  the same function, converted to its public `i64` type with saturation at `i64::MAX`. Its
  expired flag is derived from it too, so the two can never disagree again.

**Behaviour:** entries stop expiring on a backward clock step, and `explain` stops reporting 0 for
a huge TTL. Nothing that expires correctly today changes. This is a correctness fix, in the spirit
of RFC 022. It rejects nothing.

### R2 — Tests, failing-before

- An entry whose stored `updated_at` is one second in the future is fresh through `get_if_fresh`,
  `check_status`, `explain`, and `cleanup_expired`, which must not delete it.
- `Duration::MAX`: `explain().ttl_remaining_secs` is a large positive value, never 0.
- Boundaries: exactly at the TTL, one second before, and one second after, for `ttl = 1 s` and
  `ttl = 3600 s`.

Each must fail on the v0.21.4 code first, except the boundaries, which are guards.

## Part B — v0.22.0 (breaking, announced in v0.21.5)

### R3 — Seven purpose-named variants

```rust
/// A configuration value is invalid, or two values cannot be combined. Returned by `open`/`build`.
#[error("invalid configuration: {option}: {reason}")]
InvalidConfiguration { option: &'static str, reason: String },

/// An argument to one call is invalid. Nothing was changed.
#[error("invalid argument: {argument}: {reason}")]
InvalidArgument { argument: &'static str, reason: String },

/// The database's schema is not the current one, and this open may not migrate it
/// (a read-only open). Opening writable migrates it.
#[error("database schema is not current; a read-only open cannot migrate it; database was not modified")]
SchemaNotCurrent,

/// The database's schema is not one this version recognizes: written by a newer
/// localcache, or not a localcache database. Nothing was modified.
#[error("incompatible database schema: {0}")]
IncompatibleSchema(String),

/// An encrypted entry was read, or a key rotation requested, with no key configured.
#[cfg(feature = "encryption")]
#[error("no encryption key is configured")]
MissingEncryptionKey,

/// SQLite or the database file could not be brought into, or verified in, the state
/// localcache requires (journal, `synchronous`, enforcement pragmas, migration pre- or
/// postconditions, a concurrent change during validation). The message says which.
#[error("database state: {0}")]
DatabaseState(String),

/// The operating system's file-watching facility failed.
#[cfg(feature = "watching")]
#[error("file watching failed: {0}")]
Watcher(String),
```

- **`option` and `argument` are `&'static str` names from a fixed set**, so they stay matchable.
  This is RFC 018's reason for `Poisoned { resource }`. The set: `option` ∈ {`database_path`,
  `read_only`, `encryption_key`, `max_entries`, `ttl`, `size`}; `argument` ∈ {`new_key`,
  `path_glob`, `index_name`, `items`, `records`}. A new name needs a review, like a new variant.
- **`MissingEncryptionKey` is gated on `encryption`,** because without that feature an encrypted
  entry is `UnknownEncoding` (the existing rule, documented on `EncryptionError`). The same goes
  for **`Watcher` and `watching`**. Each variant exists exactly in the builds that can return it.

### R4 — The mapping, site by site

Every row of the inventory maps to exactly one variant:

| Sites | Variant |
|---|---|
| read-only + in-memory; key length at open; `ReadPool` size; `ReadPool` + `:memory:` | `InvalidConfiguration` (`read_only`, `encryption_key`, `size`, `database_path`) |
| `rotate_encryption_key` key length; both glob errors; index identifier; `import_entries` base64 | `InvalidArgument` (`new_key`, `path_glob`, `index_name`, `records`) |
| read-only open of a non-current schema | `SchemaNotCurrent` |
| unrecognized schema | `IncompatibleSchema` |
| rotation without a key; decode of an encrypted entry without a key | `MissingEncryptionKey` |
| the seven `db/schema*` and migration state failures | `DatabaseState` |
| the eight watcher OS errors | `Watcher` |
| `encode_payload`'s unreachable tag | `Serialization` (an internal invariant, not a caller condition) |

**Messages keep their "database was not modified" / "transaction will be rolled back" clauses.**
Those are load-bearing facts for the reader.

The CLI's `copy` matches `SchemaNotCurrent` instead of message text, and its `SCHEMA_NOT_CURRENT`
constant is deleted.

### R5 — `UnsupportedFeature` and `PayloadVersionMismatch` are removed

After R4, no site constructs `UnsupportedFeature`. Nothing in this crate is an "unsupported
feature". Each case was either invalid input or an unsuitable state. It is removed rather than left
as an empty catch-all that invites future misuse.

`PayloadVersionMismatch` is removed, because version mismatch is a freshness outcome (see
Motivation).

### R6 — Reject what was announced, before any write

| Input | Where | Error |
|---|---|---|
| `max_entries(0)` (or `Some(0)` in `CacheOptions`) | `open`/`build`, every wrapper's `open` | `InvalidConfiguration { option: "max_entries", … }` |
| a TTL under one second | same | `InvalidConfiguration { option: "ttl", … }` |
| `batch_set` with more **distinct** paths than `max_entries` | before the transaction starts | `InvalidArgument { argument: "items", … }` |
| `import_entries` / `import_from` with more distinct paths than `max_entries` (RFC 024 B3) | before writing | `InvalidArgument { argument: "records", … }` |

"Distinct" means after canonicalization, the key the database uses. Duplicates in one batch count
once, as they occupy one row.

### R7 — `Database` stops exposing `rusqlite::Error`

- `Database(DatabaseError)`, where `DatabaseError` is a public opaque struct.
- It implements `Display`, and `Error::source()` returns the underlying error type-erased. It also
  provides `pub fn sqlite_extended_code(&self) -> Option<i32>`: the raw SQLite extended result
  code, when the failure has one, which is enough to recognize `SQLITE_BUSY` and friends.
- The `From<rusqlite::Error>` impl becomes crate-private (a `pub(crate)` constructor, not
  `#[from]`), so no `rusqlite` type appears in any public signature or public trait impl.
- It lands in the same release as RFC 023's `rusqlite 0.40.2` (Q1c), so users absorb one change to
  this variant, not two.

### R8 — TTL resolution is documented

TTLs have one-second resolution. A fractional TTL of one second or more is truncated to whole
seconds, as today. The rustdoc on `ttl`, `CacheOptions::ttl`, and `CacheOptionsExt` says so, next to
R6's rejection of values under one second. This is documentation, not a new rejection.

### R9 — RFC 024's removals

Execute RFC 024 B4 as written, including the two observable `ConnectionPool` strings, which become
`SyncCacheEngine`. `ReadPool`'s `:memory:` refusal is `InvalidConfiguration` under R4 anyway, and
its reason names `SyncCacheEngine`.

### R10 — `source()` where it is free

`Io` and `Database` already carry sources. `Watcher` and `DatabaseState` carry strings, because
their origins are `notify` errors and ad-hoc checks. Making `notify::Error` a public source would
expose a dependency type, the problem R7 removes. No other chaining is added.

## Test plan

- **Part A:** R2.
- **R3–R5:** for every inventory row, one test asserting the new variant, with its `option` or
  `argument` where it has one. Build them from the existing tests that assert `UnsupportedFeature`.
  A search for `UnsupportedFeature` and `PayloadVersionMismatch` must find neither anywhere in the
  crate. The CLI hint test passes with the variant match.
- **R6:** each rejection, with the database byte-identical before and after (nothing written). A
  `batch_set` whose duplicates bring it within the bound succeeds.
- **R7:** a busy database (the RFC 022 test hook pattern) yields `Database(e)` with
  `e.sqlite_extended_code()` equal to `SQLITE_BUSY`'s code. A compile-time check that no public item
  names `rusqlite`: the `api_surface.rs` approach, scanning `pub` signatures in `error.rs`.
- **The `compile_fail` exhaustiveness doctest** in `error.rs` is updated to the new variant list.

## Security considerations

- R1 removes a way to make every cached entry vanish with a clock change, which is a
  denial-of-service against cache warmth. It is not a confidentiality issue.
- `MissingEncryptionKey` separates "you forgot the key" from "wrong key or corrupted data"
  (`EncryptionError`). Callers can then treat the second as possible tampering without false alarms
  from the first.
- R7 does not hide information: the source chain and the SQLite code remain available.

## Compatibility

- **v0.21.5:** R1 changes only incorrect outcomes. It is not breaking.
- **v0.22.0:**
  - code matching `UnsupportedFeature` or `PayloadVersionMismatch` stops compiling, and must match
    the new variants;
  - code that names `rusqlite::Error` through `Database` must use `DatabaseError`;
  - four inputs start returning errors.

  All of this is announced in the v0.21.5 summary, and each change is listed in the v0.22.0 upgrade
  notes with its replacement.

## Alternatives considered

### Split into fewer variants (for example, one `Invalid(String)`)

This is less change, but it recreates the catch-all problem one level down. "Invalid" would then
mean configuration, argument, and state at once. **Rejected.**

### Keep `UnsupportedFeature` for future genuinely unsupported cases

No current case is one, and an empty catch-all attracts misuse. RFC 018 found that the catch-all
grew by accretion. If a genuinely unsupported feature ever arises, a precise variant is additive.
**Rejected.** This is the first decision.

### Make `PayloadVersionMismatch` real, returning it from `get` on a version mismatch

That would turn a freshness outcome into an error, and break every caller that treats a stale
version as a miss. **Rejected.** This is the second decision.

### Keep `rusqlite::Error` public

It is simpler, but every future `rusqlite` bump would then be a breaking release (RFC 023 R8).
**Rejected.** This is the third decision.

### Ship Part A in v0.22.0 with the rest

It is a correctness fix that breaks nothing, and users are exposed to it on every clock step until
it ships. **Rejected**, in favour of v0.21.5. This is the fourth decision.

## Rollback

Part A: revert the helper. It changes no public signature. Part B is withdrawn before v0.22.0 by
removing its slices and its v0.21.5 notice. After v0.22.0, the variants are the public contract.

## Decisions requested of the owner

**Decided 2026-09-24: all four accepted as recommended.**

1. **Remove `UnsupportedFeature`** after the split (recommended), or keep it for future use.
2. **Remove `PayloadVersionMismatch`** (recommended), or return it from `get`.
3. **Opaque `DatabaseError` in v0.22.0** (recommended), or keep `rusqlite::Error` public.
4. **Part A (the TTL fixes) in v0.21.5** (recommended), or with v0.22.0.

(The variant set in R3 follows from the inventory. If the owner wants it coarser or finer, say so
at review.)

The slices, which the architect schedules and the owner authorizes with this RFC:

| Slice | Content | Ships |
|---|---|---|
| **Q3a** | R1–R2 (TTL) | v0.21.5. It can start at acceptance, while the dev team is otherwise idle |
| **Q3d** | R7 (`DatabaseError`) | v0.22.0, directly after Q1c |
| **Q3b** | R3–R5 and R10 (variants, mapping, removals of the two variants, CLI match) | v0.22.0 |
| **Q3c** | R6, R8 (rejections, TTL docs) | v0.22.0, before RFC 024's Q2f, which uses its variant |
| **Q3e** | R9 (RFC 024 B4 removals) | v0.22.0, last among the API slices |

## Open questions

None. The four decisions above are settled.
