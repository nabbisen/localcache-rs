# Changelog

All notable changes to this project will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

---

## [0.3.0] — 2025-05-02

### Added

- **True `MetadataThenPartialHash`** — now reads the first 64 KiB and last
  64 KiB of the file (128 KiB total I/O maximum per check).  For files
  ≤ 128 KiB the entire content is hashed.  Partial hashes are stored with a
  `"partial:"` prefix so they are distinguishable from full-hash entries
  written by other modes.  No schema change required.
- **`read_only` open mode** — `CacheOptions::read_only: bool`.  When `true`,
  the SQLite connection is opened with `SQLITE_OPEN_READ_ONLY`; all write
  operations (`set`, `batch_set`, `remove`, `cleanup_missing_files`,
  `cleanup_expired`, `shrink_database`) return
  `LocalFileCacheError::ReadOnly`.
- **In-memory backend** — pass `database_path: ":memory:".into()` to open a
  private, ephemeral SQLite database.  Useful for unit tests that need a
  fully functional `CacheEngine` without touching the filesystem.
- **`LocalFileCacheError::ReadOnly`** — new error variant.

### Changed

- **Streaming bincode serialisation** — `serialize_payload` now uses
  `bincode::serialized_size` to pre-allocate the output `Vec` and
  `bincode::serialize_into` to write without internal reallocation.
  `deserialize_payload` uses `bincode::deserialize_from` with a zero-copy
  `std::io::Cursor`, eliminating an intermediate buffer copy on reads.
- `MetadataThenPartialHash` no longer silently falls back to
  `MetadataThenFullHash`.  It uses genuine head+tail sampling.

---

## [0.2.0] — 2025-05-02

### Added

- `cache_namespace`, `JournalMode`, `SynchronousMode`, `CacheOptions::ttl`.
- `batch_set` / `batch_get` / `batch_get_fresh`, `BatchSetReport`.
- `cleanup_expired`.
- Automatic schema migration v1 → v2.
- Improved `remove` (works on deleted files).

---

## [0.1.0] — 2025-05-02

### Added

- Initial release: core sync API, bincode payloads, BLAKE3 hashing, SQLite
  schema, 14 integration tests.

[Unreleased]: https://github.com/nabbisen/localcach-rs/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/nabbisen/localcach-rs/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/nabbisen/localcach-rs/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/nabbisen/localcach-rs/releases/tag/v0.1.0
