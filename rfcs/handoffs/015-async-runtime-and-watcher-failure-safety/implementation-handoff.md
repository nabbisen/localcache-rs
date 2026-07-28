# RFC 015 Implementation Handoff

## 1. Summary

Implement the M5 async-runtime and watcher failure-safety work defined by
[RFC 015](../../accepted/015-async-runtime-and-watcher-failure-safety.md).

RFC 015 is Accepted. **Its requirements are authoritative.** This handoff sequences the work,
names the exact call sites, and states the escalation rules; it adds no design decision and
overrides none. Where this document and the RFC appear to disagree, the RFC wins — report the
discrepancy instead of choosing.

Two design reviews are binding context, because two requirements have a plausible-looking wrong
implementation that would still compile and still pass the existing suite:

- `.git-exclude/reviewed/architect-rfc-015-design-review-2026-07-28.md`
- `.git-exclude/reviewed/architect-rfc-015-focused-rereview-2026-07-28.md`

M5 ends at a focused implementation review point. It closes no blocker, records no milestone,
moves no RFC to `done/`, and does not tag, publish, push, or create a release.

## 2. Scope followed

### In scope

| ID | Work | Primary site |
|---|---|---|
| R1a | Replace the no-op `unsafe` with a safe borrow | `cache/async_engine.rs:343` |
| R2 | `AsyncCacheEngine::lock()` helper; convert all `.lock().unwrap()` | `cache/async_engine.rs` (31 sites) |
| R3 | Closure-wrapped `catch_unwind` on async-std and smol backends | `cache/runtime.rs` |
| R6 | Extract maintenance ops; relocate classifier tests | `cache/engine.rs`, `db/schema/classifier.rs` |
| R1b | `EngineCore` + `decode_with`; delete the type-changing cast | `cache/query.rs`, `cache/engine.rs`, `cache/async_engine.rs:300` |
| R4/R5 | Watcher diagnostics and counters | `cache/watcher.rs` |

### Explicitly out of scope

Do not touch any of the following under this RFC. Each is deliberately excluded:

- `crates/cli/src/main.rs:849`'s `unsafe extern "C" { fn isatty(...) }` — a legitimate POSIX FFI
  declaration, **not** an unnecessary cast.
- `indexes.rs` (a freshly reviewed RFC 011 security boundary), `repository.rs`, `query.rs`
  splitting for size, and `cli/src/main.rs`'s inline test module — deferred debt.
- The two residual pre-RC corrections (`explain` partial-hash comparison; CLI `import --overwrite`
  truthfulness). They are separately scheduled and **due Aug 12**; they must not be folded in here.
- Adding a `LocalFileCacheError` variant, or marking the enum `#[non_exhaustive]` — tracked for
  0.21.0, not now.
- Any schema, payload wire-format, MSRV, dependency, feature-name, version, packaging, or release
  change.

### Sequencing — follow this order

Each step is independently reviewable, and the order is load-bearing, not stylistic:

1. **R1a** — one-line safe borrow. Zero design risk; halves the `unsafe` surface immediately and
   gives an early clean checkpoint.
2. **R2** — the `lock()` helper and all call-site conversions. Mechanical, and a **prerequisite for
   R3**: R3's `AssertUnwindSafe` is only sound because R2 makes the resulting poisoned state
   recoverable.
3. **R3** — closure-wrapped `catch_unwind` on the two non-Tokio backends.
4. **R6** — the two module moves. Do this **before R1b** so R1b's diff lands against a smaller
   `engine.rs`.
5. **R1b** — `EngineCore`, `decode_with`, and deleting the `:300` cast. Largest and only
   non-mechanical change; do it against an already-clean tree.
6. **R4/R5** — watcher diagnostics, counters, and the `tracing::warn!` emission.

## 3. Files to change

- `crates/localcache/src/cache/async_engine.rs` — R1a, R2, and the `:300` deletion in R1b.
- `crates/localcache/src/cache/runtime.rs` — R3, async-std and smol `SpawnBlocking` impls only.
  **Do not modify the Tokio impl**; it already produces the target behaviour and is the reference.
- `crates/localcache/src/cache/query.rs` — R1b: `QueryBuilder` field swap and the `execute_query`
  decode call site.
- `crates/localcache/src/cache/engine.rs` — R1b (`EngineCore`, `core()`, `decode_with`) and R6
  (extraction).
- `crates/localcache/src/cache/engine/maintenance.rs` — **new**, R6.
- `crates/localcache/src/db/schema/classifier.rs` and
  `crates/localcache/src/db/schema/classifier/tests.rs` — **new**, R6 relocation.
- `crates/localcache/src/cache/watcher.rs` — R4/R5.
- `docs/src/watching.md` — R4/R5 documentation.
- `CHANGELOG.md` — under `[Unreleased]`. Do not change the workspace version.

## 4. Design decisions and assumptions

These are settled. Implement them; do not re-litigate them mid-task.

- **R1a is a same-type cast.** `&*guard` already yields `&CacheEngine<T>`. If a lifetime error
  appears, it indicates `build`'s signature wants a higher-ranked trait bound — **not** that
  `unsafe` is required.
- **R1b's field set is already derived, not guessed.** `query.rs` reads exactly `conn` (4 sites)
  and `namespace` (3 sites); `decode_pub` → `decode` → `decode_payload` touches only
  `encryption_key`. `EngineCore` carries those three and nothing else, with `pub(crate)` fields.
- **`decode_payload` is already a free `pub(crate) fn`** at `serialization.rs:112`, bound
  `T: DeserializeOwned`, taking `Option<&[u8; 32]>`. `decode_with` is a thin wrapper over it — not
  a re-architecture. `EngineCore.encryption_key: Option<&'e [u8; 32]>` feeds it with no conversion.
- **`EngineCore` must mirror every `#[cfg]` gate** on the corresponding `CacheEngine<T>` field —
  today only `#[cfg(feature = "encryption")]` on `encryption_key`. `database_path` / `watch_dirs`
  are `watching`-gated, are not read by the query path, and are **not** part of `EngineCore`.
- **R2 reuses `UnsupportedFeature`.** Do not add a variant: `LocalFileCacheError` is not
  `#[non_exhaustive]`, so a new variant is source-breaking for downstream exhaustive matches.
  Mirror `ConnectionPool::lock` at `pool.rs:332` exactly, including message shape.
- **R3 wraps the closure, never the `.await`.** `catch_unwind` takes `FnOnce() -> R`, not a
  `Future`. Wrapping the await would require `futures::FutureExt::catch_unwind`; `futures` is not a
  workspace dependency and **must not be added**.
- **R5 counts, never retries.** A failed `remove` is counted, not retried — retrying synchronously
  inside the `notify` OS callback risks the same thread stall that makes backpressure unacceptable.
- **R6 is mechanical.** No method changes signature, visibility, or semantics. `CacheEngine` stays
  one type; only an `impl` block moves. `cache::engine::maintenance` is a *child* module of
  `cache::engine`, so it can access `CacheEngine`'s private items — no visibility widening is
  needed or permitted.

### Escalation triggers — stop and report, do not work around

1. **Any pointer cast reintroduced to obtain a typed engine for decoding.** This is a failure of
   R1b, not an acceptable fallback. If `decode_with` seems to require a typed engine, the refactor
   has gone wrong.
2. **Any need to add `futures`** (or any other dependency) to satisfy R3.
3. **Any need to widen a visibility beyond `pub(crate)`**, or to add/rename/remove a public item
   other than the additive ones R4/R5 name.
4. **Any behavioural test change required by R6.** R6 is behaviour-preserving by definition; a test
   that must change means the split was not mechanical.

## 5. Tests and gates to run

Full acceptance criteria are in `acceptance-qa-checklist.md`. Minimum per-step gate — run after
**each** of the six steps, not only at the end:

```sh
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked
cargo test --workspace --doc --all-features --locked
```

Additionally, after **R1b and R6** (both touch `cfg`-gated fields or module boundaries), run the
RFC 009 R7 per-feature matrix and the RFC 014 R8 declared-MSRV matrix:

```sh
cargo +1.85.0 check -p localcache --all-targets --all-features --locked
cargo +1.85.0 check -p localcache --all-targets --no-default-features --features localcache/async-std --locked
cargo +1.85.0 check -p localcache --all-targets --no-default-features --features localcache/smol --locked
cargo +1.85.0 check -p localcache-cli --all-targets --all-features --locked
```

After R6 specifically, confirm the RFC 011 hostile-input suite is green — the
`create_path_index` / `drop_path_index` / `list_path_indexes` delegations move in that step.

Run `cargo fmt --all` once after all implementation is complete, then re-run the gates. Do not
review the formatted output.

## 6. Generated artifacts

None. This RFC produces no fixture, archive, package, or evidence bundle. Any generated
`docs/book/` from a local `mdbook build` must be removed before the review request.

## 7. Known limitations

- **R3's parity guarantee is unwind-only.** Under `panic = "abort"` no backend catches anything and
  the process aborts identically; parity holds trivially but the `AsyncTaskPanicked` *error*
  guarantee does not apply. The runtime-matrix test must be written to assert nothing under
  `abort`, matching the RFC 011 panic-unwind precedent.
- **R2 does not repair engine state.** A poisoned mutex still means the data behind it may reflect a
  partially completed operation; R2 only stops the panic from propagating to callers who did nothing
  wrong. This matches `ConnectionPool`'s existing contract.
- **R4's accessor is pull-based**, which is why the `tracing::warn!` emission is required alongside
  it — the accessor is the audit trail, not the only signal.
- `UnsupportedFeature` becomes further overloaded by R2. Accepted for a patch release; the
  `#[non_exhaustive]` split is tracked for 0.21.0 and is out of scope here.

## 8. Recommended next step

Implement in the §2 order, gating after each step. On completion, prepare a focused M5
implementation review request that:

- identifies the exact implementation commit and range;
- maps each of R1a, R1b, R2, R3, R4, R5, R6 to its evidence;
- reports the runtime matrix, poisoned-mutex, watcher-diagnostics, and RFC 011 regression results;
- discloses any gate that failed or was not run, rather than omitting it; and
- claims no milestone completion, blocker closure, or release readiness.

Do not record M5 complete, move RFC 015 to `done/`, or begin M6 work until that review is accepted
and the owner authorizes the record.
