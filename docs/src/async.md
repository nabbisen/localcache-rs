# Async & Thread Safety

## `AsyncCacheEngine<T>`

Available with the `async` (Tokio), `async-std`, or `smol` Cargo features.
Every blocking operation runs on a `spawn_blocking`-equivalent from the
active runtime, ensuring the async executor thread is never blocked by
SQLite I/O.  The default and most-documented backend is Tokio — see
[Alternative async runtimes](#alternative-async-runtimes) below for
the others.

```rust
use localcache::{AsyncCacheEngine, CacheOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let engine = AsyncCacheEngine::<Vec<f32>>::open(CacheOptions {
        database_path: "cache.sqlite3".into(),
        ..Default::default()
    }).await?;

    let path = std::path::PathBuf::from("document.txt");
    let payload = vec![0.1_f32, 0.2, 0.3];

    engine.set(path.clone(), payload).await?;

    if let Some(entry) = engine.get_if_fresh(path).await? {
        println!("hit: {:?}", entry.payload);
    }

    Ok(())
}
```

`AsyncCacheEngine<T>` is `Clone` — all clones share the underlying engine
via `Arc<Mutex<CacheEngine<T>>>`.

### Async query execution

Because `QueryBuilder` borrows the engine, it cannot cross an `await` point.
Use `query_run` to build the query inside a closure:

```rust
let results: Vec<_> = engine.query_run(|q| {
    q.path_like("%/docs/%").limit(20)
}).await?;
```

## `SyncCacheEngine<T>` (sync multi-threading)

For synchronous multi-threaded applications (e.g. Actix-web handlers,
Rayon workers) where you don't want an async runtime, use `SyncCacheEngine` — one engine
behind a mutex, the synchronous counterpart of `AsyncCacheEngine`:

```rust
use std::{sync::Arc, thread};
use localcache::{SyncCacheEngine, CacheOptions};

let engine = Arc::new(SyncCacheEngine::<Vec<f32>>::open(CacheOptions {
    database_path: "cache.sqlite3".into(),
    ..Default::default()
})?);

let handles: Vec<_> = (0..8).map(|_| {
    let engine = Arc::clone(&engine);
    thread::spawn(move || {
        let _ = engine.get_if_fresh("some_file.txt");
    })
}).collect();

for h in handles { h.join().unwrap(); }
```

`SyncCacheEngine<T>` is `Clone` — all clones share the same engine.

## `ReadPool<T>` (concurrent read-only)

For **read-heavy** workloads where a separate writer engine handles all
writes (e.g. a gallery loader or similarity search pipeline), `ReadPool`
holds N independent read-only connections.  All N slots can be used
concurrently — SQLite WAL mode allows unlimited simultaneous readers.

```rust
use localcache::{CacheEngine, ReadPool, CacheOptions};

// Writer (separate thread / process owns this):
let writer: CacheEngine<Vec<f32>> = CacheEngine::builder()
    .database("cache.sqlite3")
    .build()?;

// Read-only pool shared by worker threads:
let pool: ReadPool<Vec<f32>> = CacheEngine::builder()
    .database("cache.sqlite3")
    .build_read_pool(4)?;   // 4 independent read-only connections

// Clone is cheap (Arc clone — same slots):
let pool2 = pool.clone();
std::thread::spawn(move || {
    let entry = pool2.get_if_fresh("file.txt")?;
    Ok::<_, localcache::LocalFileCacheError>(entry)
});
```

`ReadPool` exposes the full read-side API: `get`, `get_if_fresh`, `batch_get`,
`check_status`, `contains`, `keys`, `export_entries`, `query_run`, and more.
**Write methods are absent from the type** — read-onlyness is a compile-time
property, not a runtime `Err(ReadOnly)`.

### Shared-cache backend (lower memory)

To share the SQLite page cache across slots (lower per-slot memory, at the
cost of shared-cache table locks):

```rust
let pool: ReadPool<Vec<f32>> = ReadPool::open(
    CacheOptions { database_path: "cache.sqlite3".into(), shared_cache: true, ..Default::default() },
    8,
)?;
```

## SQLite concurrency notes

`localcache` uses SQLite's **WAL (Write-Ahead Logging)** journal mode by
default, which allows one writer and multiple concurrent readers.

- `CacheEngine` is `Send` but not `Sync` — it can be moved to another
  thread, but not shared across threads through a `&CacheEngine` (an
  `Arc<CacheEngine<T>>` is therefore not itself `Send`, since that
  requires `T: Sync`).
- `SyncCacheEngine` and `AsyncCacheEngine` both solve this by wrapping the
  engine in `Arc<Mutex<…>>` and holding the lock only for the duration of
  each operation.
- Multiple `CacheEngine` instances can be opened on the **same file**
  simultaneously (each with its own connection) — SQLite handles locking.

## Alternative async runtimes

`AsyncCacheEngine` is no longer tied to Tokio.  Enable one of the
alternative runtime features instead:

```toml
# async-std backend
localcache = { version = "0.21.4", features = ["async-std"] }

# smol backend
localcache = { version = "0.21.4", features = ["smol"] }
```

The public API of `AsyncCacheEngine` is identical regardless of which
runtime is active.  When multiple runtime features are enabled, the
active backend is selected by priority: **Tokio (`async`) > async-std > smol**.

```rust
// async-std example
#[async_std::main]
async fn main() -> Result<(), localcache::LocalFileCacheError> {
    let engine = AsyncCacheEngine::<Vec<f32>>::open(CacheOptions {
        database_path: "cache.sqlite3".into(),
        ..CacheOptions::default()
    }).await?;

    engine.set("file.txt".into(), vec![0.1_f32]).await?;
    Ok(())
}
```

### Decision table (updated)

| Scenario | Recommended |
|---|---|
| Single-threaded / simple scripts | `CacheEngine<T>` |
| Async (Tokio) | `AsyncCacheEngine<T>` with `async` feature |
| Async (async-std) | `AsyncCacheEngine<T>` with `async-std` feature |
| Async (smol) | `AsyncCacheEngine<T>` with `smol` feature |
| Sync multi-threaded — mixed reads and writes | `SyncCacheEngine<T>` |
| Sync multi-threaded — read-heavy, separate writer | `ReadPool<T>` (N concurrent connections) |
| Direct access to the inner engine | `SyncCacheEngine::with` / `with_mut` |
