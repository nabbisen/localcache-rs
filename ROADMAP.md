# Roadmap

## Phase 1 — Foundation (v0.1.x) ✅
Core sync API, SQLite schema, bincode payloads, BLAKE3 hashing, 14 tests.

## Phase 2 — Ergonomics & Reliability (v0.2.x) ✅
Namespaces, batch ops, TTL, configurable PRAGMAs, schema migration, 26 tests.

## Phase 3 — Performance (v0.3.x) ✅
True partial hash, streaming bincode, read-only mode, in-memory backend, 39 tests.

## Phase 4 — Async & Ecosystem (v0.4.x) ✅

- [x] `async` feature — `AsyncCacheEngine<T>` via `tokio::task::spawn_blocking`
- [x] `compression` feature — zstd payload compression (`payloads.encoding` column)
- [x] `scan_dir(dir, recursive)` — directory scan helper on sync and async engines
- [x] Payload schema versioning — `payload_version` in `CacheOptions` and DB schema
- [x] Schema v3 migration (v2 → v3 via `ALTER TABLE ADD COLUMN`)

## Future / Unscheduled

- Encryption at rest
- LRU / max-entry eviction
- File-watching integration
- `serde_json` alternative codec feature
- `async-std` / `smol` feature variants
- Batch schema version migration helper
- `scan_dir` with glob/extension filters
