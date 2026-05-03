# Roadmap

## Phases 1–11 ✅  (see CHANGELOG for details)

## Phase 12 — Release Readiness (v0.12.x) ✅

- [x] `benches/cache_bench.rs` — criterion benchmark suite covering `set`,
      `get`, `get_if_fresh`, `batch_set`, `check_status_batch`, payload-size
      scaling, and metadata queries (`--features json` required)
- [x] `ConnectionPool<T>` — cloneable, thread-safe wrapper around
      `CacheEngine<T>` for multi-threaded synchronous applications
- [x] `shared_engine<T>(options)` — convenience constructor returning
      `Arc<Mutex<CacheEngine<T>>>`
- [x] `CacheOptionsExt` trait — ergonomic TTL helpers: `with_ttl_secs`,
      `with_ttl_mins`, `with_ttl_hours`
- [x] `examples/embedding_cache.rs` — vector embedding pipeline demo
- [x] `examples/document_pipeline.rs` — versioned analysis + batch ingest demo
- [x] `examples/connection_pool.rs` — multi-threaded `ConnectionPool` demo
- [x] `[package.metadata.docs.rs]` — docs.rs `all-features = true` configuration
- [x] `exclude` field in `Cargo.toml` — clean crates.io package
- [x] Proper `required-features` for `[[bench]]` target
- [x] `serde_json` fully gated behind `json` feature in `query.rs`

## Future / Unscheduled

- File-watching integration (`notify` crate)
- `async-std` / `smol` feature variants
- `QueryBuilder`: `order_by_last_accessed`, multi-column sort, async `.run()`
- Query index hints / explain plan
- Read-only shared-memory DB mode
- `cargo publish` automation / release workflow
