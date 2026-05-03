# Changelog

All notable changes to this project will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

---

## [0.4.0] — 2025-05-02

### Added

- **`async` feature** — `AsyncCacheEngine<T>`: an async wrapper that delegates
  every blocking operation (SQLite + filesystem) to
  `tokio::task::spawn_blocking` via an `Arc<Mutex<CacheEngine<T>>>`.  Provides
  the full API surface: `open`, `get`, `get_if_fresh`, `set`, `batch_set`,
  `batch_get`, `batch_get_fresh`, `remove`, `check_status`, `scan_dir`,
  `cleanup_missing_files`, `cleanup_expired`, `shrink_database`.
  `AsyncCacheEngine` is `Clone` — clones share the same underlying engine.
- **`compression` feature** — zstd payload compression.  Set
  `CacheOptions::compress_payloads = true` to compress on write.  The encoding
  (`"raw"` or `"zstd"`) is recorded in a new `payloads.encoding` column; reads
  decompress transparently.  Mixed encoding within a single database (via
  namespaces) is fully supported.
- **`scan_dir(dir, recursive)`** — scans a directory tree and returns a
  `Vec<(PathBuf, CacheStatus)>` for every regular file found.  Available on
  both `CacheEngine` and `AsyncCacheEngine`.
- **Payload schema versioning** — `CacheOptions::payload_version: u32`.  When
  non-zero, `get_if_fresh`, `batch_get_fresh`, and `check_status` treat entries
  with a different stored version as `Stale`.  The version is stored in a new
  `files.payload_version` column.
- **`LocalFileCacheError::UnknownEncoding(String)`** — returned when a stored
  payload uses an encoding unknown to the current build (e.g. `"zstd"` when the
  `compression` feature is not enabled).
- **`LocalFileCacheError::PayloadVersionMismatch`** — returned on explicit
  version-mismatch errors (reserved for future strict-mode API).
- **`LocalFileCacheError::AsyncTaskPanicked`** (`async` feature only) —
  propagated when a `spawn_blocking` task panics.

### Changed

- Schema bumped to v3 (`PRAGMA user_version = 3`).  Migration from v2 adds
  `files.payload_version` and `payloads.encoding` via lightweight
  `ALTER TABLE ADD COLUMN` statements (no data movement required).
  v1 databases are migrated v1 → v2 → v3 in one `open` call.

---

## [0.3.0] — 2025-05-02
True `MetadataThenPartialHash`, streaming bincode, `read_only`, in-memory backend.

## [0.2.0] — 2025-05-02
Namespaces, batch ops, TTL, configurable PRAGMAs, schema migration.

## [0.1.0] — 2025-05-02
Initial release.

[Unreleased]: https://github.com/nabbisen/localcache-rs/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/nabbisen/localcache-rs/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/nabbisen/localcache-rs/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/nabbisen/localcache-rs/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/nabbisen/localcache-rs/releases/tag/v0.1.0
