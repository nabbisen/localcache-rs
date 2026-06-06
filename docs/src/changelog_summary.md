# Changelog Summary

For the full changelog see
[CHANGELOG.md](https://github.com/nabbisen/localcache-rs/blob/main/CHANGELOG.md)
on GitHub.

## v0.19.0 — Read-only Pool & Compatibility Guarantees

- `ReadPool<T>` — N-slot concurrent read-only connection pool (`Clone+Send+Sync`),
  `build_read_pool(size)` builder method
- `Codec::Bincode` wire-format stability guarantee: documented in API + committed
  golden fixture (`tests/fixtures/compat-v0_18.sqlite3`) decoded on every CI run
- Path-semantics contract documented: canonicalization, deleted-file fallback,
  `cleanup_missing_files` Windows behaviour
- Release tarballs now use `localcache-vX.Y.Z/(files)` structure

## v0.18.0 — Directory-scoped Query Predicates

- `QueryBuilder::path_in_dir(dir, recursive)` — SQL-native directory scoping;
  LIKE-metacharacter-safe; replaces LIKE + Rust post-filter pattern
- `QueryBuilder::path_glob(pattern)` — `*`/`?`/`{a,b}` brace alternation in SQL
- Both compose with `index_hint`, `dry_run`, payload predicates, and each other
- Shared `build_path_sql` helper + `params_from_iter` in `repository.rs`

## v0.17.0 — RFC Backlog Clearance

- **Recursive directory watching** — `watch_dir`/`unwatch_dir` on both watcher types;
  `watch_dirs(bool)` builder flag; `contains()` filter in callbacks
- **Query index hints** — `QueryBuilder::index_hint`, `dry_run`,
  `AsyncCacheEngine::query_dry_run`
- **OpenTelemetry** — `opentelemetry` feature (0.32/0.33); `namespace` field on
  all tracing spans; `check_status` upgraded to `debug_span!`
- **Shared-cache read-only mode** — `CacheOptions::shared_cache`,
  `.shared_cache()` builder; SQLite URI + `PRAGMA query_only = ON`
- **async-std / smol runtime variants** — `AsyncCacheEngine` now works with
  async-std 1.13 and smol 2.x; precedence-based dispatch keeps `--all-features` clean
- RFC 000 lifecycle policy adopted; all RFCs restructured into `rfcs/done/`

## v0.16.0 — Documentation Overhaul

- 18-chapter mdBook documentation in `docs/src/`
- All 7 Cargo features documented with examples
- Complete `CacheEngineBuilder` option reference
- Architecture, migration, cookbook (7 recipes), CLI reference
- Integration test suite split: 8 thematic files, 188 tests

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
