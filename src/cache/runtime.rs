//! Internal async executor abstraction for [`super::async_engine::AsyncCacheEngine`].
//!
//! This module implements RFC 0005: *async-std / smol Feature Variants*.
//!
//! # Feature priority
//!
//! Cargo features must be **additive** — enabling multiple features
//! simultaneously (e.g. `--all-features`, docs.rs) must not cause a
//! compile error.  When more than one async-runtime feature is enabled,
//! this module selects **one backend** using the following priority order:
//!
//! 1. `async` (Tokio) — highest priority
//! 2. `async-std`
//! 3. `smol`
//!
//! Callers that need a specific runtime should enable only that feature.

use crate::error::LocalFileCacheError;

// ---------------------------------------------------------------------------
// SpawnBlocking trait
// ---------------------------------------------------------------------------

/// Internal abstraction over async executor `spawn_blocking`-style APIs.
///
/// Provides a single async method that runs a blocking closure on a thread
/// pool and returns its result.  Each async-runtime feature provides one
/// implementation; the public `spawn_blocking` free function dispatches to
/// whichever is active according to the priority order above.
pub(crate) trait SpawnBlocking {
    async fn spawn<F, R>(f: F) -> Result<R, LocalFileCacheError>
    where
        F: FnOnce() -> Result<R, LocalFileCacheError> + Send + 'static,
        R: Send + 'static;
}

// ---------------------------------------------------------------------------
// Tokio backend
// ---------------------------------------------------------------------------

#[cfg(feature = "async")]
pub(crate) struct TokioRuntime;

#[cfg(feature = "async")]
impl SpawnBlocking for TokioRuntime {
    async fn spawn<F, R>(f: F) -> Result<R, LocalFileCacheError>
    where
        F: FnOnce() -> Result<R, LocalFileCacheError> + Send + 'static,
        R: Send + 'static,
    {
        tokio::task::spawn_blocking(f)
            .await
            .map_err(|_| LocalFileCacheError::AsyncTaskPanicked)?
    }
}

// ---------------------------------------------------------------------------
// async-std backend
// ---------------------------------------------------------------------------

// Only compiled when `async` (Tokio) is NOT active, giving Tokio priority.
#[cfg(all(not(feature = "async"), feature = "async-std"))]
pub(crate) struct AsyncStdRuntime;

#[cfg(all(not(feature = "async"), feature = "async-std"))]
impl SpawnBlocking for AsyncStdRuntime {
    async fn spawn<F, R>(f: F) -> Result<R, LocalFileCacheError>
    where
        F: FnOnce() -> Result<R, LocalFileCacheError> + Send + 'static,
        R: Send + 'static,
    {
        // `async_std::task::spawn_blocking` is stable in async-std 1.13.
        async_std::task::spawn_blocking(f).await
    }
}

// ---------------------------------------------------------------------------
// smol backend
// ---------------------------------------------------------------------------

// Compiled only when neither `async` nor `async-std` is active.
#[cfg(all(not(feature = "async"), not(feature = "async-std"), feature = "smol"))]
pub(crate) struct SmolRuntime;

#[cfg(all(not(feature = "async"), not(feature = "async-std"), feature = "smol"))]
impl SpawnBlocking for SmolRuntime {
    async fn spawn<F, R>(f: F) -> Result<R, LocalFileCacheError>
    where
        F: FnOnce() -> Result<R, LocalFileCacheError> + Send + 'static,
        R: Send + 'static,
    {
        // `smol::unblock` runs the closure on the `blocking` thread pool
        // and is available in smol 2.x without any extra feature flags.
        smol::unblock(f).await
    }
}

// ---------------------------------------------------------------------------
// Public dispatch function — used by async_engine.rs
// ---------------------------------------------------------------------------

/// Run `f` on a blocking thread pool using whichever async runtime is active.
///
/// Priority: Tokio → async-std → smol (see module docs).
#[cfg(any(feature = "async", feature = "async-std", feature = "smol"))]
pub(crate) async fn spawn_blocking<F, R>(f: F) -> Result<R, LocalFileCacheError>
where
    F: FnOnce() -> Result<R, LocalFileCacheError> + Send + 'static,
    R: Send + 'static,
{
    // Exactly one branch compiles per build thanks to the cfg guards on the
    // backend structs above — the others are dead-code-eliminated.
    #[cfg(feature = "async")]
    {
        TokioRuntime::spawn(f).await
    }
    #[cfg(all(not(feature = "async"), feature = "async-std"))]
    {
        AsyncStdRuntime::spawn(f).await
    }
    #[cfg(all(not(feature = "async"), not(feature = "async-std"), feature = "smol"))]
    {
        SmolRuntime::spawn(f).await
    }
}
