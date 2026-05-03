# API Overview

This page summarises the main public types and their relationships.
For full method-level documentation see [docs.rs/localcache](https://docs.rs/localcache).

## Core types

```
CacheEngine<T>          — the main entry point
  ├── builder()         → CacheEngineBuilder<T>   (fluent configuration)
  ├── open(opts)        → CacheEngine<T>           (direct construction)
  ├── get / get_if_fresh / set / remove
  ├── batch_get / batch_get_fresh / batch_set
  ├── check_status / check_status_batch / contains
  ├── keys / list_entries / entry_count / cache_stats
  ├── preload           → PreloadReport
  ├── explain           → Diagnosis
  ├── query()           → QueryBuilder<T>          (json feature)
  ├── export_entries / import_entries / import_from / namespace_copy
  ├── namespace_list
  ├── touch / cleanup_missing_files / cleanup_expired / shrink_database
  ├── purge_stale_versions
  ├── create_path_index / drop_path_index / list_path_indexes
  ├── rotate_encryption_key                        (encryption feature)
  ├── watcher()         → CacheWatcher<T>          (watching feature)
  └── debounced_watcher() → CacheDebouncedWatcher<T>  (watching feature)
```

## Feature-gated types

| Type | Feature | Description |
|---|---|---|
| `AsyncCacheEngine<T>` | `async` | Tokio-based async wrapper |
| `ConnectionPool<T>` | *(none)* | Thread-safe sync pool |
| `CacheWatcher<T>` | `watching` | OS-native file-system watcher |
| `CacheDebouncedWatcher<T>` | `watching` | Debounced watcher |
| `QueryBuilder<T>` | `json` (predicates) | Payload-content query builder |

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
    payload_version:        u32,
    max_entries:            Option<usize>,
    compress_payloads:      bool,   // compression feature
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
