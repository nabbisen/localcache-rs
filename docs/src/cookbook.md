# Cookbook

Common patterns and recipes for `localcache`.

## Embedding pipeline

Cache vector embeddings for a document corpus.  Only re-embed files
that have changed since the last run.

```rust
use localcache::{CacheEngine, CacheStatus, ChangeDetectionMode};

fn run(corpus: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let engine = CacheEngine::<Vec<f32>>::builder()
        .database("embeddings.sqlite3")
        .change_detection(ChangeDetectionMode::MetadataThenFullHash)
        .namespace("embeddings-v2")
        .payload_version(2)   // bump when model changes
        .build()?;

    let statuses = engine.check_status_batch(corpus);

    for (path, status) in corpus.iter().zip(statuses) {
        match status? {
            CacheStatus::Fresh => {
                // Use cached embedding — free.
            }
            CacheStatus::Stale | CacheStatus::Missing => {
                let embedding = call_embedding_api(path)?;
                engine.set(path, &embedding)?;
            }
        }
    }
    Ok(())
}
```

## Multi-threaded web server

Share a cache pool across Actix-web request handlers:

```rust
use actix_web::{web, App, HttpServer};
use localcache::{ConnectionPool, CacheOptions};
use std::sync::Arc;

type Pool = Arc<ConnectionPool<Vec<f32>>>;

async fn handle(pool: web::Data<Pool>, path: web::Path<String>)
    -> impl actix_web::Responder
{
    let entry = pool.get_if_fresh(&*path).unwrap();
    format!("fresh: {}", entry.is_some())
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let pool = Arc::new(
        ConnectionPool::<Vec<f32>>::open(CacheOptions {
            database_path: "cache.sqlite3".into(),
            ..Default::default()
        }).unwrap()
    );

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(Arc::clone(&pool)))
            .route("/cache/{path}", web::get().to(handle))
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await
}
```

## Reactive pipeline with file watching

Automatically re-process files when they change on disk:

```rust
use localcache::{CacheEngine, InvalidationReason};
use std::time::Duration;

let engine = CacheEngine::<Vec<f32>>::builder()
    .database("reactive.sqlite3")
    .build()?;

// Initial preload.
engine.preload("./data", Default::default(), false, |p| {
    Ok(process(p)?)
})?;

// Start debounced watcher (300 ms window).
let watcher = engine.debounced_watcher(Duration::from_millis(300))?;
let rx = watcher.events();

println!("Watching for changes. Ctrl-C to stop.");
for event in rx.iter() {
    if event.path.exists() {
        match process(&event.path) {
            Ok(payload) => {
                engine.set(&event.path, &payload)?;
                println!("re-cached: {}", event.path.display());
            }
            Err(e) => eprintln!("error: {e}"),
        }
    }
}
```

## Versioned cache with migration

Bump `payload_version` when your computation logic changes:

```rust
const CURRENT_VERSION: u32 = 3;

let engine = CacheEngine::<Analysis>::builder()
    .database("analysis.sqlite3")
    .payload_version(CURRENT_VERSION)
    .build()?;

// Purge all entries from older versions.
let purged = engine.purge_stale_versions()?;
println!("purged {purged} old entries");

// Check version distribution.
for (version, count) in engine.entry_count_by_version()? {
    println!("  v{version}: {count} entries");
}
```

## Encrypted cache

Protect payloads at rest with AES-256-GCM:

```rust
use localcache::CacheEngine;

let key: Vec<u8> = load_key_from_secure_store()?; // 32 bytes

let engine = CacheEngine::<Vec<f32>>::builder()
    .database("secure.sqlite3")
    .encryption_key(key)
    .build()?;

// Key rotation (re-encrypt all entries atomically).
let new_key = generate_new_key();
let rotated = engine.rotate_encryption_key(&new_key)?;
println!("rotated {rotated} entries");
```

## TTL-based expiry

Use time-to-live for data that ages out regardless of file changes:

```rust
use localcache::CacheEngine;
use std::time::Duration;

let engine = CacheEngine::<String>::builder()
    .database("api_cache.sqlite3")
    .ttl(Duration::from_secs(900))  // 15 minutes
    .build()?;

// get_if_fresh respects TTL automatically.
if let Some(entry) = engine.get_if_fresh("endpoint.txt")? {
    return Ok(entry.payload); // still fresh
}

let response = fetch_api()?;
engine.set("endpoint.txt", &response)?;
```

## Query + export pattern

Find high-value entries and export them to a smaller database:

```rust
use localcache::{CacheEngine, Codec};

let src = CacheEngine::<Doc>::builder()
    .database("full.sqlite3")
    .codec(Codec::Json)
    .build()?;

// Find entries matching criteria.
let results = src.query()
    .field_gt("score", 0.9)
    .order_by_field("score", false)
    .run()?;

// Export just those entries.
let records = results.iter()
    .map(|e| src.export_entries()) // simplified
    .collect::<Result<Vec<_>, _>>()?;

let dst = CacheEngine::<Doc>::builder()
    .database("top_entries.sqlite3")
    .build()?;
// dst.import_entries(&records)?;
```

## Monitoring with metrics

Wire up Prometheus to monitor cache performance:

```toml
[dependencies]
localcache         = { version = "0.15", features = ["metrics"] }
metrics-exporter-prometheus = "0.16"
```

```rust
use metrics_exporter_prometheus::PrometheusBuilder;

// Install the recorder once at startup.
PrometheusBuilder::new()
    .install()
    .expect("failed to install Prometheus recorder");

// localcache now emits localcache.get.hit, localcache.get.miss, etc.
let engine = CacheEngine::<Vec<f32>>::builder()
    .database("cache.sqlite3")
    .build()?;
```
