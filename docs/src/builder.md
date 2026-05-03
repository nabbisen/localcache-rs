# Builder API

`CacheEngine::builder()` provides a fluent interface for configuring a cache.
It is the recommended way to open an engine.

```rust
use std::time::Duration;
use localcache::{CacheEngine, ChangeDetectionMode};

let engine = CacheEngine::<Vec<f32>>::builder()
    .database("cache.sqlite3")       // path or ":memory:"
    .namespace("embeddings")         // logical partition
    .change_detection(ChangeDetectionMode::MetadataThenFullHash)
    .ttl(Duration::from_secs(3600))  // entries expire after 1 h
    .max_entries(10_000)             // LRU eviction limit
    .payload_version(2)              // invalidate old-format entries
    .on_evict(|path| eprintln!("evicted: {}", path.display()))
    .build()?;
```

## All options

### `database(path)`

Path to the SQLite file, or `":memory:"` for an in-process ephemeral
database.  The file is created automatically if it does not exist.

**Default**: `"localcache.sqlite3"`

### `namespace(name)`

Logical partition inside the database.  Multiple namespaces can coexist
in one file — useful for versioned caches or separating concerns.

```rust
let embeddings = CacheEngine::<Vec<f32>>::builder()
    .database("cache.sqlite3")
    .namespace("embeddings-v2")
    .build()?;

let thumbnails = CacheEngine::<Vec<u8>>::builder()
    .database("cache.sqlite3")
    .namespace("thumbnails")
    .build()?;
```

**Default**: `"default"`

### `change_detection(mode)`

Selects the algorithm used to decide whether a cached entry is still valid.
See [Change Detection](./change_detection.md) for full details.

**Default**: `ChangeDetectionMode::MetadataOnly`

### `codec(codec)`

Selects the payload serialisation format.  `Codec::Json` requires the
`json` feature and enables `QueryBuilder` predicates.

**Default**: `Codec::Bincode`

### `ttl(duration)` / `no_ttl()`

Time-to-live for cache entries.  Entries older than `duration` are treated
as stale by `get_if_fresh` and `check_status`.

### `max_entries(n)`

Cap the namespace at `n` entries.  When the limit is exceeded after a
`set`, the **least recently accessed** entries are removed (true LRU based
on `last_accessed_at`).

### `payload_version(v)`

Schema version tag.  When `v > 0`, entries written with a different version
are treated as stale.  Bump this when your computation logic changes to
invalidate all old cached results.

### `on_evict(callback)`

Called with the canonical path of each entry removed by the `max_entries`
LRU policy.  The callback runs synchronously after the deletion.

```rust
let evicted = Arc::new(Mutex::new(Vec::new()));
let evicted2 = Arc::clone(&evicted);

let engine = CacheEngine::<Vec<f32>>::builder()
    .database(":memory:")
    .max_entries(100)
    .on_evict(move |path| evicted2.lock().unwrap().push(path.to_path_buf()))
    .build()?;
```

### `read_only()`

Opens the database in read-only mode.  Write operations (`set`, `remove`,
etc.) return `LocalFileCacheError::ReadOnly`.

### Feature-gated options

| Method | Feature | Description |
|---|---|---|
| `.compress()` | `compression` | Enable zstd payload compression |
| `.encryption_key(key)` | `encryption` | AES-256-GCM key (32 bytes) |

## `CacheOptions` alternative

For advanced use or when you need to pass configuration as data, construct
`CacheOptions` directly and call `CacheEngine::open(options)`.

```rust
use localcache::{CacheEngine, CacheOptions, ChangeDetectionMode};

let engine = CacheEngine::<Vec<f32>>::open(CacheOptions {
    database_path: "cache.sqlite3".into(),
    change_detection_mode: ChangeDetectionMode::MetadataThenFullHash,
    namespace: "docs".into(),
    ..CacheOptions::default()
})?;
```

`CacheOptionsExt` provides ergonomic TTL helpers:

```rust
use localcache::CacheOptionsExt as _;

let opts = CacheOptions::default()
    .with_ttl_hours(2);   // also: with_ttl_secs(), with_ttl_mins()
```
