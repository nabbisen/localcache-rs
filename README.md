# localcache

[![crates.io](https://img.shields.io/crates/v/localcache?label=rust)](https://crates.io/crates/localcache)
[![License](https://img.shields.io/github/license/nabbisen/localcache-rs)](https://github.com/nabbisen/localcache-rs/blob/main/LICENSE)
[![Rust Documentation](https://docs.rs/localcache/badge.svg?version=latest)](https://docs.rs/localcache)
[![Dependency Status](https://deps.rs/crate/localcache/latest/status.svg)](https://deps.rs/crate/localcache)

**Cache expensive computation results tied to local files — fast, simple, and SQLite-backed.**

---

## Overview

`localcache` stores arbitrary, serialisable payloads (embeddings, parsed documents,
feature vectors, …) next to metadata about the source file they were derived from.
On the next request it can tell you immediately whether the cached result is still
valid — without re-running the expensive computation.

Storage is a single SQLite file that lives wherever you point it.  No daemon, no
background threads, no network.

---

## Why / When

Use `localcache` when you have a workflow like:

```
for file in corpus:
    if cache.is_fresh(file):
        embedding = cache.get(file)
    else:
        embedding = model.embed(file)   # expensive
        cache.set(file, embedding)
```

Typical use cases:

- **Document / image analysis** — avoid re-parsing unchanged files.
- **AI inference** — skip re-embedding files whose content has not changed.
- **Feature extraction** — reuse computed feature vectors across runs.
- **Build tools / pipelines** — skip re-processing up-to-date artifacts.

---

## Quick Start

Add to `Cargo.toml`:

```toml
[dependencies]
localcache = "0.13"
serde = { version = "1", features = ["derive"] }
```

Basic usage:

```rust
use localcache::{CacheEngine, ChangeDetectionMode};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let engine = CacheEngine::<Vec<f32>>::builder()
        .database("cache.sqlite3")
        .change_detection(ChangeDetectionMode::MetadataThenFullHash)
        .build()?;

    let path = "sample.txt";
    let embedding = vec![0.1_f32, 0.2, 0.3];

    engine.set(path, &embedding)?;

    if let Some(entry) = engine.get_if_fresh(path)? {
        println!("cached vector: {:?}", entry.payload);
    }

    Ok(())
}
```

---

## Features

| Cargo feature | Description |
|---|---|
| `async` | `AsyncCacheEngine` backed by `tokio::task::spawn_blocking` |
| `compression` | Transparent `zstd` payload compression |
| `json` | `Json` codec + payload field queries in `QueryBuilder` |
| `encryption` | AES-256-GCM payload encryption |
| `tracing` | `tracing` instrumentation on hot paths (zero-cost when disabled) |

---

## Design Highlights

- **Zero-daemon** — just a library; no background processes.
- **Single-file storage** — one SQLite database, easy to ship or delete.
- **Pluggable change detection** — `MetadataOnly` (fast), `MetadataThenFullHash`
  (balanced), or `StrictFullHash` (exact).
- **Any serialisable payload** — `T: Serialize + DeserializeOwned` via bincode or JSON.
- **Atomic writes** — `set` uses a single SQLite transaction; partial failures
  leave no corrupt state.
- **LRU eviction** — `max_entries` evicts the least recently accessed entries automatically.
- **Thread-safe** — `ConnectionPool<T>` wraps the engine in `Arc<Mutex<…>>` for
  multi-threaded use; `AsyncCacheEngine<T>` for async runtimes.
- **Data portability** — `export_entries` / `import_entries` / `import_from` for
  cross-database migration.

---

## CLI Tool

The `localcache-cli` crate ships a `localcache` binary for database inspection:

```sh
cargo install localcache-cli

localcache -d cache.sqlite3 stats
localcache -d cache.sqlite3 list --limit 20
localcache -d cache.sqlite3 inspect /path/to/file.txt
localcache -d cache.sqlite3 export > backup.jsonl
localcache -d cache.sqlite3 scan ./docs --recursive --glob "*.{md,txt}"
```

---

## Repository

<https://github.com/nabbisen/localcache-rs>

For documentation see [docs.rs/localcache](https://docs.rs/localcache).
