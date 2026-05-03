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

- [x] Cargo workspace — `localcache` (library) + `localcache-cli` (binary) as workspace members
- [x] `localcache-cli` — CLI inspection tool with `list`, `stats`, `check`, `cleanup`,
      `vacuum`, `purge-version`, `scan` subcommands; powered by `clap 4`
- [x] `on_evict` callback — `CacheEngineBuilder::on_evict(|path| …)` hook called
      after each LRU eviction from `max_entries`
- [x] Multi-group glob brace expansion — `{a,b}_{c,d}.txt` → Cartesian product
      (recursive `expand_braces`, replacing the single-group implementation)
- [x] `EvictCallback` type alias — reduces complex type repetition in engine and builder

## Future / Unscheduled

- File-watching integration (`notify` crate)
- `async-std` / `smol` feature variants
- Nested brace groups within alternatives
- `serde_json` path-based queries on cached payloads
- Read-only shared-memory DB mode
- CLI: `export` / `import` subcommands (dump/restore)
