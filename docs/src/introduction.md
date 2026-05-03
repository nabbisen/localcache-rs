# localcache

**Cache expensive computation results tied to local files — fast, simple, and SQLite-backed.**

```toml
[dependencies]
localcache = "0.15"
serde = { version = "1", features = ["derive"] }
```

---

## What it does

`localcache` answers one question: **has this file changed since I last processed it?**

If the answer is "no", it returns the result you computed last time.
If "yes" (or you haven't processed it yet), you compute and store the result.
The cache handles everything else.

```rust
use localcache::{CacheEngine, ChangeDetectionMode};

let engine = CacheEngine::<Vec<f32>>::builder()
    .database("embeddings.sqlite3")
    .change_detection(ChangeDetectionMode::MetadataThenFullHash)
    .build()?;

// Cheap: returns cached embedding if the file hasn't changed.
if let Some(entry) = engine.get_if_fresh("document.txt")? {
    return Ok(entry.payload);
}

// Expensive: compute and store the embedding.
let embedding = embed("document.txt")?;
engine.set("document.txt", &embedding)?;
```

## Why localcache?

| Need | localcache |
|---|---|
| Store any serialisable type | ✅ `T: Serialize + DeserializeOwned` |
| Detect file changes automatically | ✅ metadata + BLAKE3 hash |
| Works offline, no services | ✅ single SQLite file |
| Thread-safe sharing | ✅ `ConnectionPool<T>` |
| Async runtimes (Tokio) | ✅ `AsyncCacheEngine<T>` |
| React to file changes | ✅ `CacheWatcher<T>` (OS events) |
| Bulk-cache a directory | ✅ `engine.preload(dir, opts, ...)` |
| Encrypt payloads at rest | ✅ AES-256-GCM |
| Compress payloads | ✅ zstd |
| Query by payload content | ✅ `QueryBuilder` |
| Production observability | ✅ `tracing` + `metrics` features |

## Quick links

- [Getting Started](./getting_started.md) — install and first cache in 5 minutes
- [Features](./features.md) — optional feature flags explained
- [Cookbook](./cookbook.md) — common patterns and recipes
- [CLI Tool](./cli.md) — `localcache` binary for inspection and maintenance
- [GitHub](https://github.com/nabbisen/localcache-rs) — source code
- [crates.io](https://crates.io/crates/localcache) — latest release
- [docs.rs](https://docs.rs/localcache) — API reference
