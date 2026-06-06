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
localcache         = { version = "0.19", features = ["metrics"] }
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

## Recipe 8 — Distributed tracing with OpenTelemetry

Enable the `opentelemetry` feature to export `localcache` spans to any
OTel-compatible backend:

```toml
[dependencies]
localcache          = { version = "0.19", features = ["opentelemetry"] }
opentelemetry       = { version = "0.32", features = ["trace"] }
tracing-opentelemetry = "0.33"
tracing-subscriber  = { version = "0.3", features = ["registry"] }
opentelemetry_sdk   = { version = "0.32", features = ["rt-tokio"] }
opentelemetry-stdout = { version = "0.32" }
```

```rust
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry::global;
use tracing_subscriber::{layer::SubscriberExt, Registry};
use tracing_opentelemetry::OpenTelemetryLayer;

fn init_tracing() {
    let exporter = opentelemetry_stdout::SpanExporter::default();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter)
        .build();
    global::set_tracer_provider(provider.clone());

    let tracer = provider.tracer("localcache-app");
    let otel_layer = OpenTelemetryLayer::new(tracer);
    let subscriber = Registry::default().with(otel_layer);
    tracing::subscriber::set_global_default(subscriber).unwrap();
}

// Then use the engine normally — spans export automatically:
//   localcache::get  { path = "…", namespace = "embeddings" }
//   localcache::set  { path = "…", namespace = "embeddings", bytes = 4096 }
```

`localcache` never calls OTel APIs directly; it only emits `tracing` spans.
The `opentelemetry` feature simply ensures compatible dependency versions
are in scope.

## Recipe 9 — Read-only shared-cache for worker fleets

One writer + many readers in the same process (e.g. a thread pool):

```rust
// Writer: normal read-write engine.
let writer: CacheEngine<Vec<f32>> = CacheEngine::builder()
    .database("shared_cache.sqlite3")
    .build()?;
writer.set("embedding.bin", &embedding)?;

// Readers: lightweight shared-cache handles (share the SQLite page cache).
let reader: CacheEngine<Vec<f32>> = CacheEngine::builder()
    .database("shared_cache.sqlite3")
    .shared_cache()   // read-only, PRAGMA query_only = ON
    .build()?;

let entry = reader.get("embedding.bin")?;
// reader.set(…) → Err(LocalFileCacheError::ReadOnly)
```

For in-process pipelines where you need both engines to see the same
in-memory data without a file:

```rust
// Both engines open the same named shared in-memory database.
let e1: CacheEngine<Vec<f32>> = CacheEngine::builder()
    .database(":memory:")
    .shared_cache()
    .build()?;
let e2: CacheEngine<Vec<f32>> = CacheEngine::builder()
    .database(":memory:")
    .shared_cache()
    .build()?;

e1.set("key.bin", &payload)?;
assert!(e2.get("key.bin")?.is_some()); // e2 sees e1's data
```
