# Migration Guide

## 0.13.x → 0.14.x (bincode 1.x → 2.x)

`localcache 0.14` upgraded `bincode` from 1.3.3 to 2.0.1.  The new version
uses `config::legacy()` which produces **byte-identical output** to bincode
1.x.  **Existing SQLite databases require no migration.**

The only breaking change is in the error type:
`LocalFileCacheError::Serialization` now wraps `String` instead of
`Box<bincode::ErrorKind>`.  If you matched on the inner value, update your
code:

```rust
// Before (0.13.x and earlier):
Err(LocalFileCacheError::Serialization(e)) => {
    // e: Box<bincode::ErrorKind>
    eprintln!("bincode error: {e:?}");
}

// After (0.14+):
Err(LocalFileCacheError::Serialization(msg)) => {
    // msg: String
    eprintln!("serialisation error: {msg}");
}
```

## Bumping `payload_version`

When your computation logic changes (new model, different analysis),
increment `payload_version` to force recomputation of all cached entries:

```rust
// Old code — version 1.
let engine = CacheEngine::<Vec<f32>>::builder()
    .payload_version(1)
    .build()?;

// New code — version 2.
let engine = CacheEngine::<Vec<f32>>::builder()
    .payload_version(2)
    .build()?;

// Purge all version-1 entries.
let purged = engine.purge_stale_versions()?;
println!("purged {purged} outdated entries");
```

## Migrating between databases

Use `export` / `import` or `migrate`:

```sh
# Export from old database.
localcache -d old.sqlite3 export -o backup.jsonl

# Import into new database.
localcache -d new.sqlite3 import -i backup.jsonl

# Or in one command:
localcache -d old.sqlite3 export | localcache -d new.sqlite3 import
```

Programmatically:

```rust
let src = CacheEngine::<T>::builder().database("old.sqlite3").build()?;
let dst = CacheEngine::<T>::builder().database("new.sqlite3").build()?;
let copied = dst.import_from(&src)?;
```

## Moving from `CacheOptions::open` to builder

Before (still works, not deprecated):

```rust
let engine = CacheEngine::<Vec<f32>>::open(CacheOptions {
    database_path: "cache.sqlite3".into(),
    change_detection_mode: ChangeDetectionMode::MetadataThenFullHash,
    ..CacheOptions::default()
})?;
```

After (recommended):

```rust
let engine = CacheEngine::<Vec<f32>>::builder()
    .database("cache.sqlite3")
    .change_detection(ChangeDetectionMode::MetadataThenFullHash)
    .build()?;
```

Both are equivalent — the builder simply populates a `CacheOptions` struct.

## Schema migrations

`localcache` handles schema upgrades automatically on `open()`.  Databases
from any version back to v0.1 are migrated transparently.

| DB version | localcache version | Change |
|---|---|---|
| 1 | 0.1 | Initial schema |
| 2 | 0.2 | Added `namespace` column |
| 3 | 0.4 | Added `payload_version`, `encoding` |
| 4 | 0.6 | Added `last_accessed_at` + LRU index |
