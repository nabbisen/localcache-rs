# RFC 0004 — Read-only Shared-memory DB Mode

| Field    | Value |
|----------|-------|
| Status   | Implemented (v0.17.0) |
| Feature  | *(core, no feature flag)* |
| Touches  | `src/cache/engine.rs`, `src/cache/options.rs`, `src/cache/builder.rs` |

## Summary

Support opening the same SQLite database file simultaneously from multiple
processes in **read-only, shared-cache** mode using the `file:path?mode=ro`
URI connection string.  This allows read-heavy workloads (e.g. a fleet of
worker processes consuming a cache written by a single producer) to share
the same SQLite WAL file without locking contention.

## Motivation

The current `read_only` option passes `Connection::open_with_flags(…,
OpenFlags::SQLITE_OPEN_READONLY)`.  This opens an independent OS-level
file handle per process.  On Linux, SQLite's WAL mode allows one writer
and multiple readers concurrently across processes, but each reader holds
its own WAL read-lock.  With many reader processes this causes unnecessary
WAL accumulation.

SQLite's **shared-cache mode** (`file:path?mode=ro&cache=shared`) allows
multiple connections within the **same process** to share the page cache,
reducing memory usage and locking overhead.  Combined with `PRAGMA
query_only = ON`, it also prevents accidental writes through a read-only
handle obtained via a URI connection.

This RFC is scoped to **within-process shared-cache** (the most common
need).  Cross-process shared memory via `shm` would require the `:memory:`
URI and named shared-cache, which is a more complex undertaking.

## Requirements

- New `CacheEngineBuilder` option: `.shared_cache(true)`.
- When enabled, open the database using a `file:` URI with
  `mode=ro&cache=shared`.
- `PRAGMA query_only = ON` enforced on the connection after open.
- Write methods (`set`, `remove`, `cleanup_*`, etc.) return
  `Err(LocalFileCacheError::ReadOnly)` — same as the existing `read_only`
  flag.
- The feature must be composable with all other read-path options
  (namespace, TTL, codec, encryption).
- Must not change behaviour when `shared_cache(false)` (default).

## Design

### `CacheOptions` changes

```rust
pub struct CacheOptions {
    // … existing fields …

    /// Open the database in read-only shared-cache mode using a `file:` URI.
    ///
    /// Multiple `CacheEngine` instances opened with this option on the same
    /// `database_path` within the same process will share the SQLite page
    /// cache, reducing memory usage and read-lock overhead.
    ///
    /// Implies `read_only = true`.  Write methods return
    /// `LocalFileCacheError::ReadOnly`.
    pub shared_cache: bool,
}
```

### `CacheEngineBuilder` changes

```rust
impl<T> CacheEngineBuilder<T> {
    /// Open in read-only shared-cache mode (see `CacheOptions::shared_cache`).
    pub fn shared_cache(mut self) -> Self {
        self.options.shared_cache = true;
        self.options.read_only = true;
        self
    }
}
```

### Connection opening — `engine.rs`

```rust
fn open_connection(options: &CacheOptions) -> Result<Connection, LocalFileCacheError> {
    if options.shared_cache {
        // Encode path for URI: percent-encode special characters.
        let path_str = options.database_path
            .to_str()
            .ok_or_else(|| LocalFileCacheError::InvalidPath {
                path: options.database_path.display().to_string(),
            })?;
        // SQLite URI: file:PATH?mode=ro&cache=shared
        let uri = format!(
            "file:{}?mode=ro&cache=shared",
            percent_encode(path_str)
        );
        let conn = Connection::open_with_flags(
            uri,
            OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_READONLY
            | OpenFlags::SQLITE_OPEN_SHARED_CACHE,
        )?;
        conn.execute_batch("PRAGMA query_only = ON;")?;
        Ok(conn)
    } else if options.read_only {
        // Existing path
        Connection::open_with_flags(
            &options.database_path,
            OpenFlags::SQLITE_OPEN_READONLY,
        ).map_err(Into::into)
    } else {
        // Existing path (read-write)
        Connection::open(&options.database_path).map_err(Into::into)
    }
}
```

`percent_encode` is a small inline helper that escapes `#`, `?`, `%`, and
spaces in the path; it does not require an external dependency.

### `guard_write` unchanged

`guard_write()` already checks `self.read_only`.  Because
`shared_cache(true)` implies `read_only = true`, all write guards fire
correctly with no additional changes.

### In-memory database interaction

`:memory:` with `shared_cache = true` opens as
`file::memory:?mode=memory&cache=shared`, which is a valid SQLite URI for
an in-process named shared-memory database.  This is a useful pattern for
testing; it should be explicitly supported and documented.

```rust
// Two engines sharing the same in-memory DB:
let writer = CacheEngine::<Vec<f32>>::builder()
    .database(":memory:")
    .build()?;

let reader = CacheEngine::<Vec<f32>>::builder()
    .database(":memory:")
    .shared_cache()
    .build()?;
```

> **Note**: for `:memory:`, the current implementation uses a single
> in-process connection.  The shared-cache URI `file::memory:?cache=shared`
> opens a *named* in-memory database distinct from the unnamed `:memory:`.
> The builder must detect `:memory:` and map it to the URI form when
> `shared_cache = true`.

## Test plan

- `shared_cache()` opens the database successfully for a non-empty file.
- Read operations (`get`, `get_if_fresh`, `keys`) work correctly.
- Write operations (`set`, `remove`) return `Err(ReadOnly)`.
- Two `CacheEngine` instances with `shared_cache()` on the same file
  within one process see each other's data (shared page cache).
- One writer + one shared-cache reader: reader sees committed data.
- `:memory:` with `shared_cache()`: two engines share the same in-memory DB.
- `shared_cache(false)` (default): behaviour is identical to current.

## Security considerations

`PRAGMA query_only = ON` prevents write statements from executing on the
connection even if `guard_write` were to malfunction.  This provides
defence in depth for read-only shared connections.

The `file:` URI may expose the database path in SQLite error messages.
This is not a new concern — `Connection::open` already includes the path
in its error messages.

## Implementation notes (v0.17.0)

### `:memory:` + `shared_cache` — read-write, not read-only

The RFC example shows a plain `:memory:` writer alongside a
`shared_cache()` reader, but `shared_cache()` as specified implies
`read_only = true`.  A read-only fresh in-memory database would be
permanently empty and therefore useless.

**Resolution:** when `shared_cache` is combined with `:memory:`, the
engine opens `file::memory:?cache=shared` in **read-write** mode and
does **not** force `read_only`.  All other `shared_cache` semantics
(page-cache sharing, `PRAGMA query_only = ON`) remain.  Both engines
must use `.shared_cache()` to share the named in-memory database.

The `CacheEngineBuilder::shared_cache()` builder method therefore does
not set `opts.read_only`; `CacheEngine::open()` computes the effective
read-only flag at connection time based on whether the path is `:memory:`.
