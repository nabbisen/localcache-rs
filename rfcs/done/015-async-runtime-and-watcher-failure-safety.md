# RFC 015 — Async Runtime and Watcher Failure Safety

| Field | Value |
|---|---|
| Status | Implemented (0.20.1) |
| Feature | *(core; parity for `async`/`async-std`/`smol` and `watching` features)* |
| Touches | `crates/localcache/src/cache/async_engine.rs`, `crates/localcache/src/cache/runtime.rs`, `crates/localcache/src/cache/query.rs`, `crates/localcache/src/cache/watcher.rs`, `crates/localcache/src/cache/engine.rs`, `crates/localcache/src/db/schema/classifier.rs`, `crates/localcache/src/error.rs`, async/watcher documentation and tests |
| Finding | Architect M4 close-out and M5 handoff review (2026-07-28); originating findings from the 2026-07-17 architecture review; corrected per architect RFC 015 design review (2026-07-28) |
| Milestone | Phase 21 M5 |

## Summary

Remove the two `unsafe` raw-pointer casts in `AsyncCacheEngine`'s query path by
giving `QueryBuilder` a payload-type-independent borrow of the engine instead
of reinterpreting `CacheEngine<T>` as `CacheEngine<U>`. Replace every
`.lock().unwrap()` in `async_engine.rs` with the poison-handling convention
`ConnectionPool` and `CacheWatcher` already use — mapping a poisoned mutex to
the existing `LocalFileCacheError::UnsupportedFeature` variant — so a panicked
async task can no longer poison the shared engine mutex and panic every
subsequent caller. Make panic behavior identical across the Tokio, async-std,
and smol backends. Make watcher path-registration failures observable without
changing any existing constructor signature or default behavior. Give the
256-event watcher channel a documented, observable drop policy instead of a
silent one. Split `engine.rs` and relocate `classifier.rs`'s inline test
module to satisfy the project's file-size and test-placement rules, touching
no other oversized file.

This RFC changes no database schema, no payload wire format, and no existing
public method signature. It adds a small number of new, purely additive
public methods. Accepted status authorizes M5 implementation under this
design; it does not close B-07, complete M5, or authorize a release.

## Motivation

The M4 close-out review re-confirmed six concrete gaps against the current
tree (all file/line citations below are current as of commit `a8058d9`):

1. **Two `unsafe` raw-pointer casts**, of two different kinds, sit in
   `AsyncCacheEngine::query_run` and `AsyncCacheEngine::query_dry_run`
   (`async_engine.rs:300`, `:343`). The existing safety comment on both says
   the cast "erases the lifetime" required by `spawn_blocking`. That is not
   the operative hazard for either site: the closure never returns a borrow,
   so nothing inside it actually needs `'static`.
   - At `:343` (`query_dry_run`), the cast is `CacheEngine<_>` →
     `CacheEngine<T>` — the **same** type parameter. `&*guard` already
     yields `&CacheEngine<T>`; this `unsafe` block is a no-op that can be
     replaced by a plain safe borrow independently of everything else in
     this RFC.
   - At `:300` (`query_run`), the cast is a genuine type-parameter change,
     `CacheEngine<T>` → `CacheEngine<U>`, because `QueryBuilder<'e, U>` is
     defined as holding `&'e CacheEngine<U>` and Rust has no safe way to
     view a locked `CacheEngine<T>` as a `CacheEngine<U>`. The cast
     currently *works* only because `CacheEngine<T>`'s sole `T`-dependent
     field is a zero-sized `PhantomData<T>` (`engine.rs:81`) — but
     `repr(Rust)` gives no cross-instantiation layout guarantee, even for a
     difference limited to one ZST field. The cast relies on unspecified
     compiler behavior, not a language guarantee; today's practical risk is
     low, but the correct framing is "the language never promised this,"
     not merely "a future field could invalidate it."
2. **`.lock().unwrap()` throughout `async_engine.rs`** (at minimum lines 60,
   71, 82, 96, 104, 112, 117, 122, 131, 140, 145, 150, 155, 160, 165, 170,
   175, 183, 192, 205, 213, 222, 228, 234, 243, 252, 289, 338, 354, 360, 366)
   means one panicking async task poisons the shared `Mutex<CacheEngine<T>>`
   and turns every subsequent call on every clone of that `AsyncCacheEngine`
   into a panic, for the lifetime of the process. `ConnectionPool::lock`
   (`pool.rs:332`) and `CacheWatcher::new_with_paths`
   (`watcher.rs:95`–`97`) already avoid this by mapping a poisoned lock to
   `LocalFileCacheError::UnsupportedFeature`. `AsyncCacheEngine` is the one
   outlier from an otherwise consistent project convention, not a case that
   needs new design.
3. **Panic behavior diverges by runtime backend** (`runtime.rs`). The Tokio
   backend maps a blocking-task panic to `LocalFileCacheError::AsyncTaskPanicked`
   because `tokio::task::spawn_blocking` returns a `JoinHandle` whose `.await`
   yields `Result<R, JoinError>`. `async_std::task::spawn_blocking` and
   `smol::unblock` both yield the value directly (`R`, not `Result<R, _>`); a
   panicked closure propagates as an actual unwind through the `.await`
   point instead of becoming an `AsyncTaskPanicked` error. The three
   backends are meant to be interchangeable (RFC 005 DEC-004); today they are
   not, for the one failure mode most likely to matter to a caller.
4. **Watcher path registration failures are discarded.**
   `CacheWatcher::new_with_paths` and `CacheDebouncedWatcher::new_with_paths`
   register each initial path with `let _ = os_watcher.watch(...)`
   (`watcher.rs:156`, `:162`, `:381`, `:387`); a watcher can report successful
   construction while one or more of its intended paths are silently
   unwatched. `CacheWatcher::watch` / `watch_dir` (the *post-construction*
   API) already return `Result`; only the paths supplied at construction time
   lose their error.
5. **The invalidation event channel silently drops events past its bound.**
   `try_send` on the 256-slot `mpsc::sync_channel` (`watcher.rs:137`, `:363`)
   discards the notification on a full channel. The underlying cache
   invalidation (`eng.remove(path)`) already happened by that point in both
   watcher types — a dropped event loses observability, not cache
   correctness — but the drop is currently undocumented and uncountable.
6. **Six source files exceed the project's 300/500 ELOC guidance** with no
   further split: `engine.rs` (1536), `classifier.rs` (1185, ~627 of
   implementation before its inline `#[cfg(test)] mod tests`), `indexes.rs`
   (989), `cli/src/main.rs` (917), `repository.rs` (767), `query.rs` (731).
   `indexes.rs` and `classifier.rs` also still embed `#[cfg(test)] mod tests`
   directly rather than declaring `mod tests;` to a sibling file, unlike the
   rest of the codebase (`cache/glob.rs` → `cache/glob/tests.rs`, `path.rs` →
   `path/tests.rs`, `db/schema.rs` → `db/schema/tests/*`).

`crates/cli/src/main.rs:849`'s `unsafe extern "C" { fn isatty(fd: i32) -> i32; }`
is a legitimate FFI declaration for a POSIX function, not an unnecessary
generic cast. It is explicitly **not** in scope for item 1 or item 6; a
future implementer must not remove or "fix" it under this RFC.

## Goals

1. Remove both `unsafe` blocks in `async_engine.rs`'s query path without
   changing `AsyncCacheEngine::query_run` / `query_dry_run`'s public
   signatures or behavior.
2. Make a poisoned `AsyncCacheEngine` mutex return an error instead of
   panicking every subsequent caller, using the project's existing
   `UnsupportedFeature` poison-handling convention rather than introducing a
   new error variant.
3. Make a panic inside the blocking closure produce the same
   `LocalFileCacheError::AsyncTaskPanicked` result on all three async
   backends (Tokio, async-std, smol).
4. Make per-path watcher registration failures observable through a new,
   additive accessor without changing either watcher constructor's
   signature or its current partial-success behavior.
5. Replace the undocumented silent event drop with a documented, countable
   one via a new, additive accessor.
6. Bring `engine.rs` under the project's ELOC guidance via risk-reducing
   extraction, and relocate `classifier.rs`'s inline test module to a
   sibling file to satisfy the project's test-placement rule.
7. Introduce no schema change, no wire-format change, no removed or
   renamed public item, and no behavior change to any currently-succeeding
   call.

## Non-goals

- The two residual pre-RC corrections (`explain` partial-hash comparison;
  CLI `import --overwrite` truthfulness) are **not** part of this RFC. They
  ride in the next implementation or pre-RC acceptance package per the
  roadmap and must not be used to broaden this RFC's scope.
- Splitting or otherwise touching `indexes.rs`, `repository.rs`, `query.rs`,
  or `cli/src/main.rs`'s inline test module. `indexes.rs` is a freshly
  reviewed security boundary (RFC 011); churning any of these four for size
  alone invites regression for no correctness gain and is deferred as
  tracked debt.
- Removing, renaming, or otherwise changing `crates/cli/src/main.rs`'s
  `isatty` FFI declaration.
- Raising the MSRV, changing a Cargo feature name, or touching the
  dependency-security policy (RFC 014 scope).
- Backpressure (blocking send) on the watcher event channel — rejected below.
- Any release, packaging, or version-number action (RFC 009/M6 scope).

## Requirements

### R1 — Remove the query-path `unsafe` casts

This requirement has two independent parts. The `:343` no-op cast (R1a) can
land on its own, with zero design risk, ahead of everything else in this RFC.
The `:300` type-changing cast (R1b) is the substantive part and is only
removed if its decisive step — the `decode_pub` refactor below — is actually
done; giving `QueryBuilder` a smaller borrowed type is necessary but **not
sufficient** on its own.

**R1a.** In `query_dry_run`, replace the `unsafe { &*(&*guard as *const
CacheEngine<_>) }` block with a plain safe borrow of `&*guard` (already typed
`&CacheEngine<T>`, which is exactly what is needed). No other change.

**R1b.** Give `QueryBuilder` a borrowed view of the engine that does not
depend on the engine's payload type `T`, so `AsyncCacheEngine<T>::query_run<U>`
never needs to reinterpret `CacheEngine<T>` as `CacheEngine<U>`.

The field set is derived mechanically from what `query.rs` actually reads
today, not assumed up front: `execute_query` and `dry_run` read only
`q.engine.conn` (4 sites) and `q.engine.namespace` (3 sites) directly.
Introduce a small `pub(crate)` type holding exactly those fields by
reference:

```rust
pub(crate) struct EngineCore<'e> {
    pub(crate) conn: &'e rusqlite::Connection,
    pub(crate) namespace: &'e str,
    #[cfg(feature = "encryption")]
    pub(crate) encryption_key: Option<&'e [u8; 32]>,
}
```

with a zero-cost accessor `CacheEngine<T>::core(&self) -> EngineCore<'_>`.
`EngineCore` must mirror every `#[cfg]` gate present on the corresponding
`CacheEngine<T>` field — today only `encryption_key`
(`#[cfg(feature = "encryption")]`) — or a feature-combination build breaks;
`database_path`/`watch_dirs` (`#[cfg(feature = "watching")]`) are not read by
the query path and are not part of `EngineCore`.

The field swap alone does not remove the `:300` cast: `execute_query`
obtains its payload via `q.engine.decode_pub(&payload_row.content,
&payload_row.encoding)` (`query.rs:579`), an **inherent method on
`CacheEngine<T>`**. Handing `QueryBuilder` an `EngineCore<'e>` does nothing
unless `decode_pub` also stops being a method on the typed engine — an
implementer who keeps `decode_pub` as-is will still need a typed engine to
call it and may reintroduce a cast to get one. `decode_pub`
(`CacheEngine<T>::decode_pub`, `engine.rs:1427`) touches only
`self.encryption_key` (via `decode`/`decode_payload`); it must become a
generic function parameterized by the target payload type, taking its
configuration from `EngineCore`:

```rust
fn decode_with<U: DeserializeOwned>(
    core: &EngineCore<'_>,
    bytes: &[u8],
    encoding: &str,
) -> Result<U, LocalFileCacheError> {
    decode_payload(
        bytes,
        encoding,
        #[cfg(feature = "encryption")]
        core.encryption_key,
    )
}
```

Change `QueryBuilder<'e, T>` to hold `EngineCore<'e>` (plus `PhantomData<T>`
where a type marker is still needed) instead of `&'e CacheEngine<T>`, and
change `execute_query`'s decode call site to `decode_with::<U>(&core, ...)`.
Reintroducing any pointer cast to obtain a typed engine for decoding is a
failure of this requirement, not an acceptable fallback.

With both parts done, `AsyncCacheEngine::query_run` builds `QueryBuilder<'_, U>`
directly from `guard.core()` — a safe, ordinary borrow scoped to the closure
— and never constructs or dereferences a raw pointer. Both `unsafe` blocks in
`async_engine.rs` are deleted, not merely re-justified.

`CacheEngine<T>::query(&self) -> QueryBuilder<'_, T>` (the existing
same-type, synchronous entry point) is unaffected in behavior; it may be
implemented in terms of `self.core()` for consistency. `EngineCore` and
`decode_with` are `pub(crate)`; this requirement changes no public type or
signature.

### R2 — Poisoned-mutex handling matches the rest of the project

Replace every `.lock().unwrap()` in `async_engine.rs` with the same pattern
`ConnectionPool::lock` already uses: map a poisoned lock to
`LocalFileCacheError::UnsupportedFeature("AsyncCacheEngine mutex was poisoned".into())`.
Do not add a new `LocalFileCacheError` variant. `error.rs`'s enum is not
`#[non_exhaustive]`, so a new variant would be a source-breaking change for
any downstream exhaustive `match`; reusing the existing variant avoids that
question entirely and keeps this a patch-compatible change, consistent with
every other Phase 21 corrective RFC.

Introduce one private helper (e.g. a `fn lock(&self) -> Result<MutexGuard<'_, CacheEngine<T>>, LocalFileCacheError>` on `AsyncCacheEngine`, mirroring
`ConnectionPool::lock`) so the ~30 call sites become `self.lock()?...` instead
of each repeating its own poison-mapping closure.

A poisoned mutex still means the data behind it may reflect a partially
completed operation. Returning `UnsupportedFeature` does not attempt to
un-poison or repair engine state; it only stops the panic from propagating to
callers who did nothing wrong. This matches `ConnectionPool`'s existing
contract and requires no new documentation beyond noting the parity.

`UnsupportedFeature` already carries schema-rejection, runtime-configuration,
migration-invariant, and now pool/watcher/async poisoning meanings,
distinguished only by its message string. That is the correct tradeoff for a
patch release — it adds no new variant and no compatibility question — but it
should not calcify. Record as a tracked follow-up, not part of this RFC: at
the next breaking-change slot (0.21.0), mark `LocalFileCacheError`
`#[non_exhaustive]` and give poisoning its own variant.

### R3 — Uniform panic behavior across async backends

Make the async-std and smol `SpawnBlocking` implementations produce
`LocalFileCacheError::AsyncTaskPanicked` on a panicking closure, matching the
Tokio backend. `std::panic::catch_unwind` takes `FnOnce() -> R`, not a
`Future`; catching a panic that propagates *through* an `.await` would
otherwise require the `futures` crate's `FutureExt::catch_unwind`, which is
not a workspace dependency today and is unnecessary here — the panic can
instead be caught **around the closure, before it is handed to the
runtime**, so it never crosses an `.await` point at all and no new dependency
is needed:

```rust
let guarded = move || {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .unwrap_or(Err(LocalFileCacheError::AsyncTaskPanicked))
};
async_std::task::spawn_blocking(guarded).await   // and smol::unblock(guarded).await
```

`async_std::task::spawn_blocking` and `smol::unblock` both yield `R`
directly rather than `Result<R, JoinError>`; wrapping `f` itself makes both
backends produce exactly Tokio's observable behavior without depending on
either runtime's internal panic handling.

`AssertUnwindSafe` is justified not because "nothing depends on inspecting
the closure's state after a caught panic" — the closure captures
`Arc<Mutex<CacheEngine<T>>>` and may panic while holding the lock, which is
precisely the hazard `UnwindSafe` exists to flag — but because R2 already
converts that hazard into a handled case: a panic while the mutex is held
poisons it, and R2 maps a poisoned mutex to a recoverable
`LocalFileCacheError::UnsupportedFeature` on the *next* lock attempt rather
than a panic. R2 and R3 interlock: R3's `AssertUnwindSafe` is sound because
R2 guarantees the poisoned state that follows is observable and recoverable,
not because the state after a panic is assumed harmless.

This parity holds only under Rust's default unwinding panic strategy. Under
`panic = "abort"`, no backend catches anything and the process aborts on any
panic; parity is trivially preserved (all three backends abort identically)
but the `AsyncTaskPanicked` *error* guarantee does not apply. Document this
caveat alongside the requirement, matching the precedent already set by the
RFC 011 panic-unwind test.

Do not attempt to recover or resume the panicked closure. The requirement is
parity of the *error produced* under unwinding, not panic suppression.

### R4 — Observable watcher registration failures, additive only

Add, without changing any existing method signature:

- `CacheWatcher::registration_errors(&self) -> &[PathRegistrationError]`
- `CacheDebouncedWatcher::registration_errors(&self) -> &[PathRegistrationError]`

where `PathRegistrationError` is a new public struct with private fields and
accessors (`path(&self) -> &Path`, `message(&self) -> &str`) — not public
fields — so its shape can still evolve without a breaking change; mark it
`#[non_exhaustive]` as well if future variants (e.g. an error kind) are
plausible. Collect one entry during `new_with_paths` for each `Err` from
`os_watcher.watch(...)` / `deb.watcher().watch(...)` instead of discarding it
with `let _ =`. Construction continues to succeed with partial coverage
exactly as it does today — this requirement makes existing behavior
observable, it does not change when construction succeeds or fails.
`watch()` / `unwatch()` / `watch_dir()` / `unwatch_dir()` (the already-
fallible post-construction API) are unaffected.

An accessor nobody polls does not improve operations by itself — it makes a
failure *discoverable*, not *visible*. Also emit `tracing::warn!` for each
registration failure at the point it is collected (the `tracing` feature
already instruments other hot paths); this is zero API impact and gives an
operator a signal without requiring them to call `registration_errors()`.
`registration_errors()` remains the authoritative record; the trace event is
a notification, not a replacement.

This was chosen over a fallible constructor specifically to avoid the
breaking API change the M4 close-out review flagged as the one item in M5
capable of breaking downstream code (see Alternatives considered).

### R5 — Documented, countable event drop, and countable invalidation failure

The watcher callback in both `CacheWatcher` and `CacheDebouncedWatcher`
discards three results, not one:

```rust
if !eng.contains(path).unwrap_or(false) {   // watcher.rs:133 / :358
    continue;                                //   a FAILED contains → treated as "not cached" → skip
}
let _ = eng.remove(path);                    // watcher.rs:136 / :361 — error discarded
let _ = inner_cb.tx.try_send(WatchEvent{…}); // watcher.rs:137 / :363
```

Countable drop for the third (the event-channel send) is justified only if
the first two cannot silently fail — otherwise a failed `remove` (database
locked, I/O error, read-only engine) or a `contains` error produces a
**silently stale cache entry**: no error, no count, and possibly no
notification either, which is the same silent-staleness failure class RFC
010 (B-02) closed for migrations, now recurring in the one component whose
job is invalidation. This requirement therefore covers all three:

- Treat a `contains` **error** as "attempt the removal anyway" rather than
  "skip" — an error from `contains` is not evidence the entry is absent.
- Add `CacheWatcher::failed_invalidation_count(&self) -> u64` (and the
  `CacheDebouncedWatcher` equivalent), backed by an `AtomicU64` incremented
  whenever `remove` returns `Err`.
- Add `CacheWatcher::dropped_event_count(&self) -> u64` (and the
  `CacheDebouncedWatcher` equivalent), backed by an `AtomicU64` incremented
  whenever `try_send` returns `Err` at `watcher.rs:137` / `:363`, as
  originally specified.

Both counters may be exposed via one combined diagnostics accessor or two
separate methods; either satisfies this requirement, and neither changes any
constructor signature. Document in the watcher module docs and
`docs/src/watching.md` that the invalidation channel is bounded (256
events), that a successful `remove` is never itself undone by a dropped
notification, and that `failed_invalidation_count` / `dropped_event_count`
are how each respective failure is detected — not that invalidation can
never fail.

Backpressure (blocking `send` instead of `try_send`) is rejected for the
event channel: the send happens inside the `notify` crate's OS callback
thread, and blocking there risks stalling or losing events at the OS watch
layer itself, which is a strictly worse failure mode than a counted drop at
the application layer. The same reasoning applies to `remove` failures —
retrying synchronously inside the OS callback risks the same stall — so a
failed `remove` is counted, not retried, by this requirement.

### R6 — Bounded module splits

Split `engine.rs` only. Extract the maintenance-operation methods already
called out as extractable in the v0.20.0 handoff notes (`cleanup_missing_files`,
`cleanup_expired`, `purge_stale_versions`, `shrink_database`,
`entry_count`, `entry_count_by_version`, `cache_stats`, and the
`create_path_index` / `drop_path_index` / `list_path_indexes` trio) into a
sibling `impl` block in `cache/engine/maintenance.rs`, declared from
`engine.rs` as `mod maintenance;`. `CacheEngine` remains one type; only the
`impl` block housing these methods moves. This is a mechanical,
behavior-preserving split — no method changes signature, visibility, or
semantics.

Relocate `classifier.rs`'s inline `#[cfg(test)] mod tests { ... }`
(currently starting at line 629) into `db/schema/classifier/tests.rs`,
declared as `mod tests;`, matching the pattern already used by
`db/schema/tests/*`. No test changes behavior; only its file location moves.

`create_path_index` / `drop_path_index` / `list_path_indexes` — the RFC 011
public API — move into `maintenance.rs` as part of this extraction. They are
thin delegations to `indexes::`; the RFC 011 security boundary itself
(`indexes.rs`) is untouched, consistent with leaving it alone. Because
`cache::engine::maintenance` is a child module of `cache::engine`, it may
access its parent's private items, so the move is mechanically viable
without widening any visibility. The RFC 011 hostile-input test suite must
be confirmed green after the move.

No other file listed in the Motivation's item 6 is touched by this RFC.

### R7 — Compatibility discipline

No existing public method's signature, return type, or error behavior for a
currently-`Ok` call changes. The new items introduced by R4 and R5
(`registration_errors`, `dropped_event_count`, `failed_invalidation_count`,
`PathRegistrationError`) are purely additive. `PathRegistrationError` uses
private fields with accessors specifically so its own shape can evolve later
without a further breaking change. R1's `EngineCore` and `decode_with` are
`pub(crate)` and not part of the public API; `QueryBuilder`'s `engine` field
is already `pub(crate)`, so replacing it with `EngineCore` is itself
invisible to downstream code. R2 and R3 change what an *already-failing*
call returns (a panic becomes a recoverable `Err`, or one backend's error
becomes consistent with another's) — this is a bug fix to previously
undocumented, inconsistent failure behavior, not a change to any documented
contract, and is treated as patch-compatible for the same reason RFC 012/013
treated their read-only and panic-safety corrections as patch-compatible.

## Detailed design

### Implementation sequencing

Land in this order; each step is independently reviewable and the riskiest,
least-mechanical work comes last:

1. **R1a** — delete the no-op `unsafe` at `:343` with a plain borrow. Zero
   design risk; immediately halves the unsafe surface.
2. **R2** — add the `AsyncCacheEngine::lock()` helper mirroring
   `ConnectionPool::lock`, and convert all ~30 call sites. Mechanical, and a
   prerequisite for R3's `AssertUnwindSafe` justification.
3. **R3** — closure-wrapped `catch_unwind` on the async-std and smol
   backends, plus the unwind-only runtime-matrix test.
4. **R6** — the two mechanical module moves, including the RFC 011
   delegations. Do this before R1b so R1b's diff lands against a smaller
   `engine.rs`; confirm the RFC 011 suite is green immediately after.
5. **R1b** — `EngineCore`, `decode_with`, and deleting the `:300` cast. The
   largest and only non-mechanical change; do it against an already-clean
   tree.
6. **R4/R5** — watcher diagnostics, including `failed_invalidation_count`,
   `dropped_event_count`, `registration_errors`, and the `tracing::warn!`
   emission.

Gate after each step with
`cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`,
the full test suite, and — because R1b and R6 touch `cfg`-gated fields and
module boundaries — the RFC 009 R7 per-feature matrix and the RFC 014 R8
declared-MSRV matrix.

## Test plan

- A runtime-matrix test that deliberately panics inside the blocking closure
  on each of the Tokio, async-std, and smol backends and asserts all three
  observe `LocalFileCacheError::AsyncTaskPanicked`. This test is **unwind-only**
  by construction: it asserts nothing under `panic = "abort"`, where all three
  backends abort the process identically instead of producing this error.
- A test that deliberately poisons an `AsyncCacheEngine`'s mutex (panic
  inside one call while holding the lock) and asserts the *next* call
  returns `LocalFileCacheError::UnsupportedFeature` rather than panicking.
- `AsyncCacheEngine::query_run` / `query_dry_run` regression tests continue
  to pass unchanged after the `EngineCore` + `decode_with` refactor, proving
  no behavior change; add a test querying with a `U` distinct from the
  engine's own `T` to keep the cross-type path explicitly covered.
- A watcher constructed with at least one unwatchable path (e.g. a
  non-existent parent directory) asserts construction still succeeds and
  `registration_errors()` is non-empty and names the failing path.
- A burst of more than 256 invalidation events on a slow consumer asserts
  `dropped_event_count()` increases and that every affected entry was still
  removed from the database (invalidation correctness is independent of
  notification delivery).
- A test that forces `remove` to fail during the watcher callback (e.g. a
  read-only or otherwise unwritable underlying connection) asserts
  `failed_invalidation_count()` increases and that no event is silently lost
  without being counted somewhere.
- The RFC 011 hostile-input suite passes unchanged after `create_path_index`
  / `drop_path_index` / `list_path_indexes` move into `maintenance.rs`,
  proving the security boundary in `indexes.rs` is untouched by R6.
- The existing 351 tests + doctests pass unchanged, proving R6's module
  splits and R1's `EngineCore` refactor introduce no behavioral difference.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`,
  the RFC 009 R7 per-feature matrix (R1/R6 touch `cfg`-gated fields), and the
  declared-MSRV matrix (RFC 014 R8) all stay green.

## Security considerations

R1 removes the only two `unsafe` blocks in the crate's async query path. The
`:343` block (R1a) was never load-bearing — it cast between identical types.
The `:300` block (R1b) relied on unspecified compiler behavior (layout
equivalence across a `PhantomData<T>` substitution that `repr(Rust)` does not
actually guarantee), not a documented invariant. `EngineCore` removes the
need for that argument entirely: the fields it borrows are named and
`T`-independent by construction, so there is nothing left to reinterpret and
no unspecified-behavior dependency remains.

R2 closes a latent availability issue: any single panic inside a locked
`AsyncCacheEngine` operation currently makes the *entire* engine permanently
unusable (every future call panics) for the life of the process, which is a
more severe failure mode than the original panic. This is a correctness fix,
not a new attack surface — `UnsupportedFeature` carries no caller-controlled
data beyond a fixed message.

R4/R5 add read-only diagnostic accessors; neither can be used to influence
cache state or bypass the read-only contract established by RFC 012.

## Compatibility

SemVer-visible additions only: `CacheWatcher::registration_errors`,
`CacheDebouncedWatcher::registration_errors`, `PathRegistrationError`,
`CacheWatcher::dropped_event_count`, `CacheDebouncedWatcher::dropped_event_count`,
`CacheWatcher::failed_invalidation_count`,
`CacheDebouncedWatcher::failed_invalidation_count`.
No existing public item is removed, renamed, or changes signature. No schema
version, payload encoding, or Cargo feature name changes. This RFC's changes
are compatible with a v0.20.1 patch release under the same reasoning RFC
012/013 used: previously-panicking or previously-inconsistent failure paths
are corrected, and new observability is purely additive.

## Alternatives considered

### Fallible watcher constructors (fail the whole construction on any registration error)

Rejected as the default. It changes existing behavior for any caller with a
transiently or permanently unwatchable path among many watched paths —
construction that succeeds today with partial coverage would start failing
entirely. This is exactly the breaking-change risk the M4 close-out review
flagged. The additive diagnostics accessor (R4) closes the observability gap
without that risk. A fallible variant may be proposed later as a new
constructor (e.g. `try_new_strict`) if a caller needs fail-fast semantics;
that is out of scope here.

### New `LocalFileCacheError` variant for mutex poisoning

Rejected. `ConnectionPool` and `CacheWatcher` already solved this with the
existing `UnsupportedFeature` variant; a new variant would be inconsistent
with that precedent, would require deciding whether `error.rs`'s enum
becomes `#[non_exhaustive]` (itself a compatibility question this RFC has no
need to raise), and provides no caller-visible benefit over the existing
variant carrying a descriptive message.

### Backpressure on the watcher event channel

Rejected (see R5). Blocking the `notify` callback thread on a full channel
risks losing OS-level events entirely, which is worse than a counted
application-level drop.

### Splitting `indexes.rs`, `repository.rs`, `query.rs`, or `cli/main.rs` in this RFC

Rejected for M5. `indexes.rs` is a freshly reviewed security boundary
(RFC 011); the other three are not part of the verified M5 scope. Bounding
the module-split work to `engine.rs` and `classifier.rs`'s test placement
avoids the churn-for-no-correctness-gain risk the M4 close-out review warned
against.

## Rollback

Before release, rollback is the ordinary reversal of the source changes in
this RFC; no schema or payload migration is involved. After v0.20.1
publication, a regression in any of R1–R6 is fixed forward in a subsequent
patch release using the same test plan, not by reintroducing the removed
`unsafe` blocks or the discarded registration/drop information.

## Open questions

None. The one design axis flagged by the M4 close-out review as capable of
breaking downstream code — watcher constructor fallibility — is resolved by
R4's additive accessor rather than left open; if a fallible constructor is
wanted later, it is a new, separately proposed API rather than a change to
this RFC's design.
