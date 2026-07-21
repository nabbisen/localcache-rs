# Migration Guide

## 0.13.x → 0.14.x (bincode 1.x → 2.x)

`localcache 0.14` upgraded `bincode` from 1.3.3 to 2.0.1.  The new version
uses `config::legacy()` which produces **byte-identical output** to bincode
1.x.  **Existing SQLite databases require no migration.**

The only breaking change is in the error type:
`LocalFileCacheError::Serialization` now wraps `String` instead of
`Box<bincode::ErrorKind>`.  If you matched on the inner value, update your
code:

```rust
// Before (0.13.x and earlier):
Err(LocalFileCacheError::Serialization(e)) => {
    // e: Box<bincode::ErrorKind>
    eprintln!("bincode error: {e:?}");
}

// After (0.14+):
Err(LocalFileCacheError::Serialization(msg)) => {
    // msg: String
    eprintln!("serialisation error: {msg}");
}
```

## Bumping `payload_version`

When your computation logic changes (new model, different analysis),
increment `payload_version` to force recomputation of all cached entries:

```rust
// Old code — version 1.
let engine = CacheEngine::<Vec<f32>>::builder()
    .payload_version(1)
    .build()?;

// New code — version 2.
let engine = CacheEngine::<Vec<f32>>::builder()
    .payload_version(2)
    .build()?;

// Purge all version-1 entries.
let purged = engine.purge_stale_versions()?;
println!("purged {purged} outdated entries");
```

## Migrating between databases

Use `export` / `import` or `migrate`:

```sh
# Export from old database.
localcache -d old.sqlite3 export -o backup.jsonl

# Import into new database.
localcache -d new.sqlite3 import -i backup.jsonl

# Or in one command:
localcache -d old.sqlite3 export | localcache -d new.sqlite3 import
```

Programmatically:

```rust
let src = CacheEngine::<T>::builder().database("old.sqlite3").build()?;
let dst = CacheEngine::<T>::builder().database("new.sqlite3").build()?;
let copied = dst.import_from(&src)?;
```

## Moving from `CacheOptions::open` to builder

Before (still works, not deprecated):

```rust
let engine = CacheEngine::<Vec<f32>>::open(CacheOptions {
    database_path: "cache.sqlite3".into(),
    change_detection_mode: ChangeDetectionMode::MetadataThenFullHash,
    ..CacheOptions::default()
})?;
```

After (recommended):

```rust
let engine = CacheEngine::<Vec<f32>>::builder()
    .database("cache.sqlite3")
    .change_detection(ChangeDetectionMode::MetadataThenFullHash)
    .build()?;
```

Both are equivalent — the builder simply populates a `CacheOptions` struct.

## Schema migrations

`localcache` handles schema upgrades automatically on `open()`.  Databases
from any supported version back to v0.1 are migrated transparently. Schema
migration and payload wire compatibility are separate guarantees: migration
changes SQLite tables while preserving payload bytes; the wire-format
guarantee below controls how those bytes decode.

| DB version | localcache version | Change |
|---|---|---|
| 1 | 0.1 | Initial schema |
| 2 | 0.2 | Added `namespace` column |
| 3 | 0.4 | Added `payload_version`, `encoding` |
| 4 | 0.6 | Added `last_accessed_at` + LRU index |
| 5 | 0.20 | `mtime` precision: whole seconds → **nanoseconds** |

### Atomic and payload-preserving upgrades

Fresh schema creation and every supported v1-through-v4 upgrade run in one
SQLite `IMMEDIATE` transaction. The transaction classifies the starting
schema, performs every required step, advances `user_version` once, validates
the final schema and foreign keys, and then commits. Any returned error or
normal panic unwind before commit rolls the complete operation back.

The v1-to-v2 step copies parent and child rows into paired shadow tables and
proves bidirectional equivalence before dropping either old table. File IDs,
metadata, missing-payload relationships, payload BLOBs, and the
AUTOINCREMENT high-water mark are preserved. Payloads are copied as raw bytes;
the migration does not need to decode them.

Plan capacity and downtime before upgrading a large cache. Preservation checks
copy every payload BLOB into a temporary snapshot, and a v1 upgrade also holds
the original and shadow tables until equivalence is proven. Peak storage can
therefore approach three times the payload data, plus SQLite journal or WAL
space. SQLite may place the temporary snapshot on a different filesystem, so
check free space for both the database and the system temporary directory. The
full comparison runs inside an `IMMEDIATE` transaction and blocks other writers
until it commits; schedule multi-gigabyte upgrades in a maintenance window and
retain a normal backup.

localcache 0.1.0 did not set `PRAGMA user_version`, so its real databases
report version 0. Initialization distinguishes that exact historical schema
from a truly empty database by read-only schema inspection. A non-empty
version-0 database with any other shape is rejected without localcache
mutation.

Schema recognition is deliberately strict. Missing or changed columns,
constraints, indexes, foreign keys, sequence state, triggers, views, and
unrelated co-located application objects are rejected. Because
`user_version` applies to the whole SQLite database, localcache does not guess
how to migrate a database shared with an unrecognized schema. Version-4
timestamps must use SQLite INTEGER storage and fall within
`-9_223_372_036..=9_223_372_036` seconds; invalid, already-nanosecond, or
partially converted values are rejected unchanged rather than coerced or
multiplied twice.

### Durability and runtime SQLite settings

Caller-selected journal and synchronous modes are delayed until schema
migration commits or an existing v5 schema passes no-write validation. A
file-backed migration requires an existing WAL or disk-backed rollback journal
and uses verified `synchronous=FULL` during the transaction. Requested
`synchronous` is applied and verified after commit, followed by requested
`journal_mode` as the last fallible configuration operation. Caller requests
for MEMORY journaling or synchronous OFF therefore cannot weaken the
migration transaction itself.

If post-commit runtime configuration fails, `open()` returns
`LocalFileCacheError::UnsupportedFeature` with the stable prefix
`database runtime configuration failed:`. Its fields include
`schema_migration_committed=true|false`, requested values, and observed values.
When the field is `true`, schema version 5 is already committed even though
engine construction failed; inspect the reported observed configuration and
retry opening rather than assuming the old schema remains.

`SQLITE_BUSY` while acquiring the migration transaction is returned without
an internal retry loop. Once the competing writer releases its transaction,
retry the complete `open()` call. Merely opening SQLite can perform SQLite's
normal journal or WAL recovery; that engine-owned recovery is outside
localcache's claim that rejected classification performs no migration.

In-memory databases receive transactional error atomicity but make no
process-crash durability claim. Keep normal application backups appropriate
to the value of your cache data; atomic migration is not a replacement for a
backup and does not create one automatically.

## Wire-format stability guarantee (v0.18.0+)

The `Bincode` codec (the default, and the only non-JSON codec) uses
`bincode::config::legacy()` throughout.  This is a **permanent, documented
commitment**:

> Payloads written by any `localcache` 0.x release are readable by every
> other 0.x release and any future 1.x release, without schema migration.

### What this means for your application

**You do not need to bump `payload_version` when upgrading `localcache`.**
The `payload_version` field is yours — increment it only when *your* payload
struct or embedding pipeline changes, not when the crate version changes.

| Trigger | Bump `payload_version`? |
|---|---|
| localcache version upgrade | **No** |
| Your struct gains / removes a field | **Yes** |
| You change your embedding model | **Yes** |
| Codec switched from Bincode → Json | **Yes** (different bytes) |

### What could break this guarantee

A deliberate, headline CHANGELOG item that introduces a schema-level
migration (e.g. `schema.rs` version bump) — like the 0.13.2 bincode 1→2
upgrade which used `config::legacy()` expressly *to preserve* this guarantee.
That event would be announced in the CHANGELOG, documented with migration
tooling, and backed by updated compatibility tests.

### How the guarantee is enforced

`crates/localcache/tests/compat.rs` opens the committed golden fixture
(`crates/localcache/tests/fixtures/compat-v0_18.sqlite3`, written by v0.18.0)
on every CI run
and asserts that all payloads decode to their expected bit-exact values.  A
change to the encoding path that breaks this test is caught before it reaches
any user database.
