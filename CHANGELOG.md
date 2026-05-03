# Changelog

All notable changes to this project will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

---

## [0.10.0] — 2025-05-03

### Added

- **`CacheEngine::contains(path)`** — returns `true` if the namespace contains
  an entry for the given path.  Does **not** load the payload, making it
  cheaper than `get()` for existence checks.
- **`CacheEngine::keys(path_like)`** — returns all stored paths (as
  `Vec<PathBuf>`) sorted lexicographically.  Pass a SQLite `LIKE` pattern
  (e.g. `Some("/home/user/%")`) to restrict the result, or `None` for all keys.
- **`CacheEngine::query()` → `QueryBuilder`** — fluent builder for
  predicate-based searches over payload content:
  * `.field_gt(field, threshold)` — numeric field greater than threshold
  * `.field_lt(field, threshold)` — numeric field less than threshold
  * `.field_eq(field, value)` — field equals a JSON value
  * `.field_contains(field, substring)` — string field contains substring
  * `.payload_contains(needle)` — full-payload text search
  * `.path_like(pattern)` — pre-filter by stored path (SQL LIKE)
  * `.limit(n)` — cap the number of returned entries
  * `.run()` — execute and return `Vec<CacheEntry<T>>`
  Predicates are evaluated against `serde_json::Value`, so they work with any
  codec.  Requires the `json` Cargo feature.
- **`QueryBuilder`** — new public type exported from `localcache`.
- **CLI `copy --from NS [--to NS]`** — copy all entries from one namespace to
  another within the same database using the fast `import_from` path.
- **CLI `migrate --src-db PATH --src-ns NS [--dst-db PATH] [--dst-ns NS]`** —
  migrate a namespace from one database file to another (or to a different
  namespace within the same file), with optional version filtering.

---

## [0.9.0] — 2025-05-03
`export_entries` / `import_entries` / `import_from`, CLI `export` / `import`,
nested brace expansion.

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

[Unreleased]: https://github.com/nabbisen/localcache-rs/compare/v0.10.0...HEAD
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
