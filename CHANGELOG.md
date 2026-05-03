# Changelog

All notable changes to this project will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

---

## [0.13.0] — 2025-05-03

### Added

- **`tracing` Cargo feature** — when enabled, key cache operations emit
  structured `tracing` events:
  * `set` — `debug_span` with path, payload bytes, and encoding on completion.
  * `get` — `debug_span` with path; logs "cache hit" or "cache miss".
  * `check_status` — `debug` log with path, status, and reason
    (e.g. `ttl_expired`, `version_mismatch`).
  Zero-cost (compiled out) when the feature is disabled.
- **`CacheEngine::explain(path)`** — returns a [`Diagnosis`] struct with:
  * `status` — overall `CacheStatus`.
  * `entry_exists` / `file_exists` — booleans.
  * `ttl_remaining_secs` — `Some(0)` if expired, `None` if no TTL configured.
  * `hash_match: Option<bool>` — hash comparison result.
  * `metadata_diff: Option<MetadataDiff>` — `mtime` / `file_size` stored vs
    current, with `mtime_changed` / `size_changed` flags.
  * `payload_version: Option<PayloadVersionInfo>` — stored vs expected version,
    with `matches` flag.
  * `summary: String` — human-readable one-liner explaining the status.
- **`Diagnosis`**, **`MetadataDiff`**, **`PayloadVersionInfo`** — new public
  types exported from `localcache`.
- **`AsyncCacheEngine::explain(path)`** and **`ConnectionPool::explain(path)`**
  — async and pooled variants.
- **`QueryBuilder::order_by_last_accessed(ascending)`** — sort by
  `last_accessed_at` timestamp.  Entries never read since being written have
  `last_accessed_at == 0` and sort as "oldest" in ascending order.
- **Multi-column sort** — `order_by` is now a `Vec<OrderBy>` (was
  `Option<OrderBy>`).  Chain secondary sort keys with:
  * `then_by_field(field_path, ascending)` (requires `json` feature)
  * `then_by_updated_at(ascending)`
  * `then_by_last_accessed(ascending)`
  * `then_by_path(ascending)`
- **CLI `inspect <PATH>`** — calls `explain()` and prints a formatted report
  with status, metadata diff, TTL, hash match, and payload version info.
- **`rust-version = "1.85"`** in `Cargo.toml` — makes the MSRV explicit
  (edition 2024 requires Rust ≥ 1.85).

---

## [0.12.0] — 2025-05-03
Benchmarks, `ConnectionPool`, `CacheOptionsExt`, examples, docs.rs metadata.

## [0.11.0] — 2025-05-03
`QueryBuilder` ordering / pagination, `touch`, persistent indexes, CLI `query`.

## [0.10.0] — 2025-05-03
`contains`, `keys`, `QueryBuilder` predicates, CLI `copy` / `migrate`.

## [0.9.0] — 2025-05-03
`export_entries` / `import_entries` / `import_from`, CLI `export` / `import`.

## [0.8.0] — 2025-05-03
Cargo workspace, `localcache-cli`, `on_evict`, multi-group brace expansion.

## [0.7.0] — 2025-05-02
Builder API, `cache_stats`, `check_status_batch`, key rotation.

## [0.6.0] — 2025-05-02
AES-256-GCM encryption, true LRU, glob scan, `list_entries`, schema v4.

## [0.5.0] — 2025-05-02
JSON codec, `max_entries`, `scan_dir_filtered`, version migration.

## [0.4.0] — 2025-05-02
`AsyncCacheEngine`, zstd, `scan_dir`, payload versioning.

## [0.3.0] — 2025-05-02
Partial hash, streaming bincode, read-only, in-memory backend.

## [0.2.0] — 2025-05-02
Namespaces, batch ops, TTL, PRAGMAs, schema migration.

## [0.1.0] — 2025-05-02
Initial release.

[Unreleased]: https://github.com/nabbisen/localcache-rs/compare/v0.13.0...HEAD
[0.13.0]: https://github.com/nabbisen/localcache-rs/compare/v0.12.0...v0.13.0
[0.12.0]: https://github.com/nabbisen/localcache-rs/compare/v0.11.0...v0.12.0
[0.11.0]: https://github.com/nabbisen/localcache-rs/compare/v0.10.0...v0.11.0
[0.10.0]: https://github.com/nabbisen/localcache-rs/compare/v0.9.0...v0.10.0
[0.9.0]: https://github.com/nabbisen/localcache-rs/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/nabbisen/localcache-rs/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/nabbisen/localcache-rs/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/nabbisen/localcache-rs/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/nabbisen/localcache-rs/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/nabbisen/localcache-rs/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/nabbisen/localcache-rs/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/nabbisen/localcache-rs/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/nabbisen/localcache-rs/releases/tag/v0.1.0
