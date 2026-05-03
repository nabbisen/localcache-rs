# Changelog

All notable changes to this project will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

---

## [0.12.0] — 2025-05-03

### Added

- **`ConnectionPool<T>`** — a cloneable, thread-safe wrapper around
  `CacheEngine<T>` for multi-threaded synchronous applications.  All clones
  share the same `Arc<Mutex<CacheEngine<T>>>`.  Exposes the full `CacheEngine`
  API surface (`get`, `set`, `batch_get`, `batch_set`, `remove`,
  `check_status`, `check_status_batch`, `contains`, `keys`, `touch`,
  `scan_dir`, `scan_dir_filtered`, `list_entries`, `entry_count`, `cache_stats`,
  `export_entries`, `import_entries`, `cleanup_*`, `shrink_database`, and
  `query_run`).  New public item in `localcache`.
- **`shared_engine<T>(options)`** — convenience function returning
  `Arc<Mutex<CacheEngine<T>>>` (aka `SharedEngine<T>`).
- **`CacheOptionsExt` trait** — ergonomic TTL builders on `CacheOptions`:
  * `with_ttl_secs(secs)` — set TTL from seconds
  * `with_ttl_mins(mins)` — set TTL from minutes
  * `with_ttl_hours(hours)` — set TTL from hours
- **`benches/cache_bench.rs`** — criterion v0.5 benchmark suite measuring:
  `set` (three change-detection modes), `get` (hit / miss), `get_if_fresh`,
  `batch_set` (10 / 100 / 500 entries), `check_status_batch` (10 / 100 / 500),
  payload-size scaling (64 → 262 144 floats), and metadata queries
  (`entry_count`, `cache_stats`, `list_entries`, `keys`).
  Run with `cargo bench --features json`.
- **`examples/embedding_cache.rs`** — demonstrates cold/warm cache and
  selective re-embedding when files change.
- **`examples/document_pipeline.rs`** — shows versioned JSON-payload analysis,
  `batch_set` ingestion, predicate querying (`json` feature), and
  `cache_stats`.
- **`examples/connection_pool.rs`** — eight parallel threads sharing one
  `ConnectionPool`, with `export_entries`, `scan_dir`, and `CacheOptionsExt`.

### Changed

- `[package.metadata.docs.rs]` added with `all-features = true` so docs.rs
  renders all feature-gated items.
- `exclude = ["benches/", "target/", ".github/"]` added for cleaner
  crates.io packages.
- `[[bench]]` now declares `required-features = ["json"]` so `cargo bench`
  without the feature does not fail.
- `serde_json` usage in `query.rs` is now fully gated behind
  `#[cfg(feature = "json")]`.  `QueryBuilder`, `order_by_updated_at`,
  `order_by_path`, `limit`, `offset`, and `path_like` are always available;
  payload predicates and `order_by_field` require `json`.

---

## [0.11.0] — 2025-05-03
`QueryBuilder` ordering / pagination, `touch`, persistent indexes, async index ops, CLI `query`.

## [0.10.0] — 2025-05-03
`contains`, `keys`, `QueryBuilder` predicates, CLI `copy` / `migrate`.

## [0.9.0] — 2025-05-03
`export_entries` / `import_entries` / `import_from`, CLI `export` / `import`, nested brace expansion.

## [0.8.0] — 2025-05-03
Cargo workspace, `localcache-cli`, `on_evict`, multi-group brace expansion.

## [0.7.0] — 2025-05-02
Builder API, `cache_stats`, `check_status_batch`, key rotation.

## [0.6.0] — 2025-05-02
AES-256-GCM encryption, true LRU, glob scan, `list_entries`, schema v4.

## [0.5.0] — 2025-05-02
JSON codec, `max_entries`, `scan_dir_filtered`, version migration.

## [0.4.0] — 2025-05-02
`AsyncCacheEngine`, zstd, `scan_dir`, payload versioning.

## [0.3.0] — 2025-05-02
Partial hash, streaming bincode, read-only, in-memory backend.

## [0.2.0] — 2025-05-02
Namespaces, batch ops, TTL, PRAGMAs, schema migration.

## [0.1.0] — 2025-05-02
Initial release.

[Unreleased]: https://github.com/nabbisen/localcache-rs/compare/v0.12.0...HEAD
[0.12.0]: https://github.com/nabbisen/localcache-rs/compare/v0.11.0...v0.12.0
[0.11.0]: https://github.com/nabbisen/localcache-rs/compare/v0.10.0...v0.11.0
[0.10.0]: https://github.com/nabbisen/localcache-rs/compare/v0.9.0...v0.10.0
[0.9.0]: https://github.com/nabbisen/localcache-rs/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/nabbisen/localcache-rs/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/nabbisen/localcache-rs/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/nabbisen/localcache-rs/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/nabbisen/localcache-rs/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/nabbisen/localcache-rs/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/nabbisen/localcache-rs/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/nabbisen/localcache-rs/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/nabbisen/localcache-rs/releases/tag/v0.1.0
