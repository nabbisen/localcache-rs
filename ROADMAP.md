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
JSON codec, `max_entries` eviction, `scan_dir_filtered`, `purge_stale_versions`,
`entry_count*`, 72 tests.

## Phase 6 — Security & Advanced Queries (v0.6.x) ✅

- [x] `encryption` feature — AES-256-GCM payload encryption; nonce prepended to
      ciphertext; encoding tags `"*-aes256gcm"` orthogonal to codec/compression
- [x] True LRU eviction — `files.last_accessed_at` (schema v4); updated on every
      `get` / `get_if_fresh`; `max_entries` now evicts least-recently-accessed entries
- [x] `scan_dir` glob patterns — `ScanOptions::glob_pattern: Option<String>` with
      inline `*` / `?` wildcard matching against file names
- [x] `list_entries()` — return `Vec<EntryInfo>` with path, metadata, encoding,
      `payload_version`, `updated_at`, `last_accessed_at` (no payload loaded)
- [x] Schema v4 migration (`files.last_accessed_at` + LRU composite index)

## Future / Unscheduled

- File-watching integration (`notify` crate)
- `async-std` / `smol` feature variants
- Glob brace expansion (`{a,b}` patterns)
- Key rotation helper for encrypted caches
- LRU eviction policy choices (LRU vs LFU vs TTL-based)
