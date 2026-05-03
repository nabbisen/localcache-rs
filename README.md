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
localcache = "0.1"
serde = { version = "1", features = ["derive"] }
```

Basic usage:

```rust
use localcache::{CacheEngine, CacheOptions, ChangeDetectionMode};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let engine = CacheEngine::<Vec<f32>>::open(CacheOptions {
        database_path: "cache.sqlite3".into(),
        change_detection_mode: ChangeDetectionMode::MetadataThenFullHash,
    })?;

    let path = "sample.txt";
    let embedding = vec![0.1, 0.2, 0.3];

    engine.set(path, &embedding)?;

    if let Some(entry) = engine.get_if_fresh(path)? {
        println!("cached vector: {:?}", entry.payload);
    }

    Ok(())
}
```

---

## Features / Design Notes

- **Zero-daemon** — just a library; no background processes.
- **Single-file storage** — one SQLite database, easy to ship or delete.
- **Pluggable change detection** — `MetadataOnly` (fast), `MetadataThenFullHash`
  (balanced), or `StrictFullHash` (exact).
- **Any serialisable payload** — `T: Serialize + DeserializeOwned` via bincode.
- **Atomic writes** — `set` uses a single SQLite transaction; partial failures
  leave no corrupt state.
- **Cascade cleanup** — payload rows are deleted automatically when their parent
  file row is removed.

---

## For more detail, see our full documentation

→ [`docs/src/`](docs/src/SUMMARY.md)

Key chapters:

- [Architecture](docs/src/architecture.md)
- [Change Detection Modes](docs/src/change_detection.md)
- [API Reference](docs/src/api.md)
