# RFC 0003 — OpenTelemetry Spans

| Field    | Value |
|----------|-------|
| Status   | Implemented (v0.17.0) |
| Feature  | `opentelemetry` (new, additive) |
| Touches  | `Cargo.toml`, `src/cache/engine.rs`, `src/cache/async_engine.rs` |
| Depends on | existing `tracing` feature |

## Summary

Add an `opentelemetry` Cargo feature that bridges the existing `tracing`
instrumentation to the OpenTelemetry ecosystem via
`tracing-opentelemetry`.  No new span sites are added; instead the
existing `tracing` spans become exportable to any OpenTelemetry-compatible
backend (Jaeger, OTLP, stdout, etc.).

## Motivation

The `tracing` feature (Phase 13) already emits `debug_span!` events for
`get`, `set`, and `check_status`.  Users who run localcache in a service
with distributed tracing infrastructure (Datadog, Honeycomb, Jaeger, …)
want these spans to appear in their traces automatically, correlated with
parent spans from the surrounding request context.

`tracing-opentelemetry 0.32` provides a `OpenTelemetryLayer` that
converts `tracing` spans into OTel spans transparently — **no changes to
existing span sites are required**.  This RFC is therefore primarily a
dependency and documentation change.

## Requirements

- Zero additional changes to span call sites in `engine.rs`.
- Opt-in only: `features = ["opentelemetry"]`.
- Must not pull OpenTelemetry dependencies when the feature is disabled.
- Compatible with any OTel exporter the caller chooses (OTLP, Jaeger,
  stdout, …); `localcache` provides the bridge, not the exporter.
- Must not conflict with applications that use `tracing` without OTel.

## Design

### Dependency additions (`Cargo.toml`)

```toml
[workspace.dependencies]
opentelemetry       = { version = "0.31", default-features = false,
                        features = ["trace"] }
tracing-opentelemetry = { version = "0.32", default-features = false }
```

In `[dependencies]` (optional):

```toml
opentelemetry          = { workspace = true, optional = true }
tracing-opentelemetry  = { workspace = true, optional = true }
```

Feature definition:

```toml
## Bridges the existing `tracing` spans to an OpenTelemetry-compatible
## backend via `tracing-opentelemetry`.  Requires the `tracing` feature.
opentelemetry = ["tracing", "dep:opentelemetry", "dep:tracing-opentelemetry"]
```

### No new span sites

`engine.rs` already has (gated behind `#[cfg(feature = "tracing")]`):

```rust
// get()
let _span = tracing::debug_span!("localcache::get",
    path = %path.as_ref().display()).entered();

// set()
let _span = tracing::debug_span!("localcache::set",
    path = %path.as_ref().display()).entered();
```

These spans carry sufficient fields for OTel correlation.  No code
changes to `engine.rs` are required.

### Caller responsibility

The caller installs a `tracing` subscriber that includes
`OpenTelemetryLayer`:

```rust
use opentelemetry::global;
use tracing_subscriber::{layer::SubscriberExt, Registry};
use tracing_opentelemetry::OpenTelemetryLayer;

// One-time setup (application startup):
let tracer = /* configure your exporter */;
let otel_layer = OpenTelemetryLayer::new(tracer);
let subscriber = Registry::default().with(otel_layer);
tracing::subscriber::set_global_default(subscriber).unwrap();

// localcache spans are now exported automatically:
let engine = CacheEngine::<Vec<f32>>::builder()
    .database("cache.sqlite3")
    .build()?;
engine.set("file.txt", &payload)?;  // → OTel span emitted
```

`localcache` itself does not call `set_global_default` — that is always
the application's responsibility.

### Span attributes emitted

| Span name | Attributes |
|---|---|
| `localcache::get` | `path` (display), `namespace` (via tracing field added in this RFC — see below) |
| `localcache::set` | `path`, `bytes` (payload size), `encoding` |
| `localcache::check_status` | `path`, `status`, `reason` |

**Namespace field addition** (small engine.rs change):

To make spans more useful for multi-namespace applications, add
`namespace` to the existing spans:

```rust
// get()
let _span = tracing::debug_span!(
    "localcache::get",
    path = %path.as_ref().display(),
    namespace = %self.namespace,   // ← new
).entered();
```

This change is gated only on `#[cfg(feature = "tracing")]` (not
`opentelemetry`) because it improves plain `tracing` output too.

### Documentation additions

- `docs/src/features.md`: new section for `opentelemetry` feature.
- `docs/src/cookbook.md`: new recipe "Distributed tracing with Jaeger".
- Example: `examples/otel_tracing.rs` demonstrating OTLP stdout exporter.

## Test plan

- Feature compiles with `--features tracing,opentelemetry`.
- Feature compiles with `--features opentelemetry` (implies `tracing`).
- Without either feature: no OTel symbols in binary.
- Smoke test: install an in-memory `tracing` subscriber with a span
  collector; assert `localcache::get` and `localcache::set` spans are
  recorded with expected field names.
- The existing `tracing_feature_no_panic` test passes unchanged.

## Security considerations

OpenTelemetry exporters may transmit span data (including file paths)
to external collectors.  This is expected behaviour for a tracing
integration.  Users should be aware that `path` attributes in spans
contain filesystem paths; redaction is the application's responsibility
at the exporter layer, not `localcache`'s.
