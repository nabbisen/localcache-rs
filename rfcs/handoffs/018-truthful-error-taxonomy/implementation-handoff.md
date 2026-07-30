# RFC 018 Implementation Handoff — Truthful Error Taxonomy

## 1. Summary

Implement [RFC 018](../../accepted/018-truthful-error-taxonomy.md), Phase 22 **N1**.

RFC 018 is Accepted and **its requirements are authoritative**; this handoff sequences
the work and names the exact sites, but adds no design decision and overrides none.
Where this document and the RFC appear to disagree, **the RFC wins — report the
discrepancy rather than choosing**.

**This is a breaking change targeting v0.21.0.** It changes no schema, no payload wire
format, no SQL, and no database file. Existing caches open unchanged.

Do **not** bump any version in this task. Coming-version housekeeping is a separate
milestone with its own gate, and setting versions early breaks the
version-reference check.

## 2. Change scope, site by site

Every site below was verified present in the tree at the time of writing. Line numbers
will drift; the identifiers will not.

### 2.1 — Make the enum extensible (R1)

`crates/localcache/src/error.rs`: add `#[non_exhaustive]` to `pub enum
LocalFileCacheError`.

This is the load-bearing change. Without it every future variant is breaking.

### 2.2 — Add the `Poisoned` variant (R2)

```rust
/// A lock guarding shared cache state was poisoned by a panic in another
/// thread. The data behind it may reflect a partially completed operation;
/// this error stops the panic propagating to callers who did nothing wrong,
/// and does not attempt to repair state.
#[error("lock poisoned: {resource}")]
Poisoned { resource: &'static str },
```

**Ungated** — no `#[cfg]`. Owner decision; see RFC 018 § "Resolved questions".

`resource` is `&'static str` from a fixed set, **not** a formatted `String`. Using a
`String` here would recreate the `UnsupportedFeature` catch-all problem one level
down, which is the whole point of the RFC.

### 2.3 — Convert the three erroring poison sites (R2)

| File | Site | Current | Becomes |
|---|---|---|---|
| `src/pool.rs` | `ConnectionPool::lock` | `UnsupportedFeature("ConnectionPool mutex was poisoned")` | `Poisoned { resource: "ConnectionPool" }` |
| `src/cache/async_engine.rs` | `AsyncCacheEngine::lock` | `UnsupportedFeature("AsyncCacheEngine mutex was poisoned")` | `Poisoned { resource: "AsyncCacheEngine" }` |
| `src/cache/watcher.rs` | engine lock in the watcher setup path | `UnsupportedFeature("mutex poisoned")` | `Poisoned { resource: "CacheWatcher" }` |

The existing doc comments on `ConnectionPool::lock` and `AsyncCacheEngine::lock`
already state the contract correctly — *"does not attempt to repair engine state"*.
**Keep that wording.** It is the contract the new variant formalises.

### 2.4 — `ReadPool::checkout` becomes fallible (R4) — the largest part

`src/read_pool.rs`, `checkout`, currently ends:

```rust
self.slots[start].lock().unwrap_or_else(|e| e.into_inner())
```

This silently recovers from poisoning and hands the caller state that another thread
abandoned mid-panic. **It must return `Poisoned { resource: "ReadPool" }` instead.**

> **Owner decision, 2026-07-30**, on the principle that *"functions provided by a
> common library like ours should be safe."* The reviewer had recommended keeping the
> current behaviour with a documented rationale; the owner overruled that, and the
> RFC records why. Do not re-litigate it — but if implementation reveals a concrete
> problem the decision did not anticipate, **report it** rather than working around
> it.

**This widens the change.** `checkout` returns `MutexGuard` today, so its callers
assume infallibility. Every read-side method that calls it must propagate `Result`.
Expect to touch most of `read_pool.rs`'s public API surface.

Note the two distinct paths in `checkout`: the `try_lock` loop over slots, and the
blocking fallback. **The `try_lock` loop must keep skipping busy slots** — a busy slot
is not a poisoned one, and turning contention into an error would be a serious
behavioural regression. Only the blocking fallback's poison case becomes an error.

Public signature changes must be reflected in `docs/src/api.md` and any doctest or
example that calls the affected methods.

### 2.5 — Route JSON codec failures to `Serialization` (R3)

`src/serialization.rs`, `serialize_json` and `deserialize_json`: both currently return
`UnsupportedFeature(format!("json … error: {e}"))`. Both must return
`Serialization(...)`, which already exists and is documented for exactly this.

**This is a behaviour change, not a rename.** Code matching `UnsupportedFeature` to
catch JSON errors stops matching. It must be described that way in the changelog — not
as a refactor.

### 2.6 — Document the watcher's deliberate skip (R4)

`src/cache/watcher.rs` has two callback sites using `if let Ok(eng) = …lock()`, which
silently skip work when the lock is poisoned. That is legitimate — they run on a notify
thread with nowhere to return an error — but it is currently implicit in the `if let`.

Add a comment at each stating it is deliberate and why. **Do not change the
behaviour**; there is no caller to return an error to.

## 3. Explicit non-change scope

- Do **not** split `UnsupportedFeature`'s remaining uses (glob, schema configuration)
  into new variants. R6 defers this deliberately; `#[non_exhaustive]` makes it additive
  later.
- Do **not** add `source()` chaining or restructure other variants.
- Do **not** change any schema, migration, SQL, payload encoding, or the read-only
  contract.
- Do **not** bump versions or edit `CHANGELOG.md`'s version heading.
- Do **not** modify the release tooling under `scripts/`.

## 4. Required tests

Beyond the RFC's test plan, all of which is required:

- Poisoning each of `ConnectionPool`, `AsyncCacheEngine`, `CacheWatcher`, and
  `ReadPool` — panic in a holder thread — yields `Poisoned` with the correct
  `resource`, and **not** `UnsupportedFeature`.
- A JSON encode failure and a JSON decode failure each yield `Serialization`.
- **`ReadPool` under contention but with no poisoning still succeeds** — this guards
  §2.4's `try_lock` trap and is the regression most likely to slip through.
- Existing tests asserting `UnsupportedFeature` for poisoning or JSON errors are
  updated. **List every such test in the review request**; do not fold the updates in
  silently.

If the suite has no compile-fail harness, say so rather than skipping the RFC's
exhaustive-match test — an unrun check is a failure, not a pass.

## 5. Required evidence

- Full suite result with counts before and after.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean.
- `cargo fmt --all --check` clean.
- The full declared-MSRV matrix on Rust 1.85 — this touches public signatures.
- `python3 scripts/source_integrity.py --require-tracked` OK.
- The exact list of public signatures changed by §2.4, so the changelog can be written
  from fact rather than reconstruction.

## 6. Known risks

1. **§2.4 is the risky one.** Making `checkout` fallible ripples through `ReadPool`'s
   API. If the ripple reaches further than `read_pool.rs` and its docs — for example
   into `ConnectionPool` or the async engine — **stop and report** before continuing;
   that would exceed what the RFC scoped.
2. **Feature-gated variants.** `AsyncCacheEngine` is behind async features; ensure the
   new tests compile and run under each relevant feature combination, not just the
   default set.
3. **Do not "fix" adjacent `UnsupportedFeature` uses** you will notice in
   `glob.rs`, `schema.rs`, and `schema/configuration.rs`. They are deliberately out of
   scope.

## 7. Recommended order

§2.1 and §2.2 together (enum shape), then §2.3 (mechanical), then §2.5 (small,
independent), then §2.6 (comments only), then §2.4 last — it is the largest and
benefits from the variant already existing.

Prepare one review request identifying the exact commit, mapping each requirement to
its evidence, and disclosing any gate that failed or was not run rather than omitting
it.
