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
    shared_cache:           bool,         // RFC 0004: shared page-cache read-only mode
    payload_version:        u32,
    max_entries:            Option<usize>,
    watch_dirs:             bool,         // watching feature: directory-level watching
    compress_payloads:      bool,         // compression feature
    encryption_key:         Option<Vec<u8>>,  // encryption feature
}
```

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

### Canonicalization contract

Every API method that accepts a file path (`set`, `get`, `get_if_fresh`,
`remove`, `contains`, `check_status`, …) calls `Path::canonicalize()` on the
input before touching the database.  The stored key is therefore the
**canonical absolute path at write time**.

Consequences:

- **Relative paths** resolve to the same entry as their absolute equivalent.
- **Symlinks** resolve to their target's canonical path.
- **Case variants** on case-insensitive filesystems (Windows, default macOS)
  resolve to the on-disk casing, so `set("File.TXT")` and `get("file.txt")`
  refer to the same entry.

### Fallback for deleted files

When the input file no longer exists on disk, canonicalization fails.
`get`, `contains`, `remove`, and `check_status` fall back to the **raw path
string** for the lookup, so entries for deleted files remain accessible:

```rust
let path = std::path::Path::new("/data/old_file.txt");
engine.set(path, &payload)?;    // file exists, canonical path stored
std::fs::remove_file(path)?;    // file gone from disk

// Entry is still accessible via the raw path fallback:
assert!(engine.contains(path)?);
assert!(engine.remove(path)?);
```

**Practical rule:** always go through the `localcache` API.  Do not compare
stored path strings directly — your input paths are canonicalized before
storage, and any future lookup with a different form (relative, symlinked,
differently cased) will resolve correctly.

### `cleanup_missing_files` semantics

`cleanup_missing_files()` iterates stored path strings and calls
`Path::exists()` on each one **without re-canonicalizing**.

On case-insensitive filesystems, a file renamed only by case still satisfies
`exists()` — its entry is therefore **preserved**, which is the correct
outcome (the original canonical path still resolves to the file).  Use
`check_status()` per entry if you need to detect case-only renames explicitly.
