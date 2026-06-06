# Changelog

All notable changes to this project will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

---

## [0.16.2] — 2026-05-05

### Added

- **`rfcs/` directory** — implementation-ready design specifications for
  planned features, written in English to match the rest of `docs/`.

  | RFC | Title | Template |
  |-----|-------|----------|
  | [0001](rfcs/0001-recursive-directory-watching.md) | Recursive Directory Watching | Lightweight |
  | [0002](rfcs/0002-query-index-hints.md) | Query Index Hints and Explain Plan | Full |
  | [0003](rfcs/0003-opentelemetry-spans.md) | OpenTelemetry Spans | Full |
  | [0004](rfcs/0004-shared-memory-db.md) | Read-only Shared-memory DB Mode | Full |
  | [0005](rfcs/0005-async-std-smol.md) | async-std / smol Feature Variants | Full |

  Each RFC covers: summary, motivation, public API surface, internal
  design with code sketches, test plan, and (where applicable) security
  considerations and open questions.

---

## [0.16.1] — 2026-05-04

### Changed

- `src/tests.rs` (4 722 lines) split into 8 thematic integration-test files
  under `tests/` alongside `tests/common/mod.rs` for shared helpers.
  Each file is an independent test binary; individual suites can be run with
  e.g. `cargo test --test core`.

  | File | Theme | Tests |
  |---|---|---|
  | `tests/core.rs` | Basic ops, namespaces, TTL | 26 |
  | `tests/storage.rs` | Hashing, scan, schema migration | 30 |
  | `tests/codec_lru.rs` | JSON codec, LRU, glob, encryption | 31 |
  | `tests/builder_ops.rs` | Builder, stats, brace expansion, on_evict | 24 |
  | `tests/portability.rs` | Export / import | 8 |
  | `tests/query.rs` | QueryBuilder, ordering, touch, indexes | 30 |
  | `tests/pool_observe.rs` | ConnectionPool, explain, tracing | 20 |
  | `tests/watching.rs` | Preload, watcher, debounce, metrics, namespace | 19 |

  Total: **188** integration tests + **15** doc-tests (unchanged).

- Two `pub(crate)` internal references removed from the old `src/tests.rs`
  before moving to `tests/` (required for top-level test binaries):
  * `crate::detection::hash::PARTIAL_PREFIX` → literal `"partial:"`
  * `crate::serialization::serialize_bincode` →
    `bincode::serde::encode_to_vec(..., bincode::config::legacy())`

- Unused-import warnings eliminated across all source files and new test
  files; all feature combinations are now warning-free.

---

## [0.16.0] — 2026-05-04

### Changed

- **Documentation overhaul** — the `docs/` mdBook has been completely
  rewritten to reflect the v0.15 API.  All chapters now contain working
  code examples, accurate type names, and feature-flag annotations.

  New chapters added:
  * **Features** — per-feature guide with code examples for all 7 flags.
  * **Builder API** — every `CacheEngineBuilder` method documented.
  * **Async & Thread Safety** — `AsyncCacheEngine`, `ConnectionPool`,
    `shared_engine`, SQLite concurrency model, and a decision table.
  * **Querying the Cache** — `QueryBuilder` predicates, multi-column sort,
    pagination, `explain()`, and namespace management.
  * **File Watching** — `CacheWatcher`, `CacheDebouncedWatcher`, the
    watcher lifetime contract, platform table, and a thread-ownership pattern.
  * **Data Portability** — export/import, `import_from`, `preload()`,
    and glob pattern reference.
  * **Cookbook** — 7 complete recipes: embedding pipeline, multi-threaded
    web server, reactive pipeline, versioned cache, encrypted cache,
    TTL-based expiry, metrics with Prometheus.
  * **CLI Tool** — all 17 subcommands with flags and output examples.
  * **Migration Guide** — bincode 1→2 upgrade, `payload_version` bump,
    DB migration, builder migration, schema version history.
  * **Changelog Summary** — phase-by-phase history from 0.1 to 0.15.

  Updated chapters:
  * **Introduction** — feature comparison table, quick links.
  * **Getting Started** — updated to use builder API, added `preload`.
  * **Change Detection** — all 4 modes, decision table, `explain()` usage.
  * **Architecture** — schema v4, encoding tags, write/read path diagrams.
  * **Error Handling** — full variant table including all feature-gated errors.
  * **API Overview** — complete type catalogue for v0.15.
  * **Roadmap** — completed phases table.

- `docs/book.toml` — repository URL updated to
  `https://github.com/nabbisen/localcache-rs`; search and fold enabled.
- `docs/src/SUMMARY.md` — restructured into **User Guide** / **Reference** /
  **Development** sections.

---

## [0.15.0] — 2026-05-04

### Added

- **`metrics` Cargo feature** — opt-in instrumentation using
  [`metrics 0.24`](https://crates.io/crates/metrics).  When enabled,
  the following counters and histograms are emitted (all labelled
  `namespace = <current namespace>`):
  * `localcache.get.total` — every `get()` call.
  * `localcache.get.hit` — cache hits in `get()`.
  * `localcache.get.miss` — cache misses in `get()`.
  * `localcache.set.total` — every `set()` call.
  * `localcache.set.bytes` — payload size in bytes per `set()`.
  Wire up any `metrics`-compatible exporter (Prometheus, StatsD, …) to
  consume these metrics without changing the `localcache` API.
- **`CacheEngine::namespace_list()`** — returns all distinct namespace
  names present in the current database, sorted alphabetically.
- **`CacheEngine::namespace_copy(source)`** — copy all entries from
  `source` (any `CacheEngine<U>`) into the current engine's namespace,
  replacing conflicts.  Equivalent to `import_from` with a more
  discoverable name for namespace-management workflows.
- **`CacheEngine::debounced_watcher(window)`** → `CacheDebouncedWatcher<T>`
  — a debounced variant of the file-system watcher (requires `watching`
  feature).  All OS events within `window` of each other for the same path
  are collapsed into a single `WatchEvent`, preventing floods caused by
  editors that write files incrementally or applications that flush many
  times per second.
- **`CacheDebouncedWatcher<T>`** — new public type (re-exported under
  `watching` feature).  Has the same `events() -> &Receiver<WatchEvent>`
  lifetime contract as `CacheWatcher`.
- **`notify-debouncer-mini 0.7`** added as an optional dependency (included
  by the `watching` feature alongside `notify 8`).
- **CLI `namespaces` subcommand** — prints a table of all namespaces in the
  database together with their entry counts.
- **`Makefile.toml`** — `cargo-make` task definitions for the full
  development and publish workflow:
  * `cargo make check` — format check + clippy (default, all features,
    no features).
  * `cargo make test-all` — test matrix (no features / default / all
    features).
  * `cargo make pre-publish` — all quality gates before releasing.
  * `cargo make release-check` — version consistency + changelog entry +
    all quality gates.
  * `cargo make publish-all` — publish library then CLI with a 30-second
    propagation delay.

---

## [0.14.0] — 2026-05-03

### Added

- **`watching` Cargo feature** — background file-system watcher using
  [`notify 8`](https://crates.io/crates/notify) (OS-native: `inotify` on
  Linux, `kqueue` on macOS, `ReadDirectoryChanges` on Windows).  Zero
  additional cost when the feature is disabled.
- **`CacheWatcher<T>`** — a background watcher tied to a `CacheEngine`.
  - Created via `CacheEngine::watcher()`.
  - Automatically registers all cached paths at construction time using a
    dedicated SQLite connection for the callback thread.
  - When a watched file is modified, renamed, or deleted, the corresponding
    cache entry is removed from the database and a `WatchEvent` is sent on
    an internal `mpsc::sync_channel`.
  - `watch(path)` / `unwatch(path)` — add/remove paths at runtime.
  - `events()` — borrow the `mpsc::Receiver<WatchEvent>` (watcher must stay
    alive while reading).
  - `watched_count()` — number of entries in the watcher's engine snapshot.
  - **Lifetime note**: dropping `CacheWatcher` stops the OS watcher and
    closes the channel.  Use `events()` (which borrows) rather than
    `into_receiver()` to keep the watcher alive.
- **`WatchEvent { path, reason }`** — new public type.
- **`InvalidationReason`** enum — `FileModified` | `FileRemoved` |
  `FileRenamed`.
- **`CacheEngine::preload(dir, opts, force, factory)`** — bulk-cache all
  files in a directory using a user-supplied `factory` closure:
  * `force = false` — skips entries that are already fresh (cheap check
    before calling `factory`).
  * `force = true` — recomputes every file unconditionally.
  * Returns a `PreloadReport`.
- **`PreloadReport { stored, already_fresh, skipped, errors }`** — new
  public type summarising the preload outcome; per-file error strings for
  skipped files.
- **CLI `watch` subcommand** — prints live invalidation events in the format
  `[YYYY-MM-DD HH:MM:SS] MODIFIED /path/to/file`.  Press Ctrl-C to stop.
  Gracefully exits with an error message when the `watching` feature is not
  compiled in.
- **`watching` feature in `localcache-cli`** — opt-in:
  `cargo build -p localcache-cli --features watching`.

### Dependencies

- `notify = "8"` added as optional dependency (`watching` feature).

---

## [0.13.2] — 2026-05-03

### Changed

- **`bincode` upgraded from 1.3.3 to 2.0.1** with `features = ["serde"]`.
  - `bincode 3.0.0` on crates.io is an intentional stub (see [xkcd #2347](https://xkcd.com/2347/))
    and not a real release; `2.0.1` is the actual latest stable version.
  - All encode/decode calls migrated to `bincode::serde::encode_to_vec` /
    `bincode::serde::decode_from_slice` with `bincode::config::legacy()`.
  - **Wire format is backward-compatible**: `config::legacy()` produces
    byte-for-byte identical output to bincode 1.x, so existing SQLite caches
    require no migration.
  - `LocalFileCacheError::Serialization` inner type changed from
    `Box<bincode::ErrorKind>` to `String` (the bincode 2.x error types are
    not the same as 1.x; `String` avoids exposing an upstream implementation
    detail in the public API).

---

## [0.13.1] — 2026-05-03

### Changed

- Updated repository URL to <https://github.com/nabbisen/localcache-rs> across
  all manifests and documentation.
- `rusqlite` dependency bumped from 0.32 to **0.39** (no API changes to the
  localcache public interface).
- `criterion` dev-dependency bumped from 0.5 to **0.8**; migrated
  `criterion::black_box` → `std::hint::black_box` as required by the new version.
- `base64` bumped from 0.22.0 to **0.22.1** (patch).
- Workspace-level package metadata (`[workspace.package]`) introduced: `version`,
  `edition`, `rust-version`, `authors`, `license`, `repository` are now declared
  once in the root `Cargo.toml` and inherited by `localcache-cli`.
- `criterion::black_box` replaced with `std::hint::black_box` in bench suite.
- `.gitignore`, `LICENSE`, `README.md`, and `.github/` refreshed.
- `NOTICE` copyright year updated to 2026.
- `.github/workflows/ci.yaml` added: matrix test across 7 feature combinations,
  bench compile check, and MSRV check — all using `dtolnay/rust-toolchain@stable`
  so the suite tracks the current stable release rather than a pinned version.
- GitHub Actions versions updated across both workflow files:
  | Action | Version |
  |---|---|
  | `actions/checkout` | **v6** |
  | `actions/cache` | **v5** (Node.js 24, new cache service v2) |
  | `actions/configure-pages` | **v6** |
  | `actions/upload-pages-artifact` | **v5** |
  | `actions/deploy-pages` | **v5** |
  | `dtolnay/rust-toolchain` | **@stable** (was @1.85) |

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

[Unreleased]: https://github.com/nabbisen/localcache-rs/compare/v0.16.2...HEAD
[0.16.2]: https://github.com/nabbisen/localcache-rs/compare/v0.16.1...v0.16.2
[0.16.1]: https://github.com/nabbisen/localcache-rs/compare/v0.16.0...v0.16.1
[0.16.0]: https://github.com/nabbisen/localcache-rs/compare/v0.15.0...v0.16.0
[0.15.0]: https://github.com/nabbisen/localcache-rs/compare/v0.14.0...v0.15.0
[0.14.0]: https://github.com/nabbisen/localcache-rs/compare/v0.13.2...v0.14.0
[0.13.2]: https://github.com/nabbisen/localcache-rs/compare/v0.13.1...v0.13.2
[0.13.1]: https://github.com/nabbisen/localcache-rs/compare/v0.13.0...v0.13.1
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
