# Features

All optional capabilities are gated behind Cargo feature flags so you pay
only for what you use.

```toml
[dependencies]
localcache = { version = "0.26.2", features = ["async", "compression", "json"] }
```

## Feature reference

| Feature | Description | Key types / functions |
|---|---|---|
| *(default)* | Core cache — compact binary payloads (`Codec::Bincode`), BLAKE3, SQLite | `CacheEngine<T>`, `SyncCacheEngine<T>` |
| `async` | Tokio-based async wrapper | `AsyncCacheEngine<T>` |
| `smol` | smol async wrapper | `AsyncCacheEngine<T>` |
| `compression` | zstd payload compression | `CacheOptions::compress_payloads` |
| `json` | JSON codec + `QueryBuilder` predicates | `Codec::Json`, `engine.query()` |
| `encryption` | AES-256-GCM payload encryption | `CacheEngineBuilder::encryption_key()` |
| `tracing` | `tracing` spans on hot paths (with `namespace` field) | automatic; zero-cost when disabled |
| `watching` | OS file-system events for reactive invalidation | `CacheWatcher<T>`, `CacheDebouncedWatcher<T>` |
| `metrics` | `metrics` counters and histograms | automatic; zero-cost when disabled |

> **Async runtime priority:** when more than one async feature is enabled
> simultaneously, the active backend is chosen by priority:
> `async` (Tokio) > `smol`.  Features remain additive for
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

### The decompression limit

A few kilobytes of compressed data can expand to gigabytes, so decompression
is bounded: `CacheOptions::max_decompressed_bytes` (builder
`.max_decompressed_bytes(n)`, default `DEFAULT_MAX_DECOMPRESSED_BYTES`, **64 MiB**)
is the largest size a compressed payload may have once decompressed.

- **On read**, a stored compressed entry that expands past the limit returns
  `LocalFileCacheError::PayloadTooLarge { limit }`. It is refused as soon as
  the limit is passed (or, if the frame header declares a size above it, before
  anything is decompressed), so memory stays near the limit however much the
  data would have expanded to.
- **On write**, a payload that is about to be compressed and is larger than the
  limit returns the same error, so an engine never stores what it would refuse to
  read.
- **Uncompressed payloads are not limited**: they are exactly as large as the
  stored blob. Each engine has its own limit; `0` is rejected when it is opened.

If you store compressed payloads larger than 64 MiB once decompressed, raise the
option on the engines that read and write them.

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
    .order_by(SortKey::Field("score".into()), SortOrder::Desc) // descending
    .limit(10)
    .run()?;
```

## encryption

Payloads are encrypted with **AES-256-GCM** before storage.  A fresh
96-bit nonce is generated per write and prepended to the ciphertext. Each
payload is **bound to its entry's path** (the path and the encoding tag are
authenticated along with the ciphertext), so a ciphertext copied onto another
row does not open. With a key, an engine reads only such bound entries; see
[Security Model](./security.md#encryption).

```rust
let key: Vec<u8> = /* 32 random bytes */;

let engine = CacheEngine::<Vec<f32>>::builder()
    .database("cache.sqlite3")
    .encryption_key(key)   // requires feature = "encryption"
    .build()?;
```

`engine.rotate_encryption_key(&new_key)` seals **every** entry of the engine's namespace under
`new_key`, bound to its path, in pages and in one atomic transaction: memory is bounded by the
page (1,000 rows or 8 MiB), not by the namespace. `new_key` may equal the current key. That is
the **upgrade** of a database written by v0.23 or earlier (call it once with the same key), and
how encryption is switched on for a database written without a key (open with the key, rotate
to it). It returns the number of entries sealed. The write-ahead log grows by about the size of
the namespace's payloads while it runs, and a failure changes nothing. Afterwards an engine without a key
gets `MissingEncryptionKey` for every entry of the namespace, a formerly plain one included. If a rotation
fails with `EncryptionError`, one entry could not be opened and the error does not name it: list it with
`query().run_report()`, `remove` the entries listed with an `EncryptionError`, and rotate again (see
[Migration](./migration.md#encryption-integrity-v0240)).

> **Warning**: losing the encryption key makes all encrypted entries
> permanently unreadable.

> **Scope**: this encrypts **payload content only**. Paths, namespaces, file
> sizes, modification times, content hashes, and timestamps are stored
> unencrypted, and anyone who can read the database file can read them.
> Enabling this feature does not encrypt the database file.

## tracing

When enabled, `get`, `set`, and `check_status` emit `tracing::debug_span!`
events with path, namespace, hit/miss status, byte counts, and staleness reasons.
Compatible with any `tracing` subscriber (e.g. `tracing-subscriber`,
`tokio-console`).

No code changes are required; all spans are compiled out when the feature
is disabled.

To export the spans to an OpenTelemetry backend, add `tracing-opentelemetry` to your own
program and install its layer in your subscriber; [Recipe 9 of the cookbook](./cookbook.md)
shows it. `localcache` itself never calls any OpenTelemetry API.

> **Path data in spans:** `path` span attributes contain filesystem paths.
> If your exporter sends traces to a remote collector, redact sensitive paths
> at the exporter layer.

## watching

Provides reactive cache invalidation using OS-native file-system events
(`inotify` / `FSEvents` / `ReadDirectoryChanges`).

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
