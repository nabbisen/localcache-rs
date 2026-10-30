# Error Handling

All `localcache` operations return `Result<T, LocalFileCacheError>`.

`LocalFileCacheError` is `#[non_exhaustive]`: an exhaustive `match` without a
`_` arm fails to compile, so a future variant is never a breaking change.
Existing exhaustive matches must add a `_` arm to upgrade to v0.21.0.

## Error variants

| Variant | Cause | Recovery |
|---|---|---|
| `Database(DatabaseError)` | SQLite error; see [Database errors](#database-errors) | Check path permissions; file not corrupt; retry if busy |
| `Io(std::io::Error)` | File read/write failure | Check file existence and permissions |
| `Serialization(String)` | bincode or JSON encode/decode failed | Payload type mismatch; check `payload_version` |
| `FileNotFound { path }` | Source file does not exist | Normal — check with `check_status` first |
| `InvalidPath { path }` | Path cannot be represented as an exact SQLite `TEXT` key | Use a valid UTF-8 filesystem/database path |
| `ReadOnly` | Mutation requested on a read-only engine | Use a deliberately writable engine if mutation is intended |
| `UnknownEncoding(String)` | Stored encoding tag not recognised | Wrong feature enabled for decoding |
| `Poisoned { resource: &'static str }` | A lock guarding shared cache state was poisoned by a panic in another thread | The poisoning caller's bug, not yours — recreate the pool/engine/watcher |
| `EncryptionError(String)` *(encryption)* | Wrong key or corrupt data — *not* a missing feature or a missing key (see below) | Verify encryption key |
| `AsyncTaskPanicked` *(async / async-std / smol)* | `spawn_blocking` task panicked | Check payload type and encoding |
| `InvalidConfiguration { option, reason }` | A configuration option is invalid or unsupported: `option` is `database_path`, `read_only`, `encryption_key`, `size`, `max_entries` or `ttl`. Found before the database is touched | Fix the named option and open again; nothing was created or changed |
| `InvalidArgument { argument, reason }` | A call's argument is invalid: `argument` is `new_key`, `path_glob`, `index_name`, `records` or `items` | Correct that argument and call again |
| `SchemaNotCurrent` | A read-only open of a database whose schema is not the current one (older, or empty) | Open it writable once to migrate it, or use the CLI's `copy --from-db --upgrade-source`; the database was not modified |
| `IncompatibleSchema(String)` | The schema is newer than this version understands, is not a localcache database, or is damaged; the message says what was found | Use a version that matches the database, or restore a backup; the database was not modified |
| `MissingEncryptionKey` *(encryption)* | An operation needs an encryption key and none is configured: rotating a key, or reading an encrypted entry | Configure `encryption_key` on the engine |
| `DatabaseState(String)` | The database or connection is in a state this crate will not work with: an unsafe journal mode for a migration, SQLite not honouring a setting, a failed migration postcondition | Read the message; it says whether the database was left unchanged |
| `Watcher(String)` *(watching)* | Setting up or running a file-system watcher failed | Check that the path exists and the system's watch limits |

### v0.21.0 migration note

v0.21.0 makes `LocalFileCacheError` truthful about lock poisoning and JSON
codec failures:

- **Add a `_` arm** to any exhaustive `match` on `LocalFileCacheError` — the
  enum is now `#[non_exhaustive]`.
- **Lock poisoning now returns `Poisoned`**, not the catch-all error that
  release had (removed in v0.22.0, below). At
  v0.21.0 this affected `SyncCacheEngine` (then called `ConnectionPool`),
  `AsyncCacheEngine`, `CacheWatcher` construction, and `ReadPool` (a
  **behaviour change**: `ReadPool`'s read methods were previously infallible
  under poisoning and silently recovered; they now return `Poisoned`
  instead). `CacheWatcher` construction no longer has a lock to poison as of
  v0.21.4 (RFC 022 R9): the helper-connection lock that produced `Poisoned {
  resource: "CacheWatcher" }` was removed, not merely made harder to reach.
- **JSON codec failures now return `Serialization`**, not the catch-all
  error. Code matching the catch-all to catch JSON encode/decode errors stops
  matching.
- **No schema, payload wire format, SQL, or other method signature changed.**
  Recompiling — after adding the `_` arm and updating any matches on the
  catch-all error above — is the only work; existing databases open unchanged.

### v0.22.0 migration note

`LocalFileCacheError::Database` now wraps `DatabaseError`, an opaque type of this
crate. `rusqlite::Error` is no longer part of the public API, so code that
handles errors no longer depends on which `rusqlite` version this crate uses.

- A `Database(_)` arm needs no change, and the message text is unchanged.
- A match on the old payload, such as
  `Database(rusqlite::Error::SqliteFailure(code, _))`, becomes
  `Database(error)` with a test on `error.sqlite_extended_code()` (below).
- Another `rusqlite::Error` variant, for example `QueryReturnedNoRows`, has no
  counterpart: `sqlite_extended_code()` is `None` for it, and the message is its
  only description.

### v0.22.0 migration note: the `Poisoned` resource of `SyncCacheEngine`

A poisoned `SyncCacheEngine` lock returns `Poisoned { resource: "SyncCacheEngine" }`. Until v0.22.0 the
value was `"ConnectionPool"`, the type's earlier name, which v0.21.5 renamed while keeping the old
string because it is observable. Code that compared `resource` with `"ConnectionPool"` must compare it
with `"SyncCacheEngine"`. The other values (`"AsyncCacheEngine"`, `"ReadPool"`, `"CacheWatcher"`) are
unchanged.

### v0.22.0 migration note: purpose-named variants

v0.22.0 removes `UnsupportedFeature`, the catch-all error of earlier releases
(its message began `unsupported feature:`), and `PayloadVersionMismatch`, which
was never returned, and adds seven variants, so a condition can be recognised
without reading English. A compile error that names either removed variant
means the match needs the variant from the table below.
For these errors, which inputs succeed and which fail is **unchanged**; only the
variant and, for a few, the wording differ. (Three inputs that were accepted
before are rejected in the same release; they are listed after the table.)

| It used to be reported as `unsupported feature: …` because… | It is now | Match on |
|---|---|---|
| read-only requested with an in-memory database | `InvalidConfiguration` | `option: "read_only"` |
| the encryption key is not 32 bytes, at open | `InvalidConfiguration` | `option: "encryption_key"` |
| a `ReadPool` of size 0, or a `:memory:` database | `InvalidConfiguration` | `option: "size"`, `option: "database_path"` |
| `rotate_encryption_key` got a key that is not 32 bytes | `InvalidArgument` | `argument: "new_key"` |
| a malformed or over-limit `path_glob` | `InvalidArgument` | `argument: "path_glob"` |
| an index name that is not allowed | `InvalidArgument` | `argument: "index_name"` |
| an import record whose payload is not base64 | `InvalidArgument` | `argument: "records"` |
| rotating a key, or reading an encrypted entry, with no key configured | `MissingEncryptionKey` | the variant |
| a read-only open of an older or empty database | `SchemaNotCurrent` | the variant |
| an unrecognised, newer or damaged schema | `IncompatibleSchema` | the variant |
| an unsafe journal mode for a migration, SQLite not honouring a setting, a failed migration postcondition, a failed runtime configuration | `DatabaseState` | the variant |
| a failure of the file-system watcher | `Watcher` | the variant |

The same release also **rejects four inputs** that were accepted before, all
announced in v0.21.4 and v0.21.5; they are new errors, not renamed ones:

| Input | Error | What to do |
|---|---|---|
| `max_entries` of `Some(0)`, at open | `InvalidConfiguration { option: "max_entries", .. }` | use `None` for no limit |
| a TTL under one second (including zero), at open | `InvalidConfiguration { option: "ttl", .. }` | use a TTL of at least one second, or no TTL |
| a `batch_set` with more distinct paths than `max_entries` | `InvalidArgument { argument: "items", .. }` | split the batch, or raise `max_entries` |
| an `import_entries` or `import_from` of more distinct entries than `max_entries` | `InvalidArgument { argument: "records", .. }` | split the import, or raise `max_entries` |

All four are found before anything is written. The first two are checked at
open, where a read-only engine is not exempt.

If you matched on the **text** of an `unsupported feature: …` message, match on
the variant instead; the text of schema, state and watcher errors was reworded so
that each message reads once (`incompatible database schema: unrecognized at
physical version 7: …`, `database state: migration requires …`, `file watching
failed: could not watch '…': …`). The clauses `database was not modified` and
`transaction will be rolled back` are kept verbatim. The CLI exits with the same
status as before; only the stderr prefix changed (`unsupported feature:` became
`serialization error:` or `invalid argument: records:`).

## Database errors

`LocalFileCacheError::Database` carries a `DatabaseError`. It is opaque: you
cannot construct one, and no database library type appears in its API.

- **`Display`** prints SQLite's own message, so `database error: …` reads as it
  always did.
- **`std::error::Error::source()`** returns the underlying error, type-erased.
- **`sqlite_extended_code() -> Option<i32>`** returns the SQLite *extended*
  result code when SQLite reported one, and `None` otherwise (for example a
  query that returned no rows). The *primary* result code is `code & 0xff`.

To recognise a busy database without naming any database library, compare the
primary code with `SQLITE_BUSY`, which is 5:

```rust
use localcache::LocalFileCacheError;

const SQLITE_BUSY: i32 = 5;

match engine.set("file.txt", &payload) {
    Err(LocalFileCacheError::Database(error))
        if error.sqlite_extended_code().map(|code| code & 0xff) == Some(SQLITE_BUSY) =>
    {
        // Another connection holds the write lock and the busy timeout
        // (about five seconds) ran out: retry later.
    }
    other => { other?; }
}
```

The codes are listed in SQLite's documentation, "Result and Error Codes".

## Common patterns

### Read-only open

Read-only mode accepts only an existing database with the exact current
schema. It does not create an empty database or migrate a historical one.
Recognized historical or empty schemas return `SchemaNotCurrent`; future or
malformed schemas return `IncompatibleSchema`. Reopen writable
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

Malformed or resource-excessive glob patterns return
`InvalidArgument { argument: "path_glob", .. }` with a stable non-echoing
reason. Scan and query terminals validate these patterns
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

No error reports a payload-version mismatch (a variant for it existed until
v0.22.0 and was never returned). When `payload_version`
is bumped, an entry whose stored version no longer matches is treated exactly
like a missing or stale entry: `get_if_fresh` returns `None`, and
`check_status` returns `CacheStatus::Stale`. Purge outdated entries all at once
with `purge_stale_versions()`, which finds them the same way — by comparing the
stored version:

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

Two related, but distinct, failures return different variants: decoding an encrypted entry
without the `encryption` Cargo feature compiled in returns `UnknownEncoding`, and decoding one
with the feature compiled in but no encryption key configured on the engine returns
`MissingEncryptionKey`. `EncryptionError` itself only covers an actual decrypt failure — the wrong
key, or corrupted stored data.

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
