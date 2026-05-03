# Architecture

## Storage

`localcache` uses a single **SQLite** file (via the `rusqlite` crate with
bundled SQLite).  No daemon, no network, no external process.

### Schema (v4)

```sql
CREATE TABLE files (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    namespace         TEXT    NOT NULL DEFAULT 'default',
    path              TEXT    NOT NULL,
    mtime             INTEGER NOT NULL,
    file_size         INTEGER NOT NULL,
    hash              TEXT,                    -- BLAKE3 hash (optional)
    updated_at        INTEGER NOT NULL,        -- Unix seconds of last write
    payload_version   INTEGER NOT NULL DEFAULT 0,
    last_accessed_at  INTEGER NOT NULL DEFAULT 0,  -- Unix seconds of last read
    UNIQUE(namespace, path)
);

CREATE TABLE payloads (
    file_id  INTEGER PRIMARY KEY,
    content  BLOB    NOT NULL,
    encoding TEXT    NOT NULL DEFAULT 'raw',   -- codec/compression/encryption tag
    FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
);
```

### Encoding tags

The `encoding` column describes the full transformation pipeline applied
to the payload:

```
raw                  — bincode only
zstd                 — bincode + zstd
json                 — serde_json only
json-zstd            — serde_json + zstd
raw-aes256gcm        — bincode + AES-256-GCM
zstd-aes256gcm       — bincode + zstd + AES-256-GCM
json-aes256gcm       — serde_json + AES-256-GCM
json-zstd-aes256gcm  — serde_json + zstd + AES-256-GCM
```

Encoding is decoded from the tag at read time, so different entries in the
same namespace can have different encodings.

## Write path

```
engine.set(path, payload)
  │
  ├── normalize_path(path)          → canonical PathBuf
  ├── collect_metadata(path)        → mtime, file_size
  ├── compute_hash_for_mode(path)   → Option<BLAKE3 hash>
  ├── encode_payload(payload)       → (bytes, encoding_tag)
  │     codec (bincode / json)
  │     compress? (zstd)
  │     encrypt? (AES-256-GCM + nonce)
  ├── repository::upsert()          → INSERT OR REPLACE INTO files + payloads
  └── enforce_max_entries()         → delete_lru_n() + on_evict callback
```

## Read path

```
engine.get_if_fresh(path)
  │
  ├── normalize_path(path)
  ├── repository::find_file()       → FileRow (mtime, hash, …)
  ├── is_expired(updated_at, ttl)   → bool
  ├── version check                 → payload_version match?
  ├── detect_change(path, metadata) → CacheStatus
  ├── repository::load_payload()    → (content, encoding)
  ├── decode_payload(content)       → T
  │     decrypt? (AES-256-GCM)
  │     decompress? (zstd)
  │     deserialise (bincode / json)
  └── touch_last_accessed()         → UPDATE last_accessed_at
```

## LRU eviction

`last_accessed_at` is updated on every successful `get` or `get_if_fresh`.
When `max_entries` is set, `enforce_max_entries` after each `set` runs:

```sql
DELETE FROM files
WHERE namespace = ?
  AND id IN (
    SELECT id FROM files WHERE namespace = ?
    ORDER BY last_accessed_at ASC, updated_at ASC
    LIMIT ?
  )
```

Entries with `last_accessed_at = 0` (never read) are evicted first.

## SQLite settings

| PRAGMA | Default | Purpose |
|---|---|---|
| `journal_mode` | `WAL` | Concurrent reads during writes |
| `synchronous` | `NORMAL` | Balanced durability vs speed |
| `foreign_keys` | `ON` | Cascade deletes payloads with files |

## Payload encoding pipeline

```
User payload (T: Serialize)
  ↓  codec   (bincode or json)
  ↓  compress (zstd, optional)
  ↓  encrypt  (AES-256-GCM, optional)
→ BLOB stored in payloads.content
```

The inverse is applied on read.  Encoding parameters are not stored
per-entry — they are determined by the engine configuration.  The
`encoding` tag verifies that the correct features are available.
