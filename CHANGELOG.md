# Changelog

All notable changes to this project will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

---

## [0.8.0] — 2025-05-03

### Added

- **Cargo workspace** — the repository is now a workspace with two members:
  * `localcache` (the library crate, unchanged public API)
  * `localcache-cli` (new binary crate)
- **`localcache-cli`** — a standalone CLI tool for inspecting and maintaining
  localcache SQLite databases.  Build with `cargo build -p localcache-cli`.
  Global options: `-d / --database <PATH>` and `-n / --namespace <NS>`.
  Subcommands:
  * `list [--limit N]` — tabular listing of all entries (path, version,
    encoding, updated_at, last_access).
  * `stats` — aggregate statistics matching `CacheEngine::cache_stats()`.
  * `check <PATH>` — freshness status (`FRESH` / `STALE` / `MISSING`) for one file.
  * `cleanup` — remove entries whose source files are missing.
  * `vacuum` — run SQLite `VACUUM`.
  * `purge-version <VERSION>` — delete entries with a different payload version.
  * `scan <DIR> [--recursive] [--extensions txt,md] [--glob PATTERN]` — directory
    scan with coloured status output.
- **`CacheEngineBuilder::on_evict`** — register an `Fn(&Path) + Send + Sync`
  callback that is invoked **after** each LRU eviction triggered by
  `max_entries`.  The callback receives the canonical path that was stored.
- **Multi-group glob brace expansion** — `{a,b}_{c,d}.txt` now expands to the
  full Cartesian product `["a_c.txt","a_d.txt","b_c.txt","b_d.txt"]` rather
  than expanding only the first group.  The recursive implementation handles
  any number of groups.
- **`EvictCallback` type alias** — `pub(crate) type EvictCallback = Arc<dyn
  Fn(&Path) + Send + Sync>` reduces boilerplate in the engine and builder.

---

## [0.7.0] — 2025-05-02
Builder API, `cache_stats`, `check_status_batch`, key rotation, single-group
glob brace expansion.

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

[Unreleased]: https://github.com/nabbisen/localcache-rs/compare/v0.8.0...HEAD
[0.8.0]: https://github.com/nabbisen/localcache-rs/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/nabbisen/localcache-rs/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/nabbisen/localcache-rs/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/nabbisen/localcache-rs/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/nabbisen/localcache-rs/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/nabbisen/localcache-rs/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/nabbisen/localcache-rs/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/nabbisen/localcache-rs/releases/tag/v0.1.0
