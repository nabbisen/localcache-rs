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

- [x] `CacheEngineBuilder<T>` — fluent builder API via `CacheEngine::builder()`
- [x] `CacheEngine::cache_stats()` — aggregate statistics (`CacheStats`)
- [x] `CacheEngine::check_status_batch()` — one-call status for many paths
- [x] `CacheEngine::rotate_encryption_key()` — re-encrypt all entries atomically
- [x] Glob brace expansion — `{a,b,c}` in `ScanOptions::glob_pattern`

## Future / Unscheduled

- File-watching integration (`notify` crate)
- `async-std` / `smol` feature variants
- Nested / multi-group glob brace expansion
- `serde_json` path-based queries on cached payloads
- Read-only shared-memory DB mode
- CLI inspection tool
