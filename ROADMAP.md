# Roadmap

## Phases 1–15 ✅  (see CHANGELOG for details)

## Phase 16 — Documentation Overhaul (v0.16.x) ✅

- [x] `docs/book.toml` — updated repository URL, search, fold configuration
- [x] `docs/src/SUMMARY.md` — restructured into User Guide + Reference +
      Development sections with 14 chapters
- [x] `introduction.md` — feature table, quick-links, value proposition
- [x] `getting_started.md` — installation, first cache, `preload`, maintenance
- [x] `features.md` — all 7 Cargo features with code examples
- [x] `builder.md` — all `CacheEngineBuilder` options with examples
- [x] `async.md` — `AsyncCacheEngine`, `ConnectionPool`, `shared_engine`,
      SQLite concurrency model, decision table
- [x] `querying.md` — `QueryBuilder` predicates, sorting, pagination,
      `explain()`, namespace management
- [x] `watching.md` — `CacheWatcher`, `CacheDebouncedWatcher`, platform table,
      thread-ownership pattern
- [x] `portability.md` — export/import, `import_from`, `preload`, glob patterns
- [x] `cookbook.md` — 7 complete recipes (embedding pipeline, multi-threaded
      server, reactive pipeline, versioned cache, encryption, TTL, metrics)
- [x] `cli.md` — all 17 subcommands with usage examples
- [x] `errors.md` — full error variant table, recovery patterns, `anyhow` example
- [x] `change_detection.md` — all 4 modes, decision table, `explain()` usage
- [x] `api.md` — complete type catalogue, `CacheOptions` fields, `ScanOptions`
- [x] `architecture.md` — schema v4, encoding tags, write/read path diagrams,
      LRU SQL, SQLite settings
- [x] `migration.md` — bincode 1→2 upgrade guide, `payload_version` bump,
      DB migration, builder migration, schema version history
- [x] `changelog_summary.md` — phase-by-phase history from 0.1 to 0.15
- [x] `roadmap.md` — completed phases table + future directions

## Phase 17 — RFC Backlog Clearance (v0.17.0) ✅

Five pending RFCs implemented in a single release:

- [x] **RFC 0001** — Recursive directory watching: `watch_dir` / `unwatch_dir`
      on both watcher types; `watch_dirs(bool)` builder flag; `contains()`
      membership filter in callbacks
- [x] **RFC 0002** — Query index hints & explain plan: `QueryBuilder::index_hint`,
      `QueryBuilder::dry_run`, `AsyncCacheEngine::query_dry_run`
- [x] **RFC 0003** — OpenTelemetry spans: `opentelemetry` feature
      (opentelemetry 0.32 + tracing-opentelemetry 0.33); `namespace` field
      added to all tracing spans; `check_status` promoted to `debug_span!`
- [x] **RFC 0004** — Read-only shared-cache mode: `CacheOptions::shared_cache`,
      `CacheEngineBuilder::shared_cache()`; SQLite URI + `query_only` ON;
      `:memory:` shared in-process variant
- [x] **RFC 0005** — async-std / smol runtime variants: `async-std` and `smol`
      features; `src/cache/runtime.rs` `SpawnBlocking` trait; precedence-based
      dispatch (Tokio > async-std > smol) for additive feature compatibility
- [x] RFC 000 lifecycle policy adopted: `rfcs/` restructured into
      `proposed/` / `done/` / `archive/` folders

## Phase 18 — Directory-scoped Query Predicates (v0.18.0) ✅

- [x] `QueryBuilder::path_in_dir(dir, recursive: bool)` — SQL-native directory
      scoping; LIKE-metacharacter-safe prefix construction with `escape_like`
- [x] `QueryBuilder::path_glob(pattern)` — brace-expanding glob in SQL via
      `expand_braces` + SQLite `GLOB`; `[` escaped to `[[]`
- [x] Shared `build_path_sql` helper + `params_from_iter` in `repository.rs`
- [x] RFC 0006 authored, implemented, and moved to `rfcs/done/`

## Phase 19 — Read-only Pool and Compatibility Guarantees (v0.19.0) ✅

- [x] **RFC 0007** — `ReadPool<T>`: N-slot read-only pool, `Clone+Send+Sync`,
      round-robin checkout, independent/shared-cache backends,
      full read-side API including `query_run`/`query_dry_run`
- [x] **RFC 0008** — Compatibility guarantees:
      wire-format stability documented + enforced by golden fixture
      (`tests/fixtures/compat-v0_18.sqlite3`); path-semantics contract
      documented in code and docs; 9 regression tests
- [x] Release tarball structure changed to `localcache-vX.Y.Z/(files)`

## Future / Unscheduled

*(all items from the previous Future section shipped in v0.17.0)*

- Performance tuning for very large namespaces (> 1M entries)
- Cross-process shared-cache via named shared memory (beyond RFC 0004 scope)
- `#[async_test]` proc-macro wrapper for unified async test authoring across
  runtime backends (deferred from RFC 0005)
