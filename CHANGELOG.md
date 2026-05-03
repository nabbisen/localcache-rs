# Changelog

All notable changes to this project will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

---

## [0.1.0] — 2025-05-02

### Added

- `CacheEngine<T>` — main entry point for the cache.
- `CacheOptions` — configuration (database path, change-detection mode).
- `ChangeDetectionMode` — `MetadataOnly`, `MetadataThenPartialHash` (falls
  back to full hash), `MetadataThenFullHash`, `StrictFullHash`.
- `CacheEntry<T>` — value returned from successful cache reads.
- `CacheStatus` — `Fresh`, `Stale`, `Missing`.
- `FileMetadata` — `mtime`, `file_size`, optional `hash`.
- `LocalFileCacheError` — unified error type via `thiserror`.
- `CacheEngine::open` — opens/creates the SQLite database and applies schema.
- `CacheEngine::get` — raw cache retrieval (no staleness check).
- `CacheEngine::get_if_fresh` — retrieval with integrated change detection.
- `CacheEngine::set` — atomic upsert inside a single SQLite transaction.
- `CacheEngine::remove` — delete a cache entry (payload CASCADE-deleted).
- `CacheEngine::check_status` — check freshness without loading payload.
- `CacheEngine::cleanup_missing_files` — remove entries for deleted files.
- `CacheEngine::shrink_database` — run `VACUUM` to reclaim disk space.
- BLAKE3 streaming hash computation (`compute_full_hash`).
- bincode serialisation helpers (`serialize_payload` / `deserialize_payload`).
- 14 integration tests covering all acceptance criteria.
- Apache-2.0 license, NOTICE, TERMS_OF_USE, ROADMAP, and GitHub community files.

[Unreleased]: https://github.com/nabbisen/localcache-rs/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/nabbisen/localcache-rs/releases/tag/v0.1.0
