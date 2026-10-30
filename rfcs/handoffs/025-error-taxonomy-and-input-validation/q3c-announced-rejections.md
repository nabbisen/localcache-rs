# Q3c Handoff — The Announced Rejections, Before Any Write (RFC 025 R6 rows 1–3, R8)

RFC: `rfcs/accepted/025-error-taxonomy-and-input-validation.md`, Part B, **R6** (the first three rows) and **R8**;
Amendment 1 ("configuration is checked before any side effect"). The fourth row of R6 (imports) is **Q2f's**:
it is the same bound as RFC 024 B3 and is specified there once.
Scope: authorized by the owner on 2026-10-29. Order: … Q3b → **Q3c** → Q2f → Q3e → Q10 → release preparation.
Start when Q3b is committed locally. Slices stay in **local commits**; do not push.
Format of the review request: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6.
Authorship: high-capability model. Authorized: the owner.

## 0. What this is

Three inputs that v0.21.4's summary and v0.21.5's CHANGELOG announced would be rejected from v0.22.0 become
errors, **before the database is touched**. This is the Phase 24 rule "newly rejected input ships only in a
minor release". Nothing else is rejected: an empty namespace is a name; a fractional TTL of one second or more
is valid (R8). If a case is not in this file, **stop and file a design request**.

## 1. The rejections

| Input | Where | Error |
|---|---|---|
| `max_entries` of `Some(0)` (`CacheOptions { max_entries: Some(0), .. }` and the builder's `.max_entries(0)`) | `open`/`build`, **every** wrapper's `open`, `ReadPool::open` | `InvalidConfiguration { option: "max_entries", .. }` |
| a TTL under one second (`Some(d)` with `d < Duration::from_secs(1)`, **including zero**) | the same entry points | `InvalidConfiguration { option: "ttl", .. }` |
| `batch_set` with more **distinct** paths than `max_entries` | before the transaction starts | `InvalidArgument { argument: "items", .. }` |

- **Where the configuration checks live:** in `check_configuration` (`cache/engine.rs`), which Q8(a) made the
  first thing `CacheEngine::open` does. Both new checks join the existing ones there, so a rejected
  configuration leaves the filesystem and the database exactly as it found them (Amendment 1). Find every
  entry point that builds an engine or a pool (`CacheEngine::open`/`builder().build()`, `SyncCacheEngine`, the
  three async wrappers on every runtime, `ReadPool::open`, `CacheOptionsExt`) and show, in the request, that each
  reaches `check_configuration` before any side effect, or add the check at the one place where it does not.
- **Distinct** means after canonicalization, the key the database uses. Duplicates in one batch count once.
  Items that fail canonicalization or whose file is missing occupy no row and are not counted; they stay
  per-item failures in the `BatchSetReport` as today. Read `batch_set` first and report where the count goes
  and that the rejection happens **before** the `IMMEDIATE` transaction begins and before any payload is
  encoded or written. An accepted batch behaves exactly as today (exactly `max_entries` distinct paths is
  accepted).
- **The name sets (Q3b):** add `ConfigOption::{MaxEntries, Ttl}` and `ArgumentName::Items` to the crate-private
  enums, with `as_str()` returning `"max_entries"`, `"ttl"`, `"items"`. Nothing else.
- **Reasons** read once under the `Display` (`invalid configuration: max_entries: must be at least 1; use None
  for no limit`; `invalid configuration: ttl: must be at least one second, got 500ms`; `invalid argument:
  items: 5 distinct paths exceed max_entries (3)`). Final wording is yours; report it.
- **Read-only engines** are not exempt from configuration validation (the options are invalid regardless).
- **The CLI:** report whether it exposes any of these options (a `--ttl`, `--max-entries` flag, or an
  environment variable). If it does, its error text and exit status are covered by a test; if not, say so.

## 2. R8 — TTL resolution is documented (no new rejection)

The rustdoc of `CacheOptions::ttl`, the builder's `ttl`, and `CacheOptionsExt` (find every place a TTL is
documented) says: TTLs have **one-second resolution**; a fractional TTL of one second or more is truncated to
whole seconds (`1500 ms` behaves as `1 s`); a TTL under one second is rejected with
`InvalidConfiguration { option: "ttl" }`. The rustdoc of `CacheOptions::max_entries` currently says
"Nothing here is rejected as an error in this release: `max_entries(0)` …" (near `options.rs:183`); rewrite it
for what is now true, including the `batch_set` bound.

## 3. Tests (failing before, on `HEAD` of this slice's base)

1. **Each rejection through each entry point.** `max_entries(0)`, `ttl = 0`, `ttl = 999 ms`: `open`, the builder,
   `SyncCacheEngine`, each async wrapper that the suite runs, `ReadPool::open`. Assert the variant, the
   `option`, and the `Display`.
2. **Before any side effect.** On a path that does not exist, a rejected `open` leaves it absent; on an existing
   database it leaves the file byte-identical (hash before and after), `user_version` and journal mode
   unchanged. Extend the existing helper in `tests/open_rejects_before_side_effects.rs`.
3. **Boundaries (guards):** `ttl = 1 s` accepted; `ttl = 1500 ms` accepted and behaves as 1 s (an entry written
   `updated_at = now - 1` is expired, `now - 0` is not); `max_entries(1)` accepted.
4. **`batch_set`:** with `max_entries(3)`, a batch of 4 distinct existing files is rejected with
   `InvalidArgument { argument: "items" }`, the database is unchanged (entry count, no row written), and no
   eviction happened; a batch of exactly 3 is accepted; a batch of 5 items with only 3 distinct canonical
   paths (duplicates and one path spelled two ways) is accepted; a batch of 4 where one file is missing counts
   3 and is accepted with that item reported as a failure, as today.
5. **Existing tests.** Some tests may use a sub-second TTL or `max_entries(0)`. Report each (file, test) and
   how you adapted it (set `updated_at` through the existing test hooks rather than sleeping). Do not delete a
   test without saying why.
6. Every other test passes; the compat fixtures unmodified.

## 4. Documentation and CHANGELOG

- Book pages that describe `ttl`, `max_entries`, `batch_set` limits or "announced" rejections: `grep -rn
  "max_entries\|announced\|TTL\|ttl" docs/src README.md`; rewrite what is now true and **re-read each page in
  full**. Say what else you found untrue.
- `docs/src/errors.md`: add the three conditions to the "v0.22.0 migration note: purpose-named variants" table
  (`option: "max_entries"`, `option: "ttl"`, `argument: "items"`), and to the variant rows.
- `CHANGELOG.md`, `## [Unreleased]` / `### Changed`, a **Breaking** entry: three newly rejected inputs, each with
  what to do (`None` for no limit; a TTL of at least one second, or no TTL; split a batch or raise `max_entries`).
  Keep the other entries.

## 5. Gates and the request

As in Q3b's handoff § 6. File `.git-exclude/review-request/NNN-dev-q3c-…-<date>.md` describing the
**uncommitted** tree; commit locally only after approval; no push, tag, version bump or `ROADMAP.md` edit. List
every required step and every judgement call.

## 6. Not yours

Imports and `max_entries` (Q2f); RFC 024 B1/B2; removals (Q3e); the bounded-decode error (Q10); any other new
rejection; the `ttl` arithmetic (Part A, done).
