# Architecture

## Storage

`localcache` uses a single **SQLite** file (via the `rusqlite` crate with
bundled SQLite).  No daemon, no network, no external process.

### Schema (v6)

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
    last_used_us      INTEGER NOT NULL DEFAULT 0   -- the use order, see Eviction
        CHECK (typeof(last_used_us) = 'integer' AND last_used_us < 4611686018427387904),
    UNIQUE(namespace, path)
);

CREATE TABLE payloads (
    file_id  INTEGER PRIMARY KEY,
    content  BLOB    NOT NULL,
    encoding TEXT    NOT NULL DEFAULT 'raw',   -- codec/compression/encryption tag
    FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
);

CREATE INDEX idx_files_use ON files(namespace, last_used_us);
```

`mtime` has been nanosecond-resolution since schema v5, so two writes to the same file within the
same second are still detected as distinct. Path lookups and the `path_like`/`path_glob`/`path_in_dir`
queries are served by the index SQLite builds for `UNIQUE(namespace, path)` (`sqlite_autoindex_files_1`,
the one the plans in [Performance](./performance.md) name); `idx_files_use` backs the eviction scan
below and the lookup of a namespace's newest use. Before v0.23.0 (schema v5) a second index on
`(namespace, path)` duplicated that autoindex, and `last_accessed_at` and `idx_files_lru` took the place
of `last_used_us` and `idx_files_use`; [Migration](./migration.md) describes the move.

### Encoding tags

The `encoding` column describes the full transformation pipeline applied
to the payload:

```
raw                  — bincode only
zstd                 — bincode + zstd
json                 — serde_json only
json-zstd            — serde_json + zstd
raw-aes256gcm-bound        — bincode + AES-256-GCM, bound to the entry's path
zstd-aes256gcm-bound       — bincode + zstd + AES-256-GCM, bound to the entry's path
json-aes256gcm-bound       — serde_json + AES-256-GCM, bound to the entry's path
json-zstd-aes256gcm-bound  — serde_json + zstd + AES-256-GCM, bound to the entry's path

raw-aes256gcm        — the same four, sealed WITHOUT binding (written by v0.23 and earlier)
zstd-aes256gcm       — ...
json-aes256gcm       — ...
json-zstd-aes256gcm  — ...
```

A **bound** payload is sealed with associated data
`"localcache/seal/1" 0x00 <the full tag> 0x00 <the entry's path>`, so its
ciphertext authenticates which entry it belongs to and how it is to be decoded:
moved to another path, or read under another tag, it fails to open. The path is
the string `files.path` stores; the associated data is not stored. The tag is
matched exactly and case-sensitively. An engine writes bound tags when it has
a key. With a key it reads **only** bound entries: a plain tag or a tag without
`-bound` is refused (`UnsealedEntry`) until `rotate_encryption_key` has sealed
it; without a key it reads plain entries and names a missing key for sealed
ones. See [Security Model](./security.md#encryption).

Encoding is decoded from the tag at read time, so different entries in the
same namespace can have different encodings.

## Write path

```
engine.set(path, payload)
  │
  ├── normalize_path(path)          → canonical PathBuf
  ├── collect_metadata(path)        → mtime, file_size
  ├── compute_hash_for_mode(path)   → Option<BLAKE3 hash>
  ├── PayloadFormat::encode(payload, path, key) → (bytes, encoding_tag)
  │     codec (bincode / json)
  │     compress? (zstd; refused above max_decompressed_bytes)
  │     encrypt? (AES-256-GCM + nonce, bound to the path)
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
  ├── PayloadFormat::decode(content, tag, path, key) → T
  │     decrypt? (AES-256-GCM; with a key only bound entries, opened with the path)
  │     decompress? (zstd; bounded by max_decompressed_bytes)
  │     deserialise (bincode / json)
  └── record_use()                  → UPDATE last_used_us (a read must not fail for want of this)
```

## Eviction

`max_entries` evicts the entry whose last **use** is oldest. A use is a read hit (`get`,
`get_if_fresh`), a `touch`, a `set` or `batch_set`, or an overwrite of an existing entry. The order is
strict: a use assigns the entry

```
last_used_us = max(now_us, <largest last_used_us of its namespace> + 1)
```

in the statement that writes it, so it is greater than every other value in the namespace whatever the
resolution of the clock, and a clock that never moves, or steps backwards, cannot produce a tie or a
reversal. SQLite lets one writer at a time through, so the statement is atomic across connections and
processes. After each `set`/`batch_set`/import, `enforce_max_entries` evicts, oldest first, by
`last_used_us` ascending and then `id` ascending (which decides only the ties an import can create:
it restores seconds, not microseconds).

`last_accessed_at`, the public field of `EntryInfo` and `ExportRecord`, is `last_used_us / 1_000_000`:
the second of the last use, never 0 for a stored entry. The eviction order is finer than that field.
An entry that is only read in one second is not ranked above one written in it; before v0.23.0 only
a read counted, and an entry read once, ever, outlived every entry that was only written.

An **import is a restore, not a use**: each entry keeps its exported recency (`last_accessed_at` seconds,
or `updated_at` for a record from before v0.23.0 that was never read), a record whose recency is
negative or too large for the order refuses the whole import before anything is written, and every
later use ranks above every imported entry. `export_entries` lists the entries oldest use first, so an
export imported in order keeps the relative recency of entries used in the same second.

A use that would take the value to 2^62 (only a damaged or hostile database holds one near it) fails
with `DatabaseState`, the column's `CHECK` stopping it: a read ignores that, `touch`, `set` and
`batch_set` return it.

**A write never evicts what it just wrote.** The row(s) a `set`/`batch_set`/`import_entries`/
`import_from` call just inserted or updated are excluded from that call's own eviction candidates.
A `batch_set` with more distinct paths than `max_entries`, and an import of more distinct entries, is
rejected before it writes anything, so one call can no longer exceed the bound; `max_entries(0)` is
rejected at open. Every write path shares one `enforce_max_entries` and runs it in the write's own
`IMMEDIATE` transaction. (Before v0.22.0 the imports ignored the bound.)

## SQLite settings

| PRAGMA | Default | Purpose |
|---|---|---|
| `journal_mode` | `Preserve`: `WAL` for a new database, unchanged for an existing one | Concurrent reads during writes, without converting a database you did not ask to convert |
| `synchronous` | `NORMAL` | Balanced durability vs speed |
| `foreign_keys` | `ON` | Cascade deletes payloads with files |

## Payload encoding pipeline

```
User payload (T: Serialize)
  ↓  codec   (bincode or json)
  ↓  compress (zstd, optional)
  ↓  encrypt  (AES-256-GCM, bound to the entry's path, optional)
→ BLOB stored in payloads.content
```

The `bincode` codec (`Codec::Bincode`, the tags `raw` and `zstd`) writes and reads the legacy
`bincode`-1 wire format of RFC 008: little-endian, fixed-width integers, `u64` length prefixes and
`u32` variant indices. The codec is the project's own (`payload_codec` in `localcache-format`, RFC 028); the
name is the format's, and `bincode` is not a dependency. Its decoder is written for hostile input:
every length and count is checked against the bytes that remain before anything is allocated, and
nesting is limited to 128 levels. See [Security Model](./security.md).

Reading runs the steps in reverse. The decompress step is **bounded**: a compressed payload
may not expand past `CacheOptions::max_decompressed_bytes` (default 64 MiB), on read or on
write, and is refused with `PayloadTooLarge` otherwise; uncompressed payloads are not limited.
See [Features](./features.md#the-decompression-limit).

The inverse is applied on read, driven entirely by the stored `encoding` tag: which codec,
whether to decompress, and whether to decrypt are all read from the tag, not from the engine's
current configuration. Configuration supplies only the encryption key (when needed) — this is
what lets different entries in the same namespace carry different encodings and still decode
correctly. One rule is configuration-dependent on purpose: with a key, a payload that is not
sealed bound to its path is refused instead of decoded (`UnsealedEntry`).

This is accurate about *configuration*, but which tags can be decoded **at all** is fixed by which
Cargo features this build was compiled with: `zstd`/`json-zstd` need `compression`, `json*` needs
`json`, and `*-aes256gcm` and `*-aes256gcm-bound` need `encryption`. A tag this build cannot handle returns
`UnknownEncoding` (`crates/localcache-format/src/lib.rs`, `PayloadFormat::decode`).

## Crates and layers

Three crates are published from this workspace, always at one version:

| Crate | Holds | Depend on it? |
|---|---|---|
| `localcache-format` | the payload format: the codec, compression, JSON, the table of encoding tags, sealing and opening, `Key`, `Codec`, `FormatError` | No: an implementation crate. Its API is not a separate stability promise |
| `localcache` | everything else: the database layer, freshness, the engine, the query layer, the pools, the watchers, the public error | **Yes**: this is the library |
| `localcache-cli` | the command-line tool | to install, not to depend on |

`localcache` depends on `localcache-format` at the exact same version (`=X.Y.Z`; `scripts/check_versions.py` fails if the pin and the workspace version differ), forwards its `compression`, `json` and `encryption` features to it, and re-exports `Codec` and `DEFAULT_MAX_DECOMPRESSED_BYTES` under the paths they always had. **The compiler enforces the boundary between them**: `localcache-format` names nothing of `localcache`, has no SQL, no async and no file system, and `localcache` reaches it through its public API only (`PayloadFormat`, `seal_state`, `reseal`, `Key`). That is also what makes the format crate checkable on its own: its golden vectors and their digests, a Miri run and two fuzz targets live in `crates/localcache-format` (see its README).

Inside `localcache` the code is built in layers, lowest first. A module may name only the layers listed for it; `tests/layering.rs` reads the source and fails, with the file and the line, when a lower layer names a higher one. This is what the compiler cannot see inside one crate.

| Layer | Holds | May use |
|---|---|---|
| `db` | the schema, its migrations and the classifier, every statement (repository, indexes), the values it stores (`EntryInfo`, `FileMetadata`, `JournalMode`, `SynchronousMode`), its error type `StorageError` (with `DatabaseError`), and the `Store`, the one handle to the database | nothing of the crate |
| `error` | the public error enum, and the crate-private lift of `StorageError` and `FormatError` into it | `db` |
| `path` | canonical path keys | `error` |
| `detection` | freshness (`CacheStatus`, `ChangeDetectionMode`, hashing) | `error`, `db` (it compares a stored `FileMetadata`) |
| `cache` | the engine, the query layer, options, the builder, globbing | everything above |
| adapters | `SyncCacheEngine`, `ReadPool`, `AsyncCacheEngine`, the watchers | everything above |

The engine, the query layer, the pools and the watchers reach the database only through `db::Store`: it opens the connection (flags, shared-cache URIs, pragmas, the schema check or migration), starts the `IMMEDIATE` write transaction, and has one method per statement the layers above need. Outside `db` no production file names `rusqlite`, and none names `db::repository`, `db::schema` or `db::indexes`; `tests/layering.rs` checks both. It is a test that reads source text, weaker than the compiler: it also rejects the relative spellings (`super::super`, a `super::` that reaches the crate root, `localcache::`, `use crate::*`) in the lower layers, and its header lists what it still cannot see (macro-built paths, `#[path]`, items reached through another module's glob). Test files may still use `rusqlite` to set up and inspect rows through `Store::raw`, which exists only in test builds.

The public types that live in a lower layer or in `localcache-format` (`EntryInfo`, `Codec`, …) are re-exported from the crate root under the same paths as before. The database layer and the format crate report their own errors (`StorageError`, `FormatError`), and `error` turns them into the public `LocalFileCacheError` through a crate-private trait (`Lift`, the `.lift()` call): there is deliberately no `From` impl on the public enum, because a trait impl would be public API and rustdoc would list the internal type. The public variants, payloads and texts are pinned by `error/golden.rs`. Three public methods of the engine and the builder return an adapter type (`CacheEngineBuilder::build_read_pool`, `CacheEngine::watcher`, `CacheEngine::debounced_watcher`); the test names them as its only exceptions.
