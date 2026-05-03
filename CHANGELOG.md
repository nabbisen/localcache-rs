# Changelog

All notable changes to this project will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

---

## [0.5.0] — 2025-05-02

### Added

- **`json` feature** — `Codec::Json` variant using `serde_json`.  Payloads can
  now be stored as human-readable JSON by setting `CacheOptions::codec`.
  Supported encoding tags: `"json"` (uncompressed), `"json-zstd"` (requires
  `json` + `compression` features together).  Bincode and JSON entries coexist
  transparently in the same database.
- **`CacheOptions::max_entries: Option<usize>`** — automatic eviction.  When
  set, the oldest entries (by `updated_at`) are removed after every `set` or
  `batch_set` until the namespace entry count is within the configured limit.
- **`CacheEngine::scan_dir_filtered(dir, ScanOptions)`** — replaces the
  boolean `recursive` flag with a rich [`ScanOptions`] struct that supports
  `max_depth` (limit descent depth) and `extensions` (case-insensitive file
  extension filter).  The existing `scan_dir(dir, recursive)` now delegates to
  this method and remains unchanged.
- **`CacheEngine::purge_stale_versions()`** — delete all entries in the current
  namespace whose stored `payload_version` differs from
  `CacheOptions::payload_version`.  Frees disk space after a schema upgrade.
- **`CacheEngine::entry_count()`** — count total entries in the current namespace.
- **`CacheEngine::entry_count_by_version()`** — group entry count by
  `payload_version`; returns `Vec<(u32, usize)>` sorted by version.
- All new engine methods are also available on `AsyncCacheEngine` (requires
  the `async` feature).
- `Codec` and `ScanOptions` are now public API items.

---

## [0.4.0] — 2025-05-02
`AsyncCacheEngine`, `compression` feature (zstd), `scan_dir`, payload versioning.

## [0.3.0] — 2025-05-02
True `MetadataThenPartialHash`, streaming bincode, `read_only`, in-memory backend.

## [0.2.0] — 2025-05-02
Namespaces, batch ops, TTL, configurable PRAGMAs, schema migration.

## [0.1.0] — 2025-05-02
Initial release.

[Unreleased]: https://github.com/nabbisen/localcache-rs/compare/v0.5.0...HEAD
[0.5.0]: https://github.com/nabbisen/localcache-rs/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/nabbisen/localcache-rs/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/nabbisen/localcache-rs/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/nabbisen/localcache-rs/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/nabbisen/localcache-rs/releases/tag/v0.1.0
