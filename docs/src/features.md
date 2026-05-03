# Features

All optional capabilities are gated behind Cargo feature flags so you pay
only for what you use.

```toml
[dependencies]
localcache = { version = "0.15", features = ["async", "compression", "json"] }
```

## Feature reference

| Feature | Description | Key types / functions |
|---|---|---|
| *(default)* | Core cache — bincode payloads, BLAKE3, SQLite | `CacheEngine<T>`, `ConnectionPool<T>` |
| `async` | Tokio-based async wrapper | `AsyncCacheEngine<T>` |
| `compression` | zstd payload compression | `CacheOptions::compress_payloads` |
| `json` | JSON codec + `QueryBuilder` predicates | `Codec::Json`, `engine.query()` |
| `encryption` | AES-256-GCM payload encryption | `CacheEngineBuilder::encryption_key()` |
| `tracing` | `tracing` spans on hot paths | automatic; zero-cost when disabled |
| `watching` | OS file-system events for reactive invalidation | `CacheWatcher<T>`, `CacheDebouncedWatcher<T>` |
| `metrics` | `metrics` counters and histograms | automatic; zero-cost when disabled |

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
events with path, hit/miss status, byte counts, and staleness reasons.
Compatible with any `tracing` subscriber (e.g. `tracing-subscriber`,
`tokio-console`).

No code changes are required; all spans are compiled out when the feature
is disabled.

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
