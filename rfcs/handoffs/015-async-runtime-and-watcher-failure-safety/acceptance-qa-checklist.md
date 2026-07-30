# RFC 015 Implementation Acceptance and QA Checklist

This checklist operationalizes the Accepted
[RFC 015](../../done/015-async-runtime-and-watcher-failure-safety.md).
The RFC remains authoritative; this list does not add or relax a requirement.

Testing is owned by the testing developer. Every box must be backed by an observed result, not by
inspection alone. An unrun check is a failure, not a pass.

## Authority and preconditions

- [ ] RFC 015 is under `rfcs/accepted/` with Status **Accepted** (commit `cbaa76a`).
- [ ] The implementation commit under test is identified exactly.
- [ ] The tracked worktree is clean.
- [ ] No schema version, payload wire format, fixture byte, Cargo feature name, dependency,
      workspace version, or `LICENSE`/`NOTICE` placement changed.
- [ ] `crates/cli/src/main.rs`'s `isatty` FFI declaration is unchanged.
- [ ] Neither residual pre-RC correction (`explain` partial-hash; CLI `import --overwrite`) is
      present in this range.

## R1a — the no-op cast is gone

- [ ] The `unsafe` block formerly at `async_engine.rs:343` is deleted and replaced by a safe borrow.
- [ ] `query_dry_run` behaviour is unchanged; its existing regression tests pass untouched.

## R1b — the type-changing cast is gone, by refactor not by re-justification

- [ ] The `unsafe` block formerly at `async_engine.rs:300` is deleted.
- [ ] **`grep -rn "unsafe" crates/localcache/src` returns no match.** The library contains zero
      `unsafe` blocks.
- [ ] `EngineCore<'e>` exists, is `pub(crate)`, and carries exactly `conn`, `namespace`, and
      `#[cfg(feature = "encryption")] encryption_key` — no other field.
- [ ] `EngineCore` mirrors every `#[cfg]` gate on the corresponding `CacheEngine<T>` field.
- [ ] `decode_pub` no longer exists as an inherent method used by the query path; decoding goes
      through a generic `decode_with<U>` taking its configuration from `EngineCore`.
- [ ] **No pointer cast was reintroduced anywhere to obtain a typed engine.**
- [ ] `QueryBuilder`'s public generic parameters are unchanged; no public type or signature changed.
- [ ] A query executed with a payload type `U` distinct from the engine's own `T` succeeds and
      returns correct data — the cross-type path is explicitly covered, not merely compiled.

## R2 — poisoned mutex returns an error

- [ ] `grep -c '\.lock()\.unwrap()' crates/localcache/src/cache/async_engine.rs` returns **0**.
- [ ] A private `AsyncCacheEngine::lock()` helper exists, mirroring `ConnectionPool::lock`.
- [ ] No new `LocalFileCacheError` variant was added; the enum is still not `#[non_exhaustive]`.
- [ ] Deliberately poisoning the mutex (panic inside one call while the lock is held) makes the
      **next** call return `LocalFileCacheError::UnsupportedFeature` rather than panic.
- [ ] A second and third subsequent call also return the error — the engine is not left in a state
      where only the first post-poison call is handled.

## R3 — panic parity across all three backends

- [ ] A panic inside the blocking closure yields `LocalFileCacheError::AsyncTaskPanicked` on
      **Tokio**.
- [ ] The same on **async-std**.
- [ ] The same on **smol**.
- [ ] The Tokio `SpawnBlocking` implementation is unchanged.
- [ ] `catch_unwind` wraps the **closure**, not the `.await`.
- [ ] `futures` was **not** added to `Cargo.toml` or `Cargo.lock`.
- [ ] The runtime-matrix test is explicitly unwind-only and asserts nothing under
      `panic = "abort"`.
- [ ] After a caught panic, the subsequent call surfaces R2's poisoned-mutex error rather than
      panicking — R2/R3 interlock is demonstrated, not assumed.

## R4 — watcher registration failures are observable

- [ ] `CacheWatcher::registration_errors()` and the `CacheDebouncedWatcher` equivalent exist.
- [ ] `PathRegistrationError` has **private** fields with accessors (not public fields).
- [ ] Constructing a watcher with at least one unwatchable path (e.g. non-existent parent
      directory) still **succeeds** — construction behaviour is unchanged.
- [ ] `registration_errors()` is non-empty and names the failing path.
- [ ] A `tracing::warn!` is emitted per registration failure.
- [ ] `watch()` / `unwatch()` / `watch_dir()` / `unwatch_dir()` signatures and behaviour are
      unchanged.
- [ ] Neither watcher constructor's signature changed.

## R5 — all three discard sites are handled

- [ ] A `contains` **error** results in the removal being attempted, not the path being skipped.
- [ ] `failed_invalidation_count()` exists on both watcher types and increments when `remove`
      returns `Err`.
- [ ] `dropped_event_count()` exists on both watcher types and increments when `try_send` returns
      `Err`.
- [ ] Forcing `remove` to fail during the callback increments `failed_invalidation_count()` and is
      not silently discarded.
- [ ] A burst exceeding the 256-slot channel increments `dropped_event_count()`, **and** every
      affected entry is confirmed removed from the database.
- [ ] A failed `remove` is **not** retried synchronously inside the callback.
- [ ] `docs/src/watching.md` and the watcher module docs document the bounded channel, both
      counters, and do **not** claim invalidation can never fail.

## R6 — bounded module splits, behaviour preserved

- [ ] `cache/engine/maintenance.rs` exists and holds the maintenance-ops `impl` block.
- [ ] `db/schema/classifier/tests.rs` exists; `classifier.rs` declares `mod tests;` and contains no
      inline `#[cfg(test)] mod tests`.
- [ ] No method changed signature, visibility, or semantics; no visibility was widened beyond
      `pub(crate)`.
- [ ] `CacheEngine` remains a single type.
- [ ] **The RFC 011 hostile-input suite passes unchanged** after `create_path_index` /
      `drop_path_index` / `list_path_indexes` moved.
- [ ] `indexes.rs`, `repository.rs`, `query.rs`, and `cli/src/main.rs`'s inline test module are
      untouched.
- [ ] No test's behaviour changed as a result of R6.

## R7 — compatibility

- [ ] No public item was removed, renamed, or had its signature changed.
- [ ] The only new public items are `registration_errors`, `dropped_event_count`,
      `failed_invalidation_count`, and `PathRegistrationError` (plus their debounced equivalents).
- [ ] `EngineCore` and `decode_with` are `pub(crate)` and absent from the public API surface.
- [ ] Workspace and member versions remain `0.20.0`.
- [ ] `CHANGELOG.md` records the work under `[Unreleased]` and does not mark v0.20.1 released.

## Gates

- [ ] `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — pass.
- [ ] `cargo test --workspace --all-targets --all-features --locked` — pass; the pre-existing 351
      tests still pass, plus the new cases above.
- [ ] `cargo test --workspace --doc --all-features --locked` — 26 doctests pass.
- [ ] `cargo fmt --all --check` — clean.
- [ ] `git diff --check` — clean.
- [ ] `python3 scripts/source_integrity.py --require-tracked` — 3 manifests, 7 targets.
- [ ] RFC 014 R8 declared-MSRV matrix — all four rows pass on `rustc`/`cargo` **1.85.0**.
- [ ] RFC 009 R7 per-feature matrix — pass (R1b and R6 touch `cfg`-gated fields).
- [ ] `mdbook build docs` — pass; generated `docs/book/` removed afterward.
- [ ] Each gate was run **after each of the six sequenced steps**, not only at the end.

## Exit and review

- [ ] Every requirement above has an observed result; no unrun gate is reported as passed.
- [ ] Any failed or skipped gate is disclosed in the review request rather than omitted.
- [ ] A focused M5 implementation review request is prepared, identifying the exact commit and
      mapping each requirement to its evidence.
- [ ] No milestone record, blocker closure, RFC lifecycle move, tag, push, publication, or hosted
      release is performed by this work.
