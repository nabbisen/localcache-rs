# Getting Started

## Installation

Add `localcache` and `serde` to your `Cargo.toml`:

```toml
[dependencies]
localcache = "0.1"
serde = { version = "1", features = ["derive"] }
```

## Basic usage

```rust
use localcache::{CacheEngine, CacheOptions, ChangeDetectionMode};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Open (or create) the cache database.
    let engine = CacheEngine::<Vec<f32>>::open(CacheOptions {
        database_path: "cache.sqlite3".into(),
        change_detection_mode: ChangeDetectionMode::MetadataThenFullHash,
    })?;

    let path = "sample.txt";
    let embedding = vec![0.1, 0.2, 0.3];

    // Store a result.
    engine.set(path, &embedding)?;

    // Retrieve it — only if the file has not changed.
    if let Some(entry) = engine.get_if_fresh(path)? {
        println!("cached: {:?}", entry.payload);
    }

    Ok(())
}
```

## Choosing a change-detection mode

| Mode | Speed | Reliability | When to use |
|------|-------|-------------|-------------|
| `MetadataOnly` | ⚡⚡⚡ | Good | Files change infrequently; mtime is reliable |
| `MetadataThenFullHash` | ⚡⚡ | Excellent | General purpose default |
| `StrictFullHash` | ⚡ | Perfect | Content-addressed workflows |
| `MetadataThenPartialHash` | ⚡⚡ | Excellent | *(falls back to full hash in v0.1)* |

## Custom payload types

Any type that implements `serde::Serialize + serde::de::DeserializeOwned` works:

```rust
use serde::{Serialize, Deserialize};
use localcache::{CacheEngine, CacheOptions};

#[derive(Serialize, Deserialize)]
struct DocumentAnalysis {
    word_count: usize,
    language: String,
    entities: Vec<String>,
}

let engine = CacheEngine::<DocumentAnalysis>::open(CacheOptions::default())?;
```

## Maintenance

```rust
// Remove cache entries whose source files have been deleted.
let removed = engine.cleanup_missing_files()?;

// Reclaim disk space after many deletions.
engine.shrink_database()?;
```
