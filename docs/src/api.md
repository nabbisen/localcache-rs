# API Overview

This page summarises the main public types and their relationships.
For full method-level documentation see [docs.rs/localcache](https://docs.rs/localcache).

## Core types

```
CacheEngine<T>          — the main entry point
  ├── builder()         → CacheEngineBuilder<T>   (fluent configuration)
  │     └── build_read_pool(n) → ReadPool<T>       (read-only pool)
  ├── open(opts)        → CacheEngine<T>           (direct construction)
  ├── get / get_if_fresh / set / remove
  ├── batch_get / batch_get_fresh / batch_set
  ├── check_status / check_status_batch / contains
  ├── keys / list_entries / entry_count / cache_stats
  ├── preload           → PreloadReport
  ├── explain           → Diagnosis
  ├── scan_dir / scan_dir_filtered
  ├── query()           → QueryBuilder<T>          (path filters always; payload predicates require json)
  ├── export_entries / import_entries / import_from
  ├── namespace_list
  ├── touch / cleanup_missing_files / cleanup_expired / shrink_database
  ├── purge_stale_versions
  ├── rotate_encryption_key                        (encryption feature)
  ├── watcher()         → CacheWatcher<T>          (watching feature)
  └── debounced_watcher() → CacheDebouncedWatcher<T>  (watching feature)

SyncCacheEngine<T>      — one engine behind a mutex; Clone + Send + Sync
  ├── open(opts) / with(|engine| …) / with_mut(|engine| …)   (escape hatch to the inner CacheEngine)
  ├── get / get_if_fresh / set / remove / touch / batch_get / batch_get_fresh / batch_set
  ├── check_status / check_status_batch / contains / explain
  ├── keys / list_entries / entry_count / entry_count_by_version / cache_stats
  ├── scan_dir / scan_dir_filtered / preload
  ├── export_entries / import_entries / import_from / namespace_list
  ├── cleanup_missing_files / cleanup_expired / purge_stale_versions / shrink_database
  ├── rotate_encryption_key                        (encryption feature)
  ├── watcher() / debounced_watcher(window)        (watching feature)
  └── query_run(|q| …) / query_run_report(|q| …) / query_dry_run(|q| …)

ReadPool<T>             — N read-only connections; Clone + Send + Sync
  ├── open(opts, size) / CacheEngineBuilder::build_read_pool(size)
  ├── get / get_if_fresh / batch_get / batch_get_fresh
  ├── check_status / check_status_batch / contains / explain
  ├── keys / list_entries / entry_count / entry_count_by_version / cache_stats / export_entries
  ├── namespace_list
  ├── scan_dir / scan_dir_filtered
  ├── query_run(|q| …) / query_run_report(|q| …) / query_dry_run(|q| …)
  └── size()

CacheWatcher<T>         (watching feature; via CacheEngine::watcher())
  ├── watch(path) / unwatch(path) / watch_dir(dir) / unwatch_dir(dir)
  ├── events()          → &Receiver<WatchEvent>
  ├── entry_count()     — entries currently cached in the watcher's engine; errors if its lock is poisoned
  ├── registration_errors() → &[PathRegistrationError]
  └── dropped_event_count() / failed_invalidation_count()

CacheDebouncedWatcher<T>   (watching feature; via CacheEngine::debounced_watcher(debounce))
  ├── watch_dir(dir) / unwatch_dir(dir)   (directory-only — no per-file watch/unwatch)
  ├── events()          → &Receiver<WatchEvent>
  ├── registration_errors() → &[PathRegistrationError]
  └── dropped_event_count() / failed_invalidation_count()
```

### What each wrapper exposes

`SyncCacheEngine` and `AsyncCacheEngine` expose **every** public `CacheEngine`
method under the same name, with these exceptions:

- `builder`: use each wrapper's `open`.
- `query`: its builder borrows the engine. Use `query_run`, `query_run_report` and
  `query_dry_run`.
- `AsyncCacheEngine::import_from`: its source is a `&CacheEngine`, which cannot
  cross the `spawn_blocking` boundary. Copy with `export_entries` on the
  source and `import_entries` on the destination.

`ReadPool` exposes every method that never writes. A method that writes is
absent from it, and that includes the watchers, because a watcher removes
entries when a file changes. A test (`crates/localcache/tests/api_surface.rs`) compares the four surfaces
and fails when a new engine method is not delegated or recorded as an exception.

### Path indexes

Earlier releases could create extra `lc_user_…` indexes on `(namespace, path)`.
They duplicate the built-in unique index on that pair, so they cannot make any
query faster, and creating them is deprecated. An index created by an earlier
release keeps working and can be removed; the migration table below names the
method that does it.

### `ReadPool` poisoning (v0.21.0)

Every `ReadPool<T>` read method already returns `Result<_, LocalFileCacheError>`
(or `Vec<Result<_, LocalFileCacheError>>` for the batch methods); no method's
type signature changed. What changed is behaviour: a poisoned connection slot
previously recovered silently (`unwrap_or_else(|e| e.into_inner())`) and now
returns `LocalFileCacheError::Poisoned { resource: "ReadPool" }` instead — for
the batch methods, one such error per requested path. See
[Error Handling](./errors.md) for the full migration note.

## Feature-gated types

| Type | Feature | Description |
|---|---|---|
| `AsyncCacheEngine<T>` | `async` / `async-std` / `smol` | Async wrapper (runtime-selectable) |
| `SyncCacheEngine<T>` | *(none)* | Thread-safe sync engine (one engine behind a mutex) |
| `ReadPool<T>` | *(none)* | Thread-safe sync pool of N read-only connections |
| `CacheWatcher<T>` | `watching` | OS-native file-system watcher |
| `CacheDebouncedWatcher<T>` | `watching` | Debounced watcher |
| `QueryBuilder<T>` | `json` *(payload predicates only)* | Path filters always available; payload predicates require `json` |

## Public structs

| Type | Description |
|---|---|
| `CacheEntry<T>` | Payload + path + metadata |
| `EntryInfo` | Metadata only (no payload) |
| `FileMetadata` | mtime (nanoseconds) + file_size + optional hash, the public-facing shape of on-disk metadata |
| `CacheStats` | Aggregate DB statistics |
| `PreloadReport` | Results from `preload()` |
| `ExportRecord` | Portable serialised entry |
| `Diagnosis` | Staleness diagnostic report |
| `MetadataDiff` | mtime / file_size comparison |
| `PayloadVersionInfo` | Version stored vs expected |
| `BatchSetReport` | Results from `batch_set()` |
| `QueryReport<T>` | Result of `run_report()`: `entries`, and the `skipped` entries that could not be decoded |
| `SkippedEntry` | One undecodable entry: its `path` and the `error` decoding it produced |
| `WatchEvent` *(watching)* | File-system invalidation event |
| `PathRegistrationError` *(watching)* | One path that failed OS-level watch registration at construction time; see `registration_errors()` |

## Public enums

| Type | Variants |
|---|---|
| `CacheStatus` | `Fresh`, `Stale`, `Missing` |
| `ChangeDetectionMode` | `MetadataOnly`, `MetadataThenPartialHash`, `MetadataThenFullHash`, `StrictFullHash` |
| `Codec` | `Bincode`, `Json` |
| `JournalMode` | `Wal`, `Delete`, `Memory` |
| `SynchronousMode` | `Off`, `Normal`, `Full`, `Extra` |
| `InvalidationReason` | `FileModified`, `FileRemoved`, `FileRenamed` |
| `SortKey` | `Field(String)` *(json)*, `Mtime`, `LastAccessed`, `Path` — `#[non_exhaustive]` |
| `SortOrder` | `Asc`, `Desc` |
| `LocalFileCacheError` | *see [Error Handling](./errors.md)* |

## `CacheOptions`

Direct struct for `CacheEngine::open()`.  The builder API mirrors all
these fields as typed methods.

```rust
CacheOptions {
    database_path:          PathBuf,
    change_detection_mode:  ChangeDetectionMode,
    codec:                  Codec,
    journal_mode:           JournalMode,
    synchronous:            SynchronousMode,
    ttl:                    Option<Duration>,
    namespace:              String,
    read_only:              bool,
    shared_cache:           bool,         // RFC 004: shared page-cache read-only mode
    payload_version:        u32,
    max_entries:            Option<usize>,
    watch_dirs:             bool,         // watching feature: directory-level watching
    compress_payloads:      bool,         // compression feature
    encryption_key:         Option<Vec<u8>>,  // encryption feature
}
```

`read_only` accepts only an existing file-backed database with the exact
current schema. It never initializes or migrates the database, and every
mutating method returns `LocalFileCacheError::ReadOnly`. Pure reads skip the
LRU timestamp update. For file-backed databases, `shared_cache` implies this
same read-only contract.

## `ScanOptions`

Controls directory scanning in `scan_dir_filtered()` and `preload()`.

```rust
ScanOptions {
    recursive:     bool,
    max_depth:     Option<usize>,
    extensions:    Vec<String>,   // e.g. vec!["txt".into(), "md".into()]
    glob_pattern:  Option<String>, // e.g. "*.{txt,md}"
}
```

## Path handling

### Canonicalization and stored-key contract

When a source exists, path-taking APIs use its exact valid UTF-8 canonical
path. Normal `set` operations therefore write a **canonical absolute path** as
the database key. Portable records supplied to `import_entries` retain their
exact valid UTF-8 stored key instead of being rewritten.

Consequences:

- **Relative paths** resolve to the same entry as their absolute equivalent.
- **Symlinks** resolve to their target's canonical path.
- **Case variants** on case-insensitive filesystems (Windows, default macOS)
  resolve to the on-disk casing, so `set("File.TXT")` and `get("file.txt")`
  refer to the same entry.

### Exact access after deletion

When a source no longer exists, `get`, `contains`, `remove`, and `explain`
look up only the caller's **exact stored key**. They never guess using a
basename, suffix, former symlink, relative alias, lossy conversion, or case
variant. `get_if_fresh` returns `None`, `check_status` returns `Missing`, and
`touch` returns `false` because freshness and warming require a source.

```rust
let path = std::path::Path::new("/data/old_file.txt").canonicalize()?;
engine.set(&path, &payload)?; // canonical key stored
std::fs::remove_file(&path)?;

// The retained exact stored key still works:
assert!(engine.contains(&path)?);
assert!(engine.remove(&path)?);
```

**Practical rule:** retain the path returned by a cache entry, `keys`,
`list_entries`, or a query when post-deletion access matters. While a source
exists, relative and symlink paths still resolve to its canonical key. After
deletion, aliases cannot be reconstructed because they were never stored.

SQLite schema v5 stores path identities as `TEXT`. A path that is not valid
UTF-8 returns `InvalidPath`; localcache never uses a lossy string as a key.

### `cleanup_missing_files` semantics

`cleanup_missing_files()` iterates stored path strings and calls
`Path::exists()` on each one **without re-canonicalizing**.

On case-insensitive filesystems, a file renamed only by case still satisfies
`exists()` — its entry is therefore **preserved**, which is the correct
outcome (the original canonical path still resolves to the file).  Use
`check_status()` per entry if you need to detect case-only renames explicitly.

## Migrating from deprecated names

v0.21.5 deprecates the items below. Each still works and behaves as before;
each is removed in v0.22.0 unless the last column says otherwise. The compiler
warning names the replacement.

| Deprecated | Use instead | Removal |
|---|---|---|
| `QueryBuilder::order_by_field(path, asc)` | `order_by(SortKey::Field(path.into()), SortOrder::…)` | removed in 0.22.0 |
| `QueryBuilder::order_by_updated_at(asc)` | `order_by(SortKey::Mtime, SortOrder::…)` — it sorts by the source file's mtime, not by `updated_at` | removed in 0.22.0 |
| `QueryBuilder::order_by_last_accessed(asc)` | `order_by(SortKey::LastAccessed, SortOrder::…)` | removed in 0.22.0 |
| `QueryBuilder::order_by_path(asc)` | `order_by(SortKey::Path, SortOrder::…)` | removed in 0.22.0 |
| `QueryBuilder::then_by_field(path, asc)` | `then_by(SortKey::Field(path.into()), SortOrder::…)` | removed in 0.22.0 |
| `QueryBuilder::then_by_updated_at(asc)` | `then_by(SortKey::Mtime, SortOrder::…)` | removed in 0.22.0 |
| `QueryBuilder::then_by_last_accessed(asc)` | `then_by(SortKey::LastAccessed, SortOrder::…)` | removed in 0.22.0 |
| `QueryBuilder::then_by_path(asc)` | `then_by(SortKey::Path, SortOrder::…)` | removed in 0.22.0 |
| `ConnectionPool<T>` | `SyncCacheEngine<T>` — same type, new name; `Poisoned { resource }` still reads `"ConnectionPool"` until 0.22.0 | removed in 0.22.0 |
| `SharedEngine<T>` | `SyncCacheEngine<T>` | removed in 0.22.0 |
| `shared_engine(opts)` | `SyncCacheEngine::open(opts)`; `with` / `with_mut` reach the inner `CacheEngine` | removed in 0.22.0 |
| `CacheEngine::namespace_copy(src)` | `import_from(src)` — identical | removed in 0.22.0 |
| `CacheWatcher::watched_count()` | `entry_count()` — returns the engine's entry count, not a count of watched paths, and reports a poisoned lock as an error instead of `0` | removed in 0.22.0 |
| `CacheEngine::create_path_index(suffix)` | nothing — the index cannot speed up a query | removed in 0.22.0 |
| `QueryBuilder::index_hint(name)` | nothing — drop the call | removed in 0.22.0 |
| `CacheEngine::drop_path_index(suffix)` | keep using it to remove an index created by an earlier release | kept until a later release decides what happens to existing indexes |
| `CacheEngine::list_path_indexes()` | keep using it to find indexes created by an earlier release | kept until a later release decides what happens to existing indexes |
| `AsyncCacheEngine::create_path_index` / `drop_path_index` / `list_path_indexes` | same as the synchronous methods above | as the synchronous method |
