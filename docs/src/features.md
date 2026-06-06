# Features

All optional capabilities are gated behind Cargo feature flags so you pay
only for what you use.

```toml
[dependencies]
localcache = { version = "0.17", features = ["async", "compression", "json"] }
```

## Feature reference

| Feature | Description | Key types / functions |
|---|---|---|
| *(default)* | Core cache — bincode payloads, BLAKE3, SQLite | `CacheEngine<T>`, `ConnectionPool<T>` |
| `async` | Tokio-based async wrapper | `AsyncCacheEngine<T>` |
| `async-std` | async-std async wrapper | `AsyncCacheEngine<T>` |
| `smol` | smol async wrapper | `AsyncCacheEngine<T>` |
| `compression` | zstd payload compression | `CacheOptions::compress_payloads` |
| `json` | JSON codec + `QueryBuilder` predicates | `Codec::Json`, `engine.query()` |
| `encryption` | AES-256-GCM payload encryption | `CacheEngineBuilder::encryption_key()` |
| `tracing` | `tracing` spans on hot paths (with `namespace` field) | automatic; zero-cost when disabled |
| `opentelemetry` | OTel bridge via `tracing-opentelemetry` (implies `tracing`) | caller installs `OpenTelemetryLayer` |
| `watching` | OS file-system events for reactive invalidation | `CacheWatcher<T>`, `CacheDebouncedWatcher<T>` |
| `metrics` | `metrics` counters and histograms | automatic; zero-cost when disabled |

> **Async runtime priority:** when more than one async feature is enabled
> simultaneously, the active backend is chosen by priority:
> `async` (Tokio) > `async-std` > `smol`.  Features remain additive for
> `--all-features` compatibility.

## async

Requires [Tokio](https://tokio.rs/).  Wraps every blocking call in
`tokio::task::spawn_blocking` so the async executor is never starved.

```rust
use localcache::{AsyncCacheEngine, CacheOptions};

let engine = AsyncCacheEngine::<Vec<f32>>::open(CacheOptions {
    database_path: "cache.sqlite3".into(),
    ..Default::default()
}).await?;

engine.set(path.into(), payload).await?;
let entry = engine.get_if_fresh(path.into()).await?;
```

`AsyncCacheEngine` is `Clone` — all clones share the same engine.

## compression

Payloads are compressed with **zstd level 3** before storage.  Useful for
large embeddings or documents where disk space matters more than write speed.

```rust
let engine = CacheEngine::<Vec<f32>>::builder()
    .database("cache.sqlite3")
    .compress()          // requires feature = "compression"
    .build()?;
```

Encoding tags stored in the database reflect the compression state
(`"raw"` vs `"zstd"`), so mixing compressed and uncompressed entries in
the same namespace works correctly.

## json

Switches the serialisation codec to **JSON** (via `serde_json`) and unlocks
payload-field predicates in `QueryBuilder`:

```rust
use localcache::{CacheEngine, Codec};

let engine = CacheEngine::<MyDoc>::builder()
    .database("cache.sqlite3")
    .codec(Codec::Json)
    .build()?;

// Filter by a field inside the payload.
let results = engine.query()
    .field_gt("score", 0.9)
    .order_by_field("score", false) // descending
    .limit(10)
    .run()?;
```

## encryption

Payloads are encrypted with **AES-256-GCM** before storage.  A fresh
96-bit nonce is generated per write and prepended to the ciphertext.

```rust
let key: Vec<u8> = /* 32 random bytes */;

let engine = CacheEngine::<Vec<f32>>::builder()
    .database("cache.sqlite3")
    .encryption_key(key)   // requires feature = "encryption"
    .build()?;
```

Key rotation is supported via `engine.rotate_encryption_key(&new_key)`.

> **Warning**: losing the encryption key makes all encrypted entries
> permanently unreadable.

## tracing

When enabled, `get`, `set`, and `check_status` emit `tracing::debug_span!`
events with path, namespace, hit/miss status, byte counts, and staleness reasons.
Compatible with any `tracing` subscriber (e.g. `tracing-subscriber`,
`tokio-console`).

No code changes are required; all spans are compiled out when the feature
is disabled.

## opentelemetry

Bridges the existing `tracing` instrumentation to any OpenTelemetry-compatible
backend (Jaeger, Honeycomb, OTLP, stdout, …) via
[`tracing-opentelemetry`](https://crates.io/crates/tracing-opentelemetry).

Implies `tracing`.  No new span sites are added — the existing spans become
exportable automatically once the caller installs an `OpenTelemetryLayer`:

```rust
use tracing_subscriber::{layer::SubscriberExt, Registry};
use tracing_opentelemetry::OpenTelemetryLayer;

let tracer = /* configure your OTLP / Jaeger / stdout exporter */;
let subscriber = Registry::default().with(OpenTelemetryLayer::new(tracer));
tracing::subscriber::set_global_default(subscriber).unwrap();

// localcache spans (get / set / check_status) now appear in your traces:
let engine = CacheEngine::<Vec<f32>>::builder()
    .database("cache.sqlite3")
    .build()?;
engine.set("file.txt", &payload)?;  // → OTel span emitted
```

`localcache` itself never calls any OTel API — exporter setup is always the
application's responsibility.

> **Path data in spans:** `path` span attributes contain filesystem paths.
> If your exporter sends traces to a remote collector, redact sensitive paths
> at the exporter layer.

## watching

Provides reactive cache invalidation using OS-native file-system events
(`inotify` / `kqueue` / `ReadDirectoryChanges`).

```rust
let watcher = engine.watcher()?;  // auto-registers all cached paths
let rx = watcher.events();

for event in rx.iter() {
    println!("invalidated: {} ({:?})", event.path.display(), event.reason);
}
// watcher must stay alive — dropping it stops the OS watcher
```

For rapid write scenarios, use `debounced_watcher`:

```rust
use std::time::Duration;
let watcher = engine.debounced_watcher(Duration::from_millis(300))?;
```

### Recursive directory watching (v0.17.0)

Instead of one OS watch per cached file, you can watch an entire directory
subtree.  Events for files *not* in the cache are filtered out automatically.

```rust
// Option 1: watch a directory explicitly after creating the watcher
let mut watcher = engine.watcher()?;
watcher.watch_dir("/data/documents")?;

// Option 2: builder flag — watcher() auto-registers parent directories
let engine = CacheEngine::<Vec<f32>>::builder()
    .database("cache.sqlite3")
    .watch_dirs(true)    // one OS watch per directory, not per file
    .build()?;
let watcher = engine.watcher()?;

// To stop watching a subtree:
watcher.unwatch_dir("/data/old")?;
```

Both `CacheWatcher` and `CacheDebouncedWatcher` expose `watch_dir` /
`unwatch_dir`.  Recursive and per-file registrations can coexist on the
same watcher.

## metrics

When enabled, `get` and `set` emit counters and histograms via the
[`metrics`](https://crates.io/crates/metrics) facade.  Wire up any
compatible recorder — Prometheus, StatsD, in-memory, etc.

| Metric | Type | Labels |
|---|---|---|
| `localcache.get.total` | counter | `namespace` |
| `localcache.get.hit` | counter | `namespace` |
| `localcache.get.miss` | counter | `namespace` |
| `localcache.set.total` | counter | `namespace` |
| `localcache.set.bytes` | histogram | `namespace` |
