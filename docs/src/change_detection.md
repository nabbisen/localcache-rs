# Change Detection

`localcache` supports four change-detection strategies, selected via
`CacheOptions::change_detection_mode`.

## MetadataOnly

Compares `mtime` (modification time) and `file_size` stored in the database
against the current values from the filesystem.

- **Fast** — two integer comparisons, no I/O beyond `stat`.
- **Limitation** — can be fooled by a same-size overwrite that leaves mtime
  unchanged (uncommon in practice but possible with certain copy tools).

Use this mode when throughput matters and your filesystem's mtime precision
is reliable.

## MetadataThenFullHash

1. Check `mtime` and `file_size` first.
2. If they match → **Fresh** (no hash needed).
3. If they differ → compute a full BLAKE3 hash and compare with the stored
   value.
4. Hash matches → **Fresh**; hash differs → **Stale**.

This is the recommended general-purpose mode.  Most of the time the metadata
check is sufficient; the hash is only computed when metadata has changed.

## StrictFullHash

Always computes a full BLAKE3 hash and compares it with the stored value,
ignoring mtime and file_size entirely.

Use this when you need content-addressed semantics, e.g. files may be
regenerated with the same content but a new mtime.

## MetadataThenPartialHash

Defined for future use.  In v0.1 it falls back to `MetadataThenFullHash`.
A future release will implement head+tail sampling to reduce I/O for large
files.

## Hash computation

Hashes are computed with [BLAKE3](https://github.com/BLAKE3-team/BLAKE3) using
a 64 KiB streaming reader, so very large files never need to be fully loaded
into memory.  The digest is stored as a lowercase hex string.

## What is stored

| Mode | `hash` column |
|------|--------------|
| `MetadataOnly` | `NULL` |
| `MetadataThenPartialHash` | full BLAKE3 hex |
| `MetadataThenFullHash` | full BLAKE3 hex |
| `StrictFullHash` | full BLAKE3 hex |

If an entry was stored with `MetadataOnly` and you later switch to a
hash-based mode, the entry will be treated as **Stale** (no stored hash to
compare) until it is re-set.
