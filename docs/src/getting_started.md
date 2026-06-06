# Getting Started

## Installation

Add to `Cargo.toml`:

```toml
[dependencies]
localcache = "0.19"
serde = { version = "1", features = ["derive"] }
```

## Your first cache

```rust
use localcache::{CacheEngine, ChangeDetectionMode};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Open (or create) the cache using the builder API.
    let engine = CacheEngine::<Vec<f32>>::builder()
        .database("cache.sqlite3")
        .change_detection(ChangeDetectionMode::MetadataThenFullHash)
        .build()?;

    let path = "sample.txt";

    // Try to get a fresh cached result first.
    if let Some(entry) = engine.get_if_fresh(path)? {
        println!("cache hit: {:?}", entry.payload);
        return Ok(());
    }

    // Cache miss — compute and store.
    let result = vec![0.1_f32, 0.2, 0.3]; // your expensive computation
    engine.set(path, &result)?;
    println!("computed and cached");

    Ok(())
}
```

## Custom payload types

Any type implementing `serde::Serialize + serde::de::DeserializeOwned` works:

```rust
use serde::{Serialize, Deserialize};
use localcache::CacheEngine;

#[derive(Serialize, Deserialize)]
struct Analysis {
    word_count: usize,
    language:   String,
    keywords:   Vec<String>,
}

let engine = CacheEngine::<Analysis>::builder()
    .database("analysis.sqlite3")
    .build()?;
```

## Processing a directory

Use `preload` to bulk-cache every file in a directory at once:

```rust
use localcache::{CacheEngine, ScanOptions};

let engine = CacheEngine::<usize>::builder()
    .database("sizes.sqlite3")
    .build()?;

let report = engine.preload(
    "./documents",
    ScanOptions { recursive: true, ..Default::default() },
    false, // skip files that are already fresh
    |path| Ok(std::fs::metadata(path)?.len() as usize),
)?;

println!("stored={} skipped_fresh={} errors={}",
    report.stored, report.already_fresh, report.skipped);
```

## Maintenance

```rust
// Remove entries whose source files no longer exist.
let removed = engine.cleanup_missing_files()?;

// Reclaim disk space after deletions.
engine.shrink_database()?;

// Check a single file's cache status.
use localcache::CacheStatus;
match engine.check_status("sample.txt")? {
    CacheStatus::Fresh   => println!("up to date"),
    CacheStatus::Stale   => println!("file changed"),
    CacheStatus::Missing => println!("not cached"),
}
```

## Next steps

- [Features](./features.md) — optional Cargo features (async, encryption, …)
- [Builder API](./builder.md) — all `CacheEngineBuilder` options
- [Cookbook](./cookbook.md) — real-world usage patterns
