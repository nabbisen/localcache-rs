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

- [x] `CacheEngine::export_entries()` — export namespace as `Vec<ExportRecord>` (Base64 payloads)
- [x] `CacheEngine::import_entries()` — bulk-import from `Vec<ExportRecord>` (single transaction)
- [x] `CacheEngine::import_from()` — direct engine-to-engine copy (no Base64 round-trip)
- [x] `ExportRecord` — serialisable struct (`serde::Serialize + Deserialize`)
- [x] `AsyncCacheEngine::export_entries()` / `import_entries()` — async variants
- [x] CLI `export [--output PATH]` — dump namespace to JSON Lines (`-` = stdout)
- [x] CLI `import [--input PATH]` — restore from JSON Lines (`-` = stdin)
- [x] Nested brace expansion — `{a,{b,c}}` now correctly expands to 3 alternatives;
      `{pre,{mid,post}}_{x,y}.txt` produces 6 combinations using proper
      matching-brace tracking + top-level comma splitting

## Future / Unscheduled

- File-watching integration (`notify` crate)
- `async-std` / `smol` feature variants
- `serde_json` path-based queries on cached payloads
- Read-only shared-memory DB mode
- CLI: `copy` subcommand (namespace-to-namespace within one DB)
- CLI: `migrate` subcommand (export + re-import across DB versions)
