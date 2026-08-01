# RFC 018 Acceptance and QA Checklist — Truthful Error Taxonomy

Operationalizes [RFC 018](../../done/018-truthful-error-taxonomy.md) (Phase 22 N1).
The RFC is authoritative; this list adds and relaxes nothing.

Testing is owned by the testing developer. **Every box must be backed by an observed
result. An unrun check is a failure, not a pass.**

## Preconditions

- [ ] The implementation commit under test is identified exactly.
- [ ] The tracked worktree is clean.
- [ ] No version was bumped and `CHANGELOG.md`'s version heading is unchanged.
- [ ] No schema, migration, payload wire format, or SQL change is present in the diff.
- [ ] No file under `scripts/` was modified.

## R1 — Enum is extensible

- [ ] `LocalFileCacheError` carries `#[non_exhaustive]`.
- [ ] A downstream-style exhaustive `match` without a `_` arm fails to compile. If the
      suite has no compile-fail harness, that is **reported explicitly**, not skipped.

## R2 — `Poisoned` variant

- [ ] The variant exists as `Poisoned { resource: &'static str }` — **not** `String`,
      **not** a unit variant.
- [ ] It is **ungated**: no `#[cfg]` on the variant.
- [ ] Poisoning `ConnectionPool` yields `Poisoned { resource: "ConnectionPool" }`.
- [ ] Poisoning `AsyncCacheEngine` yields `Poisoned { resource: "AsyncCacheEngine" }`.
- [ ] Poisoning the watcher's engine lock yields `Poisoned { resource: "CacheWatcher" }`.
- [ ] **None of the three returns `UnsupportedFeature` any more** — verified by
      assertion, not by reading the diff.
- [ ] The existing "does not attempt to repair engine state" doc wording on
      `ConnectionPool::lock` and `AsyncCacheEngine::lock` is retained.

## R3 — JSON failures use `Serialization`

- [ ] A JSON **encode** failure yields `Serialization`.
- [ ] A JSON **decode** failure yields `Serialization`.
- [ ] Neither yields `UnsupportedFeature`.
- [ ] The changelog entry describes this as a **behaviour change**, not a refactor.

## R4 — One poisoning policy

- [ ] `ReadPool::checkout` returns `Poisoned { resource: "ReadPool" }` on a poisoned
      blocking lock. It no longer calls `into_inner()`.
- [ ] **`ReadPool` under slot contention with no poisoning still succeeds.** The
      `try_lock` loop still skips busy slots; contention did not become an error.
      *(This is the most likely regression in the whole task.)*
- [ ] Every `ReadPool` public method whose signature changed is listed in the review
      request.
- [ ] `docs/src/api.md` reflects the changed signatures.
- [ ] The two `CacheWatcher` callback sites that skip on poisoning each carry a comment
      stating the skip is deliberate and why. Their **behaviour is unchanged**.

## R5 — Migration note

- [ ] `CHANGELOG.md` and `docs/src/errors.md` both state: add a `_` arm; poisoning now
      returns `Poisoned`; JSON codec failures now return `Serialization`; no schema,
      payload, or database change.
- [ ] `docs/src/errors.md`'s error-variant table includes `Poisoned`.

## R6 — Non-goals respected

- [ ] `UnsupportedFeature` still carries its glob and schema-configuration uses; they
      were **not** split into new variants.
- [ ] No `source()` chaining was added.
- [ ] No other variant was restructured.

## Gates

- [ ] Full test suite passes; counts before and after are both reported.
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean.
- [ ] `cargo fmt --all --check` clean.
- [ ] The complete declared-MSRV matrix passes on Rust 1.85.
- [ ] Tests pass under each async feature combination that gates `AsyncCacheEngine`,
      not only the default feature set.
- [ ] `python3 scripts/source_integrity.py --require-tracked` OK.
- [ ] `git diff --check` clean.

## Scope containment

- [ ] The `ReadPool` ripple stayed inside `read_pool.rs` and its documentation. If it
      reached `ConnectionPool`, the async engine, or elsewhere, that was **reported
      before proceeding**, not absorbed.
- [ ] Every pre-existing test that asserted `UnsupportedFeature` for poisoning or JSON
      errors is enumerated in the review request with its update.
