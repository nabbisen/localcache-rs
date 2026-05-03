# CLI Tool

The `localcache-cli` crate ships a `localcache` binary for inspecting and
maintaining cache databases without writing any Rust code.

## Installation

```sh
cargo install localcache-cli

# With file-watching support:
cargo install localcache-cli --features watching
```

## Global options

```
localcache [OPTIONS] <COMMAND>

Options:
  -d, --database <PATH>   SQLite database file [default: localcache.sqlite3]
  -n, --namespace <NS>    Namespace to operate on [default: default]
  -h, --help              Print help
  -V, --version           Print version
```

## Commands

### `list`

List all cached entries with metadata.

```sh
localcache -d cache.sqlite3 list
localcache -d cache.sqlite3 list --limit 50
```

Output columns: `PATH`, `VERSION`, `ENCODING`, `UPDATED_AT`, `LAST_ACCESS`

### `stats`

Show aggregate statistics for the namespace.

```sh
localcache -d cache.sqlite3 stats
localcache -d cache.sqlite3 -n embeddings stats
```

Output includes entry count, total payload bytes, encoding breakdown,
and version distribution.

### `inspect <PATH>`

Detailed staleness diagnosis for a single file — shows *why* an entry is
fresh, stale, or missing.

```sh
localcache -d cache.sqlite3 inspect /data/corpus/doc_001.txt
```

Output example:

```
=== Cache Diagnosis ===
Path:          /data/corpus/doc_001.txt
Status:        Stale
Entry exists:  true
File exists:   true
TTL:           not configured
--- Metadata ---
  mtime:     2025-01-01 00:00:00  current=2026-05-03 10:22:11  changed=true
  file_size: 4.0 KiB              current=4.1 KiB              changed=true

Summary: Both mtime and file_size differ.
```

### `check <PATH>`

Quick freshness check — prints `FRESH`, `STALE`, or `MISSING`.

```sh
localcache -d cache.sqlite3 check /data/file.txt
```

### `scan <DIR>`

Scan a directory and show the cache status of each file.

```sh
localcache -d cache.sqlite3 scan ./data --recursive
localcache -d cache.sqlite3 scan ./data --extensions txt,md
localcache -d cache.sqlite3 scan ./data --glob "report_*.pdf"
```

### `query`

List cached entries matching a SQL `LIKE` path pattern
(`%` = any sequence, `_` = one character).

```sh
localcache -d cache.sqlite3 query --path-like "%/docs/%"
```

### `namespaces`

List all namespaces in the database with their entry counts.

```sh
localcache -d cache.sqlite3 namespaces
```

### `cleanup`

Remove entries whose source files no longer exist on disk.

```sh
localcache -d cache.sqlite3 cleanup
```

### `vacuum`

Run SQLite `VACUUM` to compact the database file after deletions.

```sh
localcache -d cache.sqlite3 vacuum
```

### `purge-version <VERSION>`

Delete all entries whose `payload_version` differs from `VERSION`.

```sh
localcache -d cache.sqlite3 purge-version 3
```

### `export`

Export all entries to JSON Lines format (stdout or file).

```sh
# To stdout.
localcache -d cache.sqlite3 export

# To file.
localcache -d cache.sqlite3 export -o backup.jsonl
```

### `import`

Import entries from JSON Lines format (stdin or file).

```sh
localcache -d new.sqlite3 import < backup.jsonl
localcache -d new.sqlite3 import -i backup.jsonl
```

### `copy`

Copy all entries from one namespace to another within the same database.

```sh
localcache -d cache.sqlite3 copy --from embeddings --to embeddings_v2
```

### `migrate`

Move a namespace from one database to another.

```sh
localcache migrate \
  --src-db old.sqlite3 --src-ns default \
  --dst-db new.sqlite3 --dst-ns default
```

### `watch` *(requires `watching` feature)*

Watch cached files for changes and print invalidation events in real time.
Press Ctrl-C to stop.

```sh
localcache -d cache.sqlite3 watch
```

Output: `[YYYY-MM-DD HH:MM:SS] MODIFIED  /path/to/changed/file.txt`
