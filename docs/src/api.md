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
  ├── query()           → QueryBuilder<T>          (path filters always; payload predicates require json)
  ├── export_entries / import_entries / import_from / namespace_copy
  ├── namespace_list
  ├── touch / cleanup_missing_files / cleanup_expired / shrink_database
  ├── purge_stale_versions
  ├── create_path_index / drop_path_index / list_path_indexes
  ├── rotate_encryption_key                        (encryption feature)
  ├── watcher()         → CacheWatcher<T>          (watching feature)
  └── debounced_watcher() → CacheDebouncedWatcher<T>  (watching feature)

ReadPool<T>             — N read-only connections; Clone + Send + Sync
  ├── open(opts, size) / CacheEngineBuilder::build_read_pool(size)
  ├── get / get_if_fresh / batch_get / batch_get_fresh
  ├── check_status / check_status_batch / contains / explain
  ├── keys / list_entries / entry_count / cache_stats / export_entries
  ├── scan_dir / scan_dir_filtered
  ├── query_run(|q| …) / query_dry_run(|q| …)
  └── size()
```

### Path-index identifier boundary

`CacheEngine::create_path_index` accepts a 1–64 byte ASCII
alphanumeric/underscore suffix and returns the full `lc_user_…` name.
`drop_path_index` takes the suffix; `QueryBuilder::index_hint` takes a full
name and validates it at both `run()` and `dry_run()`. Discover public names
with `list_path_indexes()`. All operations authorize structurally valid
indexes in SQLite's `main` schema only; identifier-policy failures return
`LocalFileCacheError::UnsupportedFeature` without echoing caller input.

Valid legacy public indexes remain listable, usable, and removable even when
their names no longer satisfy the creation grammar. A removed legacy spelling
cannot necessarily be recreated.

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
| `ConnectionPool<T>` | *(none)* | Thread-safe sync pool (single connection) |
| `ReadPool<T>` | *(none)* | Thread-safe sync pool of N read-only connections |
| `CacheWatcher<T>` | `watching` | OS-native file-system watcher |
| `CacheDebouncedWatcher<T>` | `watching` | Debounced watcher |
| `QueryBuilder<T>` | `json` *(payload predicates only)* | Path filters always available; payload predicates require `json` |

## Public structs

| Type | Description |
|---|---|
| `CacheEntry<T>` | Payload + path + metadata |
| `EntryInfo` | Metadata only (no payload) |
| `CacheStats` | Aggregate DB statistics |
| `PreloadReport` | Results from `preload()` |
| `ExportRecord` | Portable serialised entry |
| `Diagnosis` | Staleness diagnostic report |
| `MetadataDiff` | mtime / file_size comparison |
| `PayloadVersionInfo` | Version stored vs expected |
| `BatchSetReport` | Results from `batch_set()` |
| `WatchEvent` | File-system invalidation event |

## Public enums

| Type | Variants |
|---|---|
| `CacheStatus` | `Fresh`, `Stale`, `Missing` |
| `ChangeDetectionMode` | `MetadataOnly`, `MetadataThenPartialHash`, `MetadataThenFullHash`, `StrictFullHash` |
| `Codec` | `Bincode`, `Json` |
| `JournalMode` | `Wal`, `Delete`, `Memory` |
| `SynchronousMode` | `Off`, `Normal`, `Full`, `Extra` |
| `InvalidationReason` | `FileModified`, `FileRemoved`, `FileRenamed` |
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
