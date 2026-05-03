# Roadmap

## Phase 1 — Foundation (v0.1.x) ✅

Core functionality with synchronous API.

- [x] SQLite-backed persistent cache
- [x] `set` / `get` / `get_if_fresh` / `remove`
- [x] `check_status` — `Fresh` / `Stale` / `Missing`
- [x] `cleanup_missing_files` / `shrink_database`
- [x] `MetadataOnly` change detection
- [x] `MetadataThenFullHash` change detection (BLAKE3)
- [x] `StrictFullHash` change detection
- [x] `MetadataThenPartialHash` (falls back to full hash in this phase)
- [x] bincode serialisation for arbitrary `T: Serialize + DeserializeOwned`
- [x] Atomic `set` via SQLite transactions
- [x] `ON DELETE CASCADE` for payload cleanup
- [x] Full integration test suite

## Phase 2 — Ergonomics & Reliability (v0.2.x)

- [ ] `batch_set` / `batch_get` API for processing many files at once
- [ ] `remove` accepts paths that no longer exist on disk (by stored path string)
- [ ] Configurable `journal_mode` / `synchronous` pragma
- [ ] Optional `updated_at` expiry / TTL support
- [ ] `cache_namespace` — multiple independent caches in a single DB

## Phase 3 — Performance (v0.3.x)

- [ ] True `MetadataThenPartialHash` (head + tail sampling)
- [ ] Streaming bincode for large payloads
- [ ] `read_only` open mode
- [ ] In-memory backend for testing (`memory://`)

## Phase 4 — Async & Ecosystem (v0.4.x)

- [ ] `async` feature flag (tokio-rusqlite)
- [ ] `compression` feature flag (zstd)
- [ ] Directory scan helper (`scan_dir`)
- [ ] Payload schema versioning

## Future / Unscheduled

- Encryption at rest
- LRU / max-entry eviction
- File-watching integration
- `serde_json` alternative codec feature
