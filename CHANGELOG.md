# Changelog

All notable changes to this project will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

---

## [0.11.0] — 2025-05-03

### Added

- **`QueryBuilder::order_by_field(field_path, ascending)`** — sort results by
  the value of a dot-separated JSON payload field.  Non-numeric / absent fields
  are placed at the end.
- **`QueryBuilder::order_by_updated_at(ascending)`** — sort by the `mtime`
  metadata field (proxy for `updated_at`).
- **`QueryBuilder::order_by_path(ascending)`** — sort by stored path string.
- **`QueryBuilder::offset(n)`** — skip the first `n` matching entries before
  applying `limit`.  Enables cursor-free pagination:
  `query().order_by_field("score", false).limit(10).offset(page * 10).run()`.
- **`SortOrder`** enum (`Asc` / `Desc`) — new public type.
- **`AsyncCacheEngine::query_run(|q| build_q)`** — execute a query built from
  a closure on the async blocking thread pool.  The closure receives a
  `QueryBuilder<'_, T>` and must return one.
- **`CacheEngine::touch(path)`** — update `last_accessed_at` for `path` to the
  current time without loading the payload.  Useful for marking entries as
  recently used so they are not evicted by the LRU policy.  Returns `true` if
  the entry existed.
- **`CacheEngine::create_path_index(name)`** — create an additional SQLite
  index on `files(namespace, path)`.  The full name is `"lc_user_{name}"`.
  Idempotent (`IF NOT EXISTS`).  Returns the full index name.
- **`CacheEngine::drop_path_index(name)`** — drop a user-created index.
  Returns `true` if it existed.
- **`CacheEngine::list_path_indexes()`** — return a sorted list of all
  `"lc_user_*"` indexes.
- Async equivalents on `AsyncCacheEngine`: `touch`, `contains`, `keys`,
  `create_path_index`, `drop_path_index`, `list_path_indexes`.
- **CLI `query [--path-like PATTERN]`** — list matching stored entries with
  coloured status output, similar to `scan` but operating on the DB rather
  than the filesystem.

---

## [0.10.0] — 2025-05-03
`contains`, `keys`, `QueryBuilder` (predicates), CLI `copy` / `migrate`.

## [0.9.0] — 2025-05-03
`export_entries` / `import_entries` / `import_from`, CLI `export` / `import`,
nested brace expansion.

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

[Unreleased]: https://github.com/nabbisen/localcache-rs/compare/v0.11.0...HEAD
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
