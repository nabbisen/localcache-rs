# Architecture

## Module layout

```
src/
  lib.rs             — public API surface (re-exports only)
  error.rs           — LocalFileCacheError
  path.rs            — normalize_path()
  serialization.rs   — serialize_payload() / deserialize_payload()

  cache/
    engine.rs        — CacheEngine<T> implementation
    entry.rs         — CacheEntry<T>, CacheStatus, FileMetadata
    options.rs       — CacheOptions, ChangeDetectionMode

  db/
    schema.rs        — DDL: CREATE TABLE / INDEX / PRAGMA
    repository.rs    — all SQL queries (find, upsert, delete, list)

  detection/
    hash.rs          — compute_full_hash() via BLAKE3
    metadata.rs      — collect_metadata() from std::fs
    strategy.rs      — detect_change() — per-mode dispatch

  tests.rs           — integration tests
```

## Layering

```
CacheEngine<T>  (cache/engine.rs)
    │
    ├── path normalization      (path.rs)
    ├── metadata collection     (detection/metadata.rs)
    ├── hash computation        (detection/hash.rs)
    ├── change detection        (detection/strategy.rs)
    ├── serialisation           (serialization.rs)
    └── DB operations           (db/repository.rs)
                                    └── schema init  (db/schema.rs)
```

Each layer has one responsibility.  `CacheEngine` orchestrates; it does not
contain SQL or hash logic directly.

## SQLite schema

```sql
CREATE TABLE files (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    path       TEXT    NOT NULL UNIQUE,
    mtime      INTEGER NOT NULL,
    file_size  INTEGER NOT NULL,
    hash       TEXT,
    updated_at INTEGER NOT NULL
);

CREATE TABLE payloads (
    file_id INTEGER PRIMARY KEY,
    content BLOB NOT NULL,
    FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
);

CREATE INDEX idx_files_path ON files(path);
```

Payloads are in a separate table so that change-detection queries
(`check_status`, `get_if_fresh`) can read file metadata without loading
potentially large BLOB data.

## Transaction guarantees

`set` runs inside a single SQLite transaction:

1. Upsert `files` row.
2. Retrieve the `id`.
3. Upsert `payloads` row.
4. Commit.

If any step fails the transaction is rolled back automatically, leaving the
database in a consistent state.
