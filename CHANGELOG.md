# Changelog

All notable changes to this project will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

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

[Unreleased]: https://github.com/nabbisen/localcache-rs/compare/v0.14.0...HEAD
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
