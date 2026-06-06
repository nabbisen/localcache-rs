# RFC 0005 — async-std / smol Feature Variants

| Field    | Value |
|----------|-------|
| Status   | Implemented (v0.17.0) |
| Feature  | `async-std` (new), `smol` (new) |
| Touches  | `Cargo.toml`, `src/cache/async_engine.rs` (refactor), new `src/cache/runtime.rs` |
| Depends on | existing `async` feature (Tokio) |

## Summary

Extract the async executor abstraction behind a `Runtime` trait so that
`AsyncCacheEngine` can run on **async-std** or **smol** in addition to the
existing Tokio backend.  Each runtime is gated behind its own Cargo
feature.  The `async` feature continues to mean Tokio; new features
`async-std` and `smol` activate the alternative backends.

## Motivation

`AsyncCacheEngine` currently hard-codes
`tokio::task::spawn_blocking`.  Projects using async-std or smol (common
in embedded and WASM-adjacent contexts) cannot use `AsyncCacheEngine`
without also pulling in Tokio.

async-std 1.13 and smol 2.x both provide `spawn_blocking`-equivalent
APIs, making the feature feasible with a thin abstraction layer.

## Requirements

- The existing `async` (Tokio) feature remains unchanged in name and
  behaviour.
- `async-std` and `smol` features are mutually exclusive with `async`.
  Enabling more than one runtime feature is a compile error.
- `AsyncCacheEngine<T>` remains the single public type regardless of
  which runtime is active.
- No runtime code in the common cache path (zero-cost when none of the
  async features is enabled).

## Design

### Feature definitions (`Cargo.toml`)

```toml
## Async backend — Tokio (existing).
async     = ["dep:tokio"]

## Async backend — async-std.  Mutually exclusive with `async` and `smol`.
async-std = ["dep:async-std-crate"]

## Async backend — smol.  Mutually exclusive with `async` and `smol`.
smol      = ["dep:smol-crate"]
```

New optional workspace dependencies:

```toml
async-std-crate = { package = "async-std", version = "1.13",
                    features = ["attributes"], optional = true }
smol-crate      = { package = "smol",      version = "2",   optional = true }
```

Mutual-exclusion guard (compile-time) in `src/lib.rs`:

```rust
#[cfg(all(feature = "async", feature = "async-std"))]
compile_error!(
    "features `async` (Tokio) and `async-std` are mutually exclusive"
);
#[cfg(all(feature = "async", feature = "smol"))]
compile_error!(
    "features `async` (Tokio) and `smol` are mutually exclusive"
);
#[cfg(all(feature = "async-std", feature = "smol"))]
compile_error!(
    "features `async-std` and `smol` are mutually exclusive"
);
```

### New `src/cache/runtime.rs`

A thin internal trait that abstracts `spawn_blocking`:

```rust
use std::future::Future;
use crate::error::LocalFileCacheError;

/// Internal abstraction over async executor spawn_blocking.
pub(crate) trait SpawnBlocking {
    /// Run `f` on a thread pool and await its completion.
    async fn spawn<F, R>(f: F) -> Result<R, LocalFileCacheError>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static;
}

// ── Tokio backend ────────────────────────────────────────────────────
#[cfg(feature = "async")]
pub(crate) struct TokioRuntime;
#[cfg(feature = "async")]
impl SpawnBlocking for TokioRuntime {
    async fn spawn<F, R>(f: F) -> Result<R, LocalFileCacheError>
    where F: FnOnce() -> R + Send + 'static, R: Send + 'static {
        tokio::task::spawn_blocking(f)
            .await
            .map_err(|_| LocalFileCacheError::AsyncTaskPanicked)
    }
}

// ── async-std backend ─────────────────────────────────────────────────
#[cfg(feature = "async-std")]
pub(crate) struct AsyncStdRuntime;
#[cfg(feature = "async-std")]
impl SpawnBlocking for AsyncStdRuntime {
    async fn spawn<F, R>(f: F) -> Result<R, LocalFileCacheError>
    where F: FnOnce() -> R + Send + 'static, R: Send + 'static {
        Ok(async_std::task::spawn_blocking(f).await)
    }
}

// ── smol backend ──────────────────────────────────────────────────────
#[cfg(feature = "smol")]
pub(crate) struct SmolRuntime;
#[cfg(feature = "smol")]
impl SpawnBlocking for SmolRuntime {
    async fn spawn<F, R>(f: F) -> Result<R, LocalFileCacheError>
    where F: FnOnce() -> R + Send + 'static, R: Send + 'static {
        Ok(smol::unblock(f).await)
    }
}
```

### `src/cache/async_engine.rs` changes

Replace the direct `tokio::task::spawn_blocking` call with a call to the
active runtime:

```rust
// Current (Tokio-only):
pub(crate) async fn spawn<F, R>(f: F) -> Result<R, LocalFileCacheError>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| LocalFileCacheError::AsyncTaskPanicked)
}

// New (runtime-agnostic):
#[cfg(any(feature = "async", feature = "async-std", feature = "smol"))]
pub(crate) async fn spawn<F, R>(f: F) -> Result<R, LocalFileCacheError>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    crate::cache::runtime::spawn_blocking(f).await
}
```

`crate::cache::runtime::spawn_blocking` is a top-level async function
that dispatches to the active runtime via `#[cfg]`:

```rust
pub(crate) async fn spawn_blocking<F, R>(f: F) -> Result<R, LocalFileCacheError>
where F: FnOnce() -> R + Send + 'static, R: Send + 'static {
    #[cfg(feature = "async")]
    { TokioRuntime::spawn(f).await }
    #[cfg(feature = "async-std")]
    { AsyncStdRuntime::spawn(f).await }
    #[cfg(feature = "smol")]
    { SmolRuntime::spawn(f).await }
}
```

### `cache.rs` module declaration

```rust
#[cfg(any(feature = "async", feature = "async-std", feature = "smol"))]
pub(crate) mod async_engine;
#[cfg(any(feature = "async", feature = "async-std", feature = "smol"))]
pub(crate) mod runtime;
```

### `lib.rs` re-export

```rust
#[cfg(any(feature = "async", feature = "async-std", feature = "smol"))]
pub use cache::async_engine::AsyncCacheEngine;
```

### `AsyncCacheEngine::open` signature

Currently requires `CacheOptions`.  No change to the public API.

### CI matrix additions (`.github/workflows/ci.yaml`)

Add `"async-std"` and `"smol"` to the feature matrix:

```yaml
features:
  - ""
  - "async"
  - "async-std"        # ← new
  - "smol"             # ← new
  - "compression"
  # … etc.
```

## Test plan

- `AsyncCacheEngine` with `async-std` feature: all existing async tests
  adapted to use `async_std::test` attribute.
- `AsyncCacheEngine` with `smol` feature: all async tests run under `smol`.
- Mutual-exclusion `compile_error!`: confirmed via `trybuild` test or
  build-script check.
- No Tokio symbols appear when only `async-std` is enabled (verify with
  `cargo tree --features async-std`).
- MSRV (1.85) still satisfied with all three runtime features individually.

## Open questions

1. **smol `unblock` vs `blocking`**: smol provides both `smol::unblock`
   (runs on a thread pool) and `blocking::unblock` (same pool, separate
   crate).  RFC uses `smol::unblock` — confirm it is available in smol 2.x
   without `blocking` crate.

2. **async-std test macro**: `async_std::test` attribute works out of the
   box; Tokio tests use `#[tokio::test]`.  Consider a
   `#[async_test]` proc-macro wrapper in `tests/common` to unify.
   Deferred to implementation.

3. **`watching` + non-Tokio runtimes**: `CacheWatcher` uses only `std`
   threads and `mpsc` channels — no async runtime dependency.  Compatible
   as-is.

## Implementation notes (v0.17.0)

### Precedence-based dispatch instead of `compile_error!`

The RFC specified that enabling more than one async-runtime feature
simultaneously should be a compile error.  This conflicts with two hard
project requirements:

- **`--all-features` quality gate** — `cargo clippy --all-features` and
  `cargo test --all-features` must succeed; mutual-exclusion
  `compile_error!` would break every CI run that uses `--all-features`.
- **`docs.rs` build** — `[package.metadata.docs.rs] all-features = true`
  means docs.rs enables all features simultaneously; a `compile_error!`
  would produce a broken documentation page.

Cargo's feature model requires features to be **additive**.

**Resolution:** when multiple runtime features are enabled, a single
backend is selected by fixed priority order (Tokio > async-std > smol)
via `#[cfg]` guards in `src/cache/runtime.rs`.  The `RFC_0005_PRIORITY`
comment in that module documents the order explicitly.  Callers who need
a specific runtime should enable only that feature.

### `smol::unblock` confirmed in smol 2.x

Open question 1 from the RFC: `smol::unblock` is available directly in
smol 2.0.x without the `blocking` crate.  Confirmed via probe build.

### async-std `spawn_blocking` stable in 1.13

Open question 2: `async_std::task::spawn_blocking` is stable in
async-std 1.13 (the `unstable` feature is no longer required).
