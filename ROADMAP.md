# Roadmap

## Phase 1 — Foundation (v0.1.x) ✅
Core sync API, SQLite schema, bincode payloads, BLAKE3 hashing, 14 tests.

## Phase 2 — Ergonomics & Reliability (v0.2.x) ✅
Namespaces, batch ops, TTL, configurable PRAGMAs, schema migration, 26 tests.

## Phase 3 — Performance (v0.3.x) ✅
True partial hash, streaming bincode, read-only mode, in-memory backend, 39 tests.

## Phase 4 — Async & Ecosystem (v0.4.x) ✅
`AsyncCacheEngine`, zstd compression, `scan_dir`, payload schema versioning, 56 tests.

## Phase 5 — Polish & Observability (v0.5.x) ✅

- [x] `json` feature — `serde_json` codec; `"json"` / `"json-zstd"` encoding tags
- [x] LRU/max-entries eviction — `CacheOptions::max_entries`; oldest-first deletion on `set`
- [x] `scan_dir_filtered` — `ScanOptions` with `extensions` filter and `max_depth`
- [x] `purge_stale_versions` — delete all entries whose version ≠ current
- [x] `entry_count` / `entry_count_by_version` — observability helpers
- [x] `Codec` enum exported as public API

## Future / Unscheduled

- Encryption at rest
- File-watching integration
- `async-std` / `smol` feature variants
- `scan_dir` with glob patterns
- LRU based on last-read time (requires `last_accessed_at` tracking)
