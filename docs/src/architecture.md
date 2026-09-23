# Architecture

## Storage

`localcache` uses a single **SQLite** file (via the `rusqlite` crate with
bundled SQLite).  No daemon, no network, no external process.

### Schema (v5)

```sql
CREATE TABLE files (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    namespace         TEXT    NOT NULL DEFAULT 'default',
    path              TEXT    NOT NULL,
    mtime             INTEGER NOT NULL,        -- nanoseconds since the Unix epoch
    file_size         INTEGER NOT NULL,
    hash              TEXT,                    -- BLAKE3 hash (optional)
    updated_at        INTEGER NOT NULL,        -- Unix seconds of last write
    payload_version   INTEGER NOT NULL DEFAULT 0,
    last_accessed_at  INTEGER NOT NULL DEFAULT 0,  -- Unix seconds of last read; 0 = never read
    UNIQUE(namespace, path)
);

CREATE TABLE payloads (
    file_id  INTEGER PRIMARY KEY,
    content  BLOB    NOT NULL,
    encoding TEXT    NOT NULL DEFAULT 'raw',   -- codec/compression/encryption tag
    FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
);

CREATE INDEX idx_files_namespace_path ON files(namespace, path);
CREATE INDEX idx_files_lru ON files(namespace, last_accessed_at, updated_at);
```

`mtime` has been nanosecond-resolution since schema v5, so two writes to the same file within the
same second are still detected as distinct. `idx_files_namespace_path` backs path lookups and
`path_like`/`path_glob`/`path_in_dir` queries; `idx_files_lru` backs the eviction scan below.

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
  │
  ├── BEGIN IMMEDIATE
  │     ├── repository::upsert_in_tx()   → INSERT ... ON CONFLICT(namespace, path) DO UPDATE
  │     └── enforce_max_entries()        → evict_lru(), excluding the row(s) just written
  ├── COMMIT
  └── on_evict callback, once per evicted path
```

The write and its eviction share **one** `IMMEDIATE` transaction: `Ok` means the entry was stored
and the bound enforced; `Err` means nothing changed. A concurrent writer waits under the busy
timeout rather than racing a deferred read-then-write.

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

## Eviction

`last_accessed_at` reflects the last **read** — it is set on every successful `get`,
`get_if_fresh`, and `touch`, and left untouched by a write to an existing entry. A brand-new row starts at
`last_accessed_at = 0` ("never read"). When `max_entries` is set, `enforce_max_entries` after each
`set`/`batch_set` selects eviction candidates in this order, oldest first:

1. `last_accessed_at` ascending — never-read entries (`0`) sort first.
2. `updated_at` ascending, as a tiebreak.
3. `id` ascending, as a final, fully deterministic tiebreak.

**A write never evicts what it just wrote.** The row(s) a `set`/`batch_set` call just inserted or
updated are excluded from that call's own eviction candidates, even though a brand-new row's
`last_accessed_at = 0` would otherwise make it the first entry eligible for eviction. An oversized
`batch_set` therefore keeps everything it reports as stored; the bound is restored by the *next*
write, not necessarily within the same call. `max_entries(0)` keeps only the most recently written
entry. The bound is not enforced by `import_entries`/`import_from`.

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

The inverse is applied on read, driven entirely by the stored `encoding` tag: which codec,
whether to decompress, and whether to decrypt are all read from the tag, not from the engine's
current configuration. Configuration supplies only the encryption key (when needed) — this is
what lets different entries in the same namespace carry different encodings and still decode
correctly.

This is accurate about *configuration*, but which tags can be decoded **at all** is fixed by which
Cargo features this build was compiled with: `zstd`/`json-zstd` need `compression`, `json*` needs
`json`, and `*-aes256gcm` needs `encryption`. A tag this build cannot handle returns
`UnknownEncoding` (`crates/localcache/src/serialization.rs`, `decode_payload`).
