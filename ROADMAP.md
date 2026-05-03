# Roadmap

## Phase 1 — Foundation (v0.1.x) ✅
Core sync API, SQLite schema, bincode payloads, BLAKE3 hashing.

## Phase 2 — Ergonomics & Reliability (v0.2.x) ✅
Namespaces, batch ops, TTL, configurable PRAGMAs, schema migration.

## Phase 3 — Performance (v0.3.x) ✅
True partial hash, streaming bincode, read-only mode, in-memory backend.

## Phase 4 — Async & Ecosystem (v0.4.x) ✅
`AsyncCacheEngine`, zstd compression, `scan_dir`, payload schema versioning.

## Phase 5 — Polish & Observability (v0.5.x) ✅
JSON codec, `max_entries` eviction, `scan_dir_filtered`, version migration helpers.

## Phase 6 — Security & Advanced Queries (v0.6.x) ✅
AES-256-GCM encryption, true LRU, glob scan, `list_entries`, schema v4.

## Phase 7 — Operational Features (v0.7.x) ✅
Builder API, `cache_stats`, `check_status_batch`, key rotation, glob brace expansion.

## Phase 8 — Workspace & Tooling (v0.8.x) ✅
Cargo workspace, `localcache-cli`, `on_evict` callback, multi-group brace expansion.

## Phase 9 — Durability & Data Portability (v0.9.x) ✅
`export_entries` / `import_entries` / `import_from`, CLI `export` / `import`,
nested brace expansion, Base64 payload encoding.

## Phase 10 — Queries & Advanced CLI (v0.10.x) ✅

- [x] `CacheEngine::contains()` — lightweight existence check (no payload load)
- [x] `CacheEngine::keys(path_like)` — list all stored paths, optionally
      filtered by a SQL `LIKE` pattern
- [x] `CacheEngine::query()` → `QueryBuilder` — fluent predicate-based search
      over payload content via `serde_json::Value`
  - `field_gt` / `field_lt` / `field_eq` / `field_contains` / `payload_contains`
  - `path_like` pre-filter on stored path
  - `limit` cap on results
- [x] CLI `copy --from NS [--to NS]` — fast namespace copy within one DB
- [x] CLI `migrate --src-db / --src-ns [--dst-db / --dst-ns]` — cross-DB migration

## Future / Unscheduled

- File-watching integration (`notify` crate)
- `async-std` / `smol` feature variants
- `QueryBuilder`: `order_by`, `offset`, async `run()`
- Persistent indexes for frequent payload queries
- Read-only shared-memory DB mode
