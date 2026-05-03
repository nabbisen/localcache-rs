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

## Future / Unscheduled

- `async-std` / `smol` feature variants
- Query index hints / explain plan
- Read-only shared-memory DB mode
- Recursive directory watching (`watching` feature)
- OpenTelemetry spans
