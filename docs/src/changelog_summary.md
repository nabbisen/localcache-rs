# Changelog Summary

For the full changelog see
[CHANGELOG.md](https://github.com/nabbisen/localcache-rs/blob/main/CHANGELOG.md)
on GitHub.

## 0.15.0 — Production hardening

- `metrics` feature: `metrics 0.24` counters/histograms on `get` and `set`
- `namespace_list()` / `namespace_copy()` for namespace management
- `debounced_watcher(window)` → `CacheDebouncedWatcher<T>`
- CLI `namespaces` subcommand
- `Makefile.toml` for `cargo-make` publish workflow

## 0.14.0 — File watching & preloading

- `watching` feature: `CacheWatcher<T>`, `CacheDebouncedWatcher<T>`
- `CacheEngine::preload()` for bulk directory caching
- CLI `watch` subcommand
- `notify 8` + `notify-debouncer-mini 0.7`
- bincode upgraded 1.3 → 2.0 (wire-compatible via `config::legacy()`)

## 0.13.0 — Observability & DX

- `tracing` feature: debug spans on `get`, `set`, `check_status`
- `CacheEngine::explain()` → `Diagnosis` (staleness report)
- `QueryBuilder::order_by_last_accessed()` + multi-column sort
- CLI `inspect` subcommand
- `rust-version = "1.85"` (MSRV)

## 0.12.0 — Release readiness

- `benches/` — criterion benchmark suite (7 groups)
- `ConnectionPool<T>` — thread-safe sync engine wrapper
- `CacheOptionsExt` trait — `with_ttl_secs/mins/hours()`
- `examples/` — embedding_cache, document_pipeline, connection_pool

## 0.11.0 — Query sorting & indexes

- `QueryBuilder::order_by_*` / `then_by_*` — multi-column sort
- `QueryBuilder::offset()` — pagination
- `CacheEngine::touch()` — manual LRU warm-up
- `create_path_index` / `drop_path_index` / `list_path_indexes`
- CLI `query` subcommand

## 0.10.0 — Queries & advanced CLI

- `CacheEngine::contains()` / `keys(path_like)`
- `QueryBuilder` with field predicates (`field_gt`, `field_lt`, etc.)
- CLI `copy` / `migrate` subcommands

## 0.9.0 — Data portability

- `export_entries()` / `import_entries()` / `import_from()`
- `ExportRecord` (serde-serialisable)
- CLI `export` / `import` subcommands
- Nested brace expansion in glob patterns (`{a,{b,c}}`)

## 0.8.0 — Workspace & tooling

- Cargo workspace (`localcache` + `localcache-cli`)
- `on_evict` callback in `CacheEngineBuilder`
- Multi-group glob brace expansion

## 0.7.0 — Operational features

- `CacheEngineBuilder<T>` fluent builder
- `cache_stats()` / `check_status_batch()`
- `rotate_encryption_key()` (encryption feature)

## 0.6.0 — Security

- `encryption` feature: AES-256-GCM payload encryption
- True LRU eviction (`last_accessed_at` in schema v4)
- `glob_pattern` in `ScanOptions`
- `list_entries()` / `EntryInfo`

## 0.1–0.5 — Foundation

Core API, namespaces, batch operations, TTL, partial hashing,
`AsyncCacheEngine`, zstd compression, JSON codec, `max_entries`,
`scan_dir_filtered`, payload versioning.
