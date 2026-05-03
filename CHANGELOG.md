# Changelog

All notable changes to this project will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

---

## [0.6.0] — 2025-05-02

### Added

- **`encryption` feature** — AES-256-GCM payload encryption.
  Set `CacheOptions::encryption_key: Some(Vec<u8>)` (exactly 32 bytes) to
  encrypt every payload written by that engine.  A fresh 96-bit nonce is
  generated per write and prepended to the ciphertext.  Encoding tags follow
  the `"<codec-layers>-aes256gcm"` convention (e.g. `"raw-aes256gcm"`,
  `"zstd-aes256gcm"`, `"json-zstd-aes256gcm"`), so encryption is orthogonal
  to codec and compression choices.  Reading with a wrong or absent key returns
  `LocalFileCacheError::EncryptionError`.
- **True LRU eviction** — `files.last_accessed_at` column (schema v4).  Every
  successful `get` or `get_if_fresh` call updates `last_accessed_at` for the
  returned entry (skipped in read-only mode).  When `max_entries` is set,
  eviction now removes the **least recently accessed** entries (ordering by
  `last_accessed_at ASC, updated_at ASC`) instead of oldest-by-write.
- **Glob pattern support in `scan_dir_filtered`** — `ScanOptions::glob_pattern:
  Option<String>` matches the file *name* (not full path) using `*` (any
  sequence) and `?` (exactly one character) wildcards.  Can be combined with
  the `extensions` filter; both must match for a file to be included.
- **`CacheEngine::list_entries()`** — returns `Vec<EntryInfo>` (sorted by
  `updated_at DESC`) with full per-entry metadata: `path`, `metadata`,
  `payload_version`, `encoding`, `updated_at`, `last_accessed_at`.  Payload
  content is **not** loaded.  Available on both sync and async engines.
- **`EntryInfo`** struct — new public type, available without any features.
- **`LocalFileCacheError::EncryptionError(String)`** — new error variant
  (requires `encryption` feature).
- Schema v4 migration: `ALTER TABLE files ADD COLUMN last_accessed_at INTEGER
  NOT NULL DEFAULT 0` plus a composite LRU index
  `(namespace, last_accessed_at, updated_at)`.  Databases at v1–v3 are
  migrated in a single `open` call.

---

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

[Unreleased]: https://github.com/nabbisen/localcache-rs/compare/v0.6.0...HEAD
[0.6.0]: https://github.com/nabbisen/localcache-rs/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/nabbisen/localcache-rs/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/nabbisen/localcache-rs/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/nabbisen/localcache-rs/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/nabbisen/localcache-rs/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/nabbisen/localcache-rs/releases/tag/v0.1.0
