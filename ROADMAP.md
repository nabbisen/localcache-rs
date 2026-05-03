# Roadmap

## Phase 1 — Foundation (v0.1.x) ✅

- [x] SQLite-backed persistent cache
- [x] `set` / `get` / `get_if_fresh` / `remove`
- [x] `check_status` — `Fresh` / `Stale` / `Missing`
- [x] `cleanup_missing_files` / `shrink_database`
- [x] `MetadataOnly` / `MetadataThenFullHash` / `StrictFullHash` change detection
- [x] `MetadataThenPartialHash` (fell back to full hash in this phase)
- [x] bincode serialisation for arbitrary `T: Serialize + DeserializeOwned`
- [x] Atomic `set` via SQLite transactions
- [x] `ON DELETE CASCADE` for payload cleanup
- [x] Full integration test suite

## Phase 2 — Ergonomics & Reliability (v0.2.x) ✅

- [x] `batch_set` / `batch_get` / `batch_get_fresh` API
- [x] `remove` accepts paths that no longer exist on disk
- [x] Configurable `journal_mode` / `synchronous` pragma
- [x] Optional `ttl` (time-to-live) for cache entries
- [x] `cache_namespace` — multiple independent caches in a single DB
- [x] Automatic schema migration v1 → v2
- [x] `cleanup_expired` maintenance helper

## Phase 3 — Performance (v0.3.x) ✅

- [x] True `MetadataThenPartialHash` — head + tail sampling (64 KiB each)
- [x] Streaming bincode — pre-allocated `Vec` via `serialized_size`, zero-copy
      `deserialize_from` with `Cursor`
- [x] `read_only` open mode — all write operations return `ReadOnly` error
- [x] In-memory backend — `database_path: ":memory:"` for ephemeral / test use

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
