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

Every writable command (`cleanup`, `vacuum`, `purge-version`, `import`, `copy`, `migrate`,
`watch`) opens its database with SQLite's `WAL` journal mode and `synchronous = NORMAL` — the
library's defaults. WAL persists in the database file as a `-wal` file until checkpointed; this
is documented current behaviour, not something the CLI changes for you.

Commands that colour their status labels (`scan` and `query`) do so only when
stdout is a terminal, and never when the `NO_COLOR` environment variable is set
to a non-empty value (`NO_COLOR=` with an empty value does not disable colour,
per <https://no-color.org>). Piped output is never coloured. Colour is
currently unix-only; Windows consoles get plain text.

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
Payload ver:   stored=2 expected=2 match=true
--- Metadata ---
  mtime:     stored=2025-01-01 00:00:00.123456789 current=2026-05-03 10:22:11.987654321 changed=true
  file_size: stored=4.0 KiB current=4.1 KiB changed=true
Hash match:    false

Summary: Both mtime and file_size differ.
```

`Payload ver:` prints only when the entry has a stored payload version; `Hash match:` prints only
when a hash comparison was made. `check` and `inspect` both always run change detection under
`MetadataThenFullHash`, regardless of the mode the entry was originally cached under.

### `check <PATH>`

Quick freshness check — prints `FRESH`, `STALE`, or `MISSING`. Always runs change detection under
`MetadataThenFullHash`, regardless of the mode the entry was originally cached under.

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

Glob matching is case-sensitive on every platform. `*` and `?` operate on
Unicode scalar values, and nested/multiple `{a,b}` alternatives are supported.
Malformed or over-limit patterns produce an error without starting the scan.

`list` truncates long paths at Unicode scalar boundaries. `inspect` renders
file modification times as UTC nanoseconds with nine fractional digits;
entry update/access times remain Unix-second timestamps.

### `query`

List cached entries matching a SQL `LIKE` path pattern
(`%` = any sequence, `_` = one character). `\` is the pattern's escape
character: write a literal `%`, `_`, or `\` as `\%`, `\_`, or `\\` (this
matters for Windows paths).

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

By default, a record whose path already exists in the target namespace
**overwrites** the stored entry — this is the same behavior as a bare
`--overwrite`. Pass `--overwrite=false` to leave existing entries untouched
instead; the command reports both counts and imports the remaining records:

```sh
localcache -d cache.sqlite3 import --overwrite=false -i backup.jsonl
# Imported 12 entries, skipped 3 existing
```

The skipped count is only printed when it is non-zero; with nothing skipped, the message is just
`Imported N entries`.

### `copy`

Copy all entries from one namespace to another. `--to` defaults to the
`-n`/`--namespace` global option when omitted.

```sh
# Within one database.
localcache -d cache.sqlite3 copy --from embeddings --to embeddings_v2

# From another database file into the `-d` database.
localcache -d new.sqlite3 copy --from-db old.sqlite3 --from embeddings --to embeddings
```

`--from-db` names the source database and defaults to the `-d`/`--database`
global option, which is also the destination. The source is always opened
**read-only**, so copying never modifies it. If the source has an older schema,
the read-only open refuses it, `copy` prints the library's message and a hint,
exits non-zero, and leaves the source file unchanged:

```text
error: unsupported feature: read-only open requires the current database schema; …; database was not modified
hint: to upgrade the source database to the current schema first, add --upgrade-source
```

Add `--upgrade-source` to open such a source writable and upgrade it to the
current schema first. Back it up before you do. It has no effect on a source that
already has the current schema.

### `migrate` *(deprecated)*

**Deprecated: `migrate` will be removed in 0.22.0. Use `copy --from-db`.** It
prints a warning to stderr each time it runs. `copy --from-db` makes the same
copy, and unlike `migrate` it leaves the source alone unless you pass
`--upgrade-source`.

Copy a namespace from one database to another — the source namespace is left
in place, not removed. Defaults: `--src-ns` is `default`; `--dst-db` defaults
to the `-d`/`--database` global option; `--dst-ns` defaults to the
`-n`/`--namespace` global option.

```sh
localcache migrate \
  --src-db old.sqlite3 --src-ns default \
  --dst-db new.sqlite3 --dst-ns default
```

`migrate` opens both databases writable. Its source may be upgraded to the
current schema before entries are copied, so back it up and plan any required
maintenance window first. Observational commands (`list`, `stats`, `check`,
`scan`, `export`, `query`, `inspect`, and `namespaces`) open read-only and
therefore never create or migrate the cache. `watch` also opens writable and
may initialize an empty database or upgrade an older one to the current
schema, the same as any other writable command.

### `watch` *(requires `watching` feature)*

Watch cached files for changes and print invalidation events in real time.
Press Ctrl-C to stop.

```sh
localcache -d cache.sqlite3 watch
```

Output: `[YYYY-MM-DD HH:MM:SS] MODIFIED /path/to/changed/file.txt`

The reason column is padded to a fixed width (`MODIFIED`, `REMOVED `, `RENAMED `, each 8
characters including any trailing pad), so `REMOVED`/`RENAMED` lines show one extra space before
the path compared to `MODIFIED`.
