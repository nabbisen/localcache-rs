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
| `InvalidPath { path }` | Path cannot be represented as an exact SQLite `TEXT` key | Use a valid UTF-8 filesystem/database path |
| `ReadOnly` | Mutation requested on a read-only engine | Use a deliberately writable engine if mutation is intended |
| `UnknownEncoding(String)` | Stored encoding tag not recognised | Wrong feature enabled for decoding |
| `PayloadVersionMismatch { stored, expected }` | Version tag mismatch | Call `purge_stale_versions()` |
| `EncryptionError(String)` *(encryption)* | Wrong key or corrupt data | Verify encryption key |
| `AsyncTaskPanicked` *(async / async-std / smol)* | `spawn_blocking` task panicked | Check payload type and encoding |

## Common patterns

### Read-only open

Read-only mode accepts only an existing database with the exact current
schema. It does not create an empty database or migrate a historical one.
Recognized historical or empty schemas return `UnsupportedFeature`; future or
malformed schemas retain the strict unrecognized-schema error. Reopen writable
only when initialization or migration is intentional and normal backup and
migration planning has been completed.

### Graceful miss handling

```rust
match engine.get_if_fresh("file.txt") {
    Ok(Some(entry)) => use_payload(entry.payload),
    Ok(None)        => compute_and_store()?,
    Err(e)          => eprintln!("cache error: {e}"),
}
```

### Ignoring missing files

`FileNotFound` is normal when a source-required operation such as `set` races
with deletion. It means the operation could not read the source; it does not
describe whether an exact stored cache key exists.

Read/delete operations can still address a deleted source by its exact valid
UTF-8 stored key. Freshness methods report it as missing. Relative, symlink,
basename, and suffix aliases are not guessed after deletion.

Malformed or resource-excessive glob patterns use `UnsupportedFeature` with a
stable non-echoing message. Scan and query terminals validate these patterns
before walking a directory or starting database work.

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
