# API Reference

Full Rust doc-comments are available via `cargo doc --open`.  This page
provides a quick overview.

## Types

### `CacheEngine<T>`

The main handle.  Open one with `CacheEngine::open(options)`.

`T` must implement `serde::Serialize + serde::de::DeserializeOwned`.

### `CacheOptions`

```rust
pub struct CacheOptions {
    pub database_path: PathBuf,            // default: "localcache.sqlite3"
    pub change_detection_mode: ChangeDetectionMode,  // default: MetadataOnly
}
```

### `ChangeDetectionMode`

```rust
pub enum ChangeDetectionMode {
    MetadataOnly,
    MetadataThenPartialHash,   // v0.1: falls back to full hash
    MetadataThenFullHash,
    StrictFullHash,
}
```

### `CacheEntry<T>`

```rust
pub struct CacheEntry<T> {
    pub path: PathBuf,
    pub metadata: FileMetadata,
    pub payload: T,
}
```

### `FileMetadata`

```rust
pub struct FileMetadata {
    pub mtime: i64,
    pub file_size: u64,
    pub hash: Option<String>,  // BLAKE3 hex, or None for MetadataOnly entries
}
```

### `CacheStatus`

```rust
pub enum CacheStatus { Fresh, Stale, Missing }
```

## Methods

| Method | Description |
|--------|-------------|
| `open(options)` | Open / create the cache database |
| `get(path)` | Load entry from DB (no staleness check) |
| `get_if_fresh(path)` | Load entry only if `Fresh` |
| `set(path, payload)` | Store or update an entry |
| `remove(path)` | Delete an entry; returns `true` if it existed |
| `check_status(path)` | Returns `Fresh`, `Stale`, or `Missing` |
| `cleanup_missing_files()` | Remove entries for files no longer on disk |
| `shrink_database()` | Run SQLite `VACUUM` |
