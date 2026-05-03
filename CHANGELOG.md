# Changelog

All notable changes to this project will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

---

## [0.2.0] — 2025-05-02

### Added

- **`cache_namespace`** — `CacheOptions::namespace` field (default `"default"`).
  Multiple `CacheEngine` instances can share one SQLite file with fully isolated
  entries.
- **`JournalMode`** enum and `CacheOptions::journal_mode` — choose between
  `Wal` (default), `Delete`, or `Memory` journal modes.
- **`SynchronousMode`** enum and `CacheOptions::synchronous` — control SQLite
  `synchronous` pragma (`Off`, `Normal` (default), `Full`, `Extra`).
- **`CacheOptions::ttl`** — optional `Duration`-based time-to-live.
  `get_if_fresh`, `batch_get_fresh`, and `check_status` respect TTL.
  Expired entries are treated as `Stale`.
- **`CacheEngine::batch_set`** — store multiple entries in a single SQLite
  transaction.  Returns a `BatchSetReport` with success count and per-item
  errors.
- **`CacheEngine::batch_get`** — retrieve multiple entries in one pass.
- **`CacheEngine::batch_get_fresh`** — like `batch_get` with change-detection
  and TTL applied.
- **`CacheEngine::cleanup_expired`** — delete entries older than the configured
  TTL.
- **`BatchSetReport`** struct — summary of a `batch_set` operation.
- Automatic schema migration from v1 (no namespace) to v2 via
  `PRAGMA user_version` versioning.  Legacy databases are migrated
  transparently on first open; existing entries are moved to the
  `"default"` namespace.

### Changed

- `remove` now falls back to a DB-path-string search when the target file no
  longer exists on disk, making it possible to remove stale entries without
  the source file being present.
- `CacheOptions` default now includes `journal_mode: Wal` and
  `synchronous: Normal` (previously the engine always used WAL without
  exposing the choice).

### Migration

Existing `0.1.x` databases are automatically migrated to the `v2` schema
(namespace column added) on the first `CacheEngine::open` call.  No manual
action is required.

---

## [0.1.0] — 2025-05-02

### Added

- `CacheEngine<T>` — main entry point for the cache.
- `CacheOptions`, `ChangeDetectionMode`, `CacheEntry<T>`, `CacheStatus`,
  `FileMetadata`, `LocalFileCacheError`.
- `set` / `get` / `get_if_fresh` / `remove` / `check_status`.
- `cleanup_missing_files` / `shrink_database`.
- `MetadataOnly`, `MetadataThenFullHash`, `StrictFullHash` change detection.
- BLAKE3 streaming hash, bincode serialisation, atomic SQLite transactions.
- Full integration test suite (14 tests).

[Unreleased]: https://github.com/nabbisen/localcache/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/nabbisen/localcache/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/nabbisen/localcache/releases/tag/v0.1.0
