# RFC 018 — Truthful Error Taxonomy and a Forward-Compatible Error Enum

| Field | Value |
|---|---|
| Status | Accepted |
| Feature | *(no Cargo feature; affects the public error type)* |
| Touches | `crates/localcache/src/error.rs`, `pool.rs`, `read_pool.rs`, `serialization.rs`, `cache/async_engine.rs`, `cache/watcher.rs`, `docs/src/errors.md` |
| Finding | Phase 21 deferred debt, plus three defects found while designing this RFC |
| Milestone | Phase 22 N1 |
| Breaking | **Yes** — requires v0.21.0 |

## Summary

Make `LocalFileCacheError` tell the truth about what went wrong, and make it
extensible without a further breaking change:

1. add `#[non_exhaustive]` so future variants are additive;
2. add a distinct `Poisoned` variant, replacing three sites that report lock
   poisoning as `UnsupportedFeature`;
3. route JSON serialization failures to the existing `Serialization` variant
   instead of `UnsupportedFeature`;
4. settle and document **one** mutex-poisoning policy, which the crate currently
   does not have.

This is a v0.21.0 change. It alters no schema, payload wire format, SQL, or
public method signature.

## Motivation

### The deferred item

Phase 21 recorded two deferred items: `LocalFileCacheError` is not
`#[non_exhaustive]`, and poisoning shares `UnsupportedFeature`. Both are breaking
to fix, so both were held for a minor release.

`#[non_exhaustive]` is the load-bearing half. Without it, **every future variant is
a breaking change**, because downstream `match` expressions are exhaustive. The enum
already has 11 variants, two of them feature-gated, and this RFC adds a twelfth. If
it is not made extensible now, the next diagnostic improvement pays the same tax.

### Three defects found while designing this

Reading every construction site turned up more than the recorded debt.

**D1 — JSON errors claim to be unsupported features.** `serialization.rs:212,219`:

```rust
serde_json::to_vec(payload)
    .map_err(|e| LocalFileCacheError::UnsupportedFeature(format!("json serialization error: {e}")))
```

A `Serialization(String)` variant already exists and is documented as covering
"serialization or deserialization error". A malformed payload is not an unsupported
feature. A user matching `Serialization` to handle encoding failures **will not
catch JSON failures today**, which is a correctness problem for their error
handling, not a cosmetic one.

**D2 — the crate has three different poisoning policies.** Four sites, no shared
contract:

| Site | Behaviour |
|---|---|
| `pool.rs:333` `ConnectionPool::lock` | error — `UnsupportedFeature("ConnectionPool mutex was poisoned")` |
| `cache/async_engine.rs:66` | error — `UnsupportedFeature("AsyncCacheEngine mutex was poisoned")` |
| `cache/watcher.rs:152` | error — `UnsupportedFeature("mutex poisoned")` |
| `read_pool.rs:161` | **silently recovers** via `unwrap_or_else(\|e\| e.into_inner())` |
| `cache/watcher.rs:179,481` | **silently skips** the callback via `if let Ok(...)` |

Three error sites with three different message strings, one silent recovery, and
two silent skips. `ConnectionPool` and `AsyncCacheEngine` document their contract
deliberately and their reasoning is sound — *"this only stops the panic from
propagating to callers who did nothing wrong — it does not attempt to repair engine
state."* The problem is that the other three sites do not follow it and do not say
why.

**D3 — `UnsupportedFeature` has become the crate's catch-all.** It currently carries
glob malformation, schema-configuration rejection, JSON codec errors, unknown
payload encodings, and lock poisoning. A variant that means five unrelated things
means nothing, and callers cannot act on it.

## Requirements

### R1 — `LocalFileCacheError` is `#[non_exhaustive]`

Downstream exhaustive matches must add a `_` arm. This is the breaking change that
justifies v0.21.0, and it must land in the same release as R2 so users absorb one
break rather than two.

### R2 — A distinct `Poisoned` variant

```rust
/// A lock guarding shared cache state was poisoned by a panic in another
/// thread. The data behind it may reflect a partially completed operation;
/// this error stops the panic propagating to callers who did nothing wrong,
/// and does not attempt to repair state.
#[error("lock poisoned: {resource}")]
Poisoned { resource: &'static str },
```

`resource` is a `&'static str` naming the guarded structure — `"ConnectionPool"`,
`"AsyncCacheEngine"`, `"CacheWatcher"` — not a free-form message. A fixed set keeps
it matchable; a `String` would recreate the `UnsupportedFeature` problem one level
down.

The three error sites in D2 must construct this variant. Their current message
strings stop being load-bearing.

### R3 — JSON failures use `Serialization`

`serialize_json` and `deserialize_json` must return `Serialization`. This is a
**behaviour change**, not only a rename: code matching `UnsupportedFeature` to catch
JSON errors will stop matching. It must appear in the changelog and migration note
as such, and not be described as a pure refactor.

### R4 — One documented poisoning policy

The crate must state one policy and follow it, or state explicitly where it
deliberately differs and why.

Proposed policy: **report, do not repair.** Any site that can surface a poisoned
lock to a caller returns `Poisoned`. Two exceptions must be documented **at the call
site**, not merely permitted:

- **`ReadPool::checkout` (`read_pool.rs:161`)** currently calls `into_inner()` and
  recovers silently.

  > **Owner decision, 2026-07-30: it must return `Poisoned`.** The governing
  > principle given was that *"functions provided by a common library like ours
  > should be safe."* Silent recovery is the unsafe option: `into_inner()` hands a
  > caller state that another thread abandoned mid-panic, and the caller has no way
  > to know. A library cannot make that judgement on a dependant's behalf. The
  > convenience argument — that a `ReadPool` only mutates `last_accessed_at`, so the
  > engine is *probably* consistent — rests on "probably", which is not a basis for
  > a safety default.
  >
  > `ReadPool::checkout` therefore becomes fallible. This widens the change: its
  > callers currently assume an infallible checkout, so their signatures must
  > propagate `Result`. That is a larger break than the enum change alone, and it is
  > deliberate. **The reviewer had recommended keeping current behaviour with
  > documentation; the owner overruled that on safety grounds, and the owner is
  > right.**
- **`CacheWatcher` callbacks (`watcher.rs:179,481`)** skip work on a poisoned lock
  because they run on a notify thread with nowhere to return an error. That is
  legitimate, and must be stated in the code rather than left implicit in an
  `if let Ok`.

### R5 — Migration note

`CHANGELOG.md` and `docs/src/errors.md` must state, for a 0.20.x user:

- add a `_` arm to exhaustive matches on `LocalFileCacheError`;
- lock poisoning now returns `Poisoned`, not `UnsupportedFeature`;
- JSON codec failures now return `Serialization`, not `UnsupportedFeature`;
- no schema, payload, or method-signature change — recompiling is the only work.

### R6 — Non-goals

Out of scope, deliberately: splitting `UnsupportedFeature`'s remaining uses (glob
and schema configuration) into their own variants; adding `source()` chaining;
replacing `String` payloads with structured fields elsewhere. Each is defensible
and none is needed to close the deferred item. **R1 makes all of them additive
later**, which is the point of doing R1 first.

## Design

`#[non_exhaustive]` on the enum; one new variant; three construction sites changed
to `Poisoned`; two changed to `Serialization`; two documented as deliberate
exceptions. No control flow changes and no new dependency.

The `Poisoned { resource: &'static str }` shape is chosen over a unit variant so a
caller can distinguish which subsystem failed, and over `String` so the set stays
closed and matchable. A unit variant would lose information the current message
strings already carry.

## Test plan

- Poisoning each of `ConnectionPool`, `AsyncCacheEngine`, and `CacheWatcher` — panic
  in a holder thread — yields `Poisoned` with the right `resource`, and **not**
  `UnsupportedFeature`.
- A JSON encode failure and a JSON decode failure each yield `Serialization`.
- Whichever `ReadPool` policy R4 selects is covered by a test asserting that
  behaviour, so the decision cannot silently regress.
- An exhaustive `match` without a `_` arm fails to compile — a compile-fail test, or
  an explicit note if the suite has no such harness.
- Existing tests that assert `UnsupportedFeature` for poisoning or JSON errors are
  updated, and the update is called out in review rather than folded in silently.

## Security considerations

Neutral to mildly positive. No change to the read-only contract, SQL construction,
or encryption. `Poisoned` narrows what `UnsupportedFeature` conveys, which slightly
reduces the chance a caller treats a poisoned lock as a benign configuration
problem. `resource` is a fixed `&'static str` and cannot leak path or payload data.

## Compatibility

**Breaking; requires v0.21.0.** No schema, payload wire format, SQL, or method
signature changes; existing databases open unchanged. The break is confined to
downstream `match` exhaustiveness and to any code matching `UnsupportedFeature` for
poisoning or JSON errors.

## Resolved questions

Both were settled by the owner on 2026-07-30 under one principle: **a common library
should be safe by default.**

1. **`ReadPool`'s silent poison recovery** — resolved in R4: it must return
   `Poisoned`. `ReadPool::checkout` becomes fallible and its callers propagate
   `Result`.
2. **Should `Poisoned` be feature-gated?** **No — ungated.** The same principle
   applies: a variant that appears and disappears with feature selection makes a
   caller's exhaustive handling depend on which features some other crate in the
   graph enabled. An occasionally-unconstructed variant is the safer failure mode.
   `ConnectionPool` is ungated, so the variant is constructible in every build
   anyway.

## Review record

**This RFC was authored by the reviewer who would ordinarily review it** — the
conflict recorded against RFC 017 at M7 §6. The owner resolved it by reviewing and
accepting the RFC directly on 2026-07-30, and by overruling the reviewer's
recommendation on R4. That is the separation this project's process needs here, and
it worked: the owner's ruling changed the design, and made it stricter.
