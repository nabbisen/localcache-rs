# Async & Thread Safety

## `AsyncCacheEngine<T>` (tokio)

Requires the `async` feature.  Every blocking operation is wrapped in
`tokio::task::spawn_blocking`, ensuring the async executor thread is never
blocked by SQLite I/O.

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

## `ConnectionPool<T>` (sync multi-threading)

For synchronous multi-threaded applications (e.g. Actix-web handlers,
Rayon workers) where you don't want an async runtime, use `ConnectionPool`:

```rust
use std::{sync::Arc, thread};
use localcache::{ConnectionPool, CacheOptions};

let pool = Arc::new(ConnectionPool::<Vec<f32>>::open(CacheOptions {
    database_path: "cache.sqlite3".into(),
    ..Default::default()
})?);

let handles: Vec<_> = (0..8).map(|_| {
    let pool = Arc::clone(&pool);
    thread::spawn(move || {
        let _ = pool.get_if_fresh("some_file.txt");
    })
}).collect();

for h in handles { h.join().unwrap(); }
```

`ConnectionPool<T>` is `Clone` — all clones share the same engine.

### `shared_engine` helper

For code that needs direct `Arc<Mutex<CacheEngine<T>>>` access:

```rust
use localcache::{shared_engine, CacheOptions};

let shared = shared_engine::<Vec<f32>>(CacheOptions::default())?;
// shared: Arc<Mutex<CacheEngine<Vec<f32>>>>
let count = shared.lock().unwrap().entry_count()?;
```

## SQLite concurrency notes

`localcache` uses SQLite's **WAL (Write-Ahead Logging)** journal mode by
default, which allows one writer and multiple concurrent readers.

- `CacheEngine` is **not** `Send` — SQLite connections cannot be shared
  across threads.
- `ConnectionPool` and `AsyncCacheEngine` both solve this by wrapping the
  engine in `Arc<Mutex<…>>` and holding the lock only for the duration of
  each operation.
- Multiple `CacheEngine` instances can be opened on the **same file**
  simultaneously (each with its own connection) — SQLite handles locking.

## Choosing the right type

| Scenario | Recommended type |
|---|---|
| Single-threaded / simple scripts | `CacheEngine<T>` |
| Async (Tokio) | `AsyncCacheEngine<T>` |
| Sync multi-threaded | `ConnectionPool<T>` |
| Manual Arc<Mutex<…>> control | `shared_engine()` |

## Alternative async runtimes (v0.17.0)

`AsyncCacheEngine` is no longer tied to Tokio.  Enable one of the
alternative runtime features instead:

```toml
# async-std backend
localcache = { version = "0.17", features = ["async-std"] }

# smol backend
localcache = { version = "0.17", features = ["smol"] }
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
| Sync multi-threaded | `ConnectionPool<T>` |
| Manual Arc<Mutex<…>> control | `shared_engine()` |
