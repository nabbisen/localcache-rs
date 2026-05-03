# Changelog

All notable changes to this project will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

---

## [0.9.0] — 2025-05-03

### Added

- **`CacheEngine::export_entries()`** — export every entry in the current
  namespace as a `Vec<ExportRecord>`.  Payload bytes are stored verbatim
  (compressed/encrypted as-is) and Base64-encoded so the record can be
  serialised to JSON.
- **`CacheEngine::import_entries(records)`** — import a slice of
  `ExportRecord`s into the current namespace inside a single SQLite
  transaction.  Existing entries for the same path are replaced.  Returns the
  count of entries imported.
- **`CacheEngine::import_from(source)`** — copy all entries from a `source`
  `CacheEngine` (which may be in a different database or namespace) without a
  Base64 round-trip.  Returns the number of entries copied.
- **`ExportRecord`** — new public struct (`serde::Serialize + Deserialize`),
  carrying `path`, `payload_b64`, `encoding`, `mtime`, `file_size`, `hash`,
  `payload_version`, `updated_at`, `last_accessed_at`.
- **`AsyncCacheEngine::export_entries()`** and
  **`AsyncCacheEngine::import_entries(records)`** — async equivalents of the
  sync API.
- **CLI `export [--output/-o PATH]`** — dump the namespace to JSON Lines
  format (one `ExportRecord` per line).  `--output -` writes to stdout.
- **CLI `import [--input/-i PATH]`** — restore entries from a JSON Lines file.
  `--input -` reads from stdin.  Existing entries are replaced.
- **Nested brace expansion** — `ScanOptions::glob_pattern` now correctly
  handles nested brace groups such as `{a,{b,c}}` → `["a","b","c"]`.  The
  implementation uses matching-brace depth tracking and a separate
  `split_top_level` helper that splits only on commas outside any `{...}`
  group.

### Dependencies

- `base64 = "0.22"` added to the library crate for payload encoding.
- `serde_json = "1"` added to `localcache-cli` for JSON Lines serialisation.

---

## [0.8.0] — 2025-05-03
Cargo workspace, `localcache-cli`, `on_evict` callback, multi-group brace expansion.

## [0.7.0] — 2025-05-02
Builder API, `cache_stats`, `check_status_batch`, key rotation, single-group brace expansion.

## [0.6.0] — 2025-05-02
AES-256-GCM encryption, true LRU, glob scan, `list_entries`, schema v4.

## [0.5.0] — 2025-05-02
JSON codec, max_entries, scan_dir_filtered, version migration helpers.

## [0.4.0] — 2025-05-02
AsyncCacheEngine, zstd compression, scan_dir, payload schema versioning.

## [0.3.0] — 2025-05-02
True partial hash, streaming bincode, read_only, in-memory backend.

## [0.2.0] — 2025-05-02
Namespaces, batch ops, TTL, configurable PRAGMAs, schema migration.

## [0.1.0] — 2025-05-02
Initial release.

[Unreleased]: https://github.com/nabbisen/localcache-rs/compare/v0.9.0...HEAD
[0.9.0]: https://github.com/nabbisen/localcache-rs/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/nabbisen/localcache-rs/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/nabbisen/localcache-rs/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/nabbisen/localcache-rs/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/nabbisen/localcache-rs/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/nabbisen/localcache-rs/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/nabbisen/localcache-rs/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/nabbisen/localcache-rs/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/nabbisen/localcache-rs/releases/tag/v0.1.0
