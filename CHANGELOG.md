# Changelog

All notable changes to this project will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

---

## [0.7.0] — 2025-05-02

### Added

- **`CacheEngineBuilder<T>`** — fluent builder obtained via
  `CacheEngine::builder()`.  All `CacheOptions` fields are covered by typed
  methods (`database`, `namespace`, `change_detection`, `ttl`, `max_entries`,
  `payload_version`, `read_only`, `codec`, `journal_mode`, `synchronous`, and
  the feature-gated `compress` / `encryption_key`).  Ends with `.build()`
  which returns `Result<CacheEngine<T>, LocalFileCacheError>`.
- **`CacheEngine::cache_stats()`** — returns a [`CacheStats`] struct with
  `total_entries`, `total_payload_bytes` (on-disk size including
  compression/encryption overhead), `oldest_updated_at`, `newest_updated_at`,
  `entries_by_encoding`, and `entries_by_payload_version`.  Available on both
  sync and async engines.
- **`CacheEngine::check_status_batch(paths)`** — check the freshness of
  multiple paths in a single call; returns `Vec<Result<CacheStatus, _>>` in
  input order.  Convenient for pre-scanning large lists before deciding what
  to recompute.
- **`CacheEngine::rotate_encryption_key(new_key)`** (`encryption` feature) —
  atomically re-encrypts every entry in the current namespace whose encoding
  ends in `"-aes256gcm"` from the current key to `new_key`.  All re-encryption
  happens inside a single SQLite transaction; a failure leaves the database
  consistent (still encrypted with the old key).  Returns the count of
  re-encrypted entries.
- **Glob brace expansion** — `ScanOptions::glob_pattern` now supports
  `{a,b,c}` to match multiple alternatives, e.g. `"*.{txt,md,rst}"`.  The
  first brace group is expanded into individual patterns; a file name matches
  if it matches any of them.
- **`CacheStats`** struct — new public type.
- **`CacheEngineBuilder`** — new public type.

---

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

[Unreleased]: https://github.com/nabbisen/localcache-rs/compare/v0.7.0...HEAD
[0.7.0]: https://github.com/nabbisen/localcache-rs/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/nabbisen/localcache-rs/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/nabbisen/localcache-rs/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/nabbisen/localcache-rs/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/nabbisen/localcache-rs/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/nabbisen/localcache-rs/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/nabbisen/localcache-rs/releases/tag/v0.1.0
