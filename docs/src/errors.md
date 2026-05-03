# Error Handling

All `localcache` operations return `Result<T, LocalFileCacheError>`.

## Error variants

| Variant | Cause | Recovery |
|---|---|---|
| `Database(rusqlite::Error)` | SQLite error | Check path permissions; file not corrupt |
| `Io(std::io::Error)` | File read/write failure | Check file existence and permissions |
| `Serialization(String)` | bincode encode/decode failed | Payload type mismatch; check `payload_version` |
| `FileNotFound { path }` | Source file does not exist | Normal — check with `check_status` first |
| `UnsupportedFeature(String)` | Feature or operation not available | Check Cargo features; read the message |
| `InvalidPath { path }` | Path cannot be canonicalised | Ensure the path string is valid UTF-8 |
| `ReadOnly` | Write on a read-only engine | Open without `.read_only()` |
| `UnknownEncoding(String)` | Stored encoding tag not recognised | Wrong feature enabled for decoding |
| `PayloadVersionMismatch { stored, expected }` | Version tag mismatch | Call `purge_stale_versions()` |
| `EncryptionError(String)` *(encryption)* | Wrong key or corrupt data | Verify encryption key |
| `AsyncTaskPanicked` *(async)* | `spawn_blocking` task panicked | Check payload type and encoding |

## Common patterns

### Graceful miss handling

```rust
match engine.get_if_fresh("file.txt") {
    Ok(Some(entry)) => use_payload(entry.payload),
    Ok(None)        => compute_and_store()?,
    Err(e)          => eprintln!("cache error: {e}"),
}
```

### Ignoring missing files

`FileNotFound` is normal when a file has been deleted — it just means
the cache has no entry for that path.

```rust
use localcache::LocalFileCacheError;

match engine.set("file.txt", &payload) {
    Ok(()) => {}
    Err(LocalFileCacheError::FileNotFound { .. }) => {
        // File was deleted between check and set — skip.
    }
    Err(e) => return Err(e.into()),
}
```

### Version migration

When `payload_version` is bumped, old entries return
`PayloadVersionMismatch`.  Purge them all at once:

```rust
let purged = engine.purge_stale_versions()?;
println!("purged {purged} outdated entries");
```

### Encryption key errors

```rust
use localcache::LocalFileCacheError;

match engine.get("file.txt") {
    Err(LocalFileCacheError::EncryptionError(msg)) => {
        eprintln!("wrong key or corrupt data: {msg}");
    }
    other => { other?; }
}
```

## Using `?` with custom error types

`LocalFileCacheError` implements `std::error::Error`, so it converts into
`Box<dyn Error>` automatically:

```rust
fn process() -> Result<(), Box<dyn std::error::Error>> {
    let engine = CacheEngine::<Vec<f32>>::builder()
        .database("cache.sqlite3")
        .build()?;
    engine.set("file.txt", &vec![1.0])?;
    Ok(())
}
```

For `anyhow`:

```rust
fn process() -> anyhow::Result<()> {
    engine.set("file.txt", &vec![1.0])?;
    Ok(())
}
```
