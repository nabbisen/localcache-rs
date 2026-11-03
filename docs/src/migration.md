# Migration Guide

## 0.13.1 → 0.13.2 (bincode 1.x → 2.x)

`localcache 0.13.2` upgraded `bincode` from 1.3.3 to 2.0.1.  The new version
uses `config::legacy()` which produces **byte-identical output** to bincode
1.x.  **Existing SQLite databases require no migration.**

The only breaking change is in the error type:
`LocalFileCacheError::Serialization` now wraps `String` instead of
`Box<bincode::ErrorKind>`.  If you matched on the inner value, update your
code:

```rust
// Before (0.13.1 and earlier):
Err(LocalFileCacheError::Serialization(e)) => {
    // e: Box<bincode::ErrorKind>
    eprintln!("bincode error: {e:?}");
}

// After (0.13.2+):
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

Use `export` / `import` or `copy --from-db`:

```sh
# Export from old database.
localcache -d old.sqlite3 export -o backup.jsonl

# Import into new database.
localcache -d new.sqlite3 import -i backup.jsonl

# Or in one command:
localcache -d old.sqlite3 export | localcache -d new.sqlite3 import

# Or copy a namespace straight from the old file (the source is opened read-only):
localcache -d new.sqlite3 copy --from-db old.sqlite3 --from default
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
| 6 | 0.23 | `last_used_us` (the use order) replaces `last_accessed_at` and its index; the duplicate path index is dropped |

### Atomic and payload-preserving upgrades

Fresh schema creation and every supported v1-through-v5 upgrade run in one
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

### Read-only opens do not migrate

An effective read-only open accepts only the exact current schema. It runs the
same strict classifier in one read snapshot but never creates tables, changes
`user_version`, migrates historical data, or applies journal and synchronous
configuration. Empty and recognized historical databases are therefore
rejected with `SchemaNotCurrent`; future and malformed schemas are rejected with
`IncompatibleSchema`. Both leave the database unmodified. A missing file remains
missing.

Use a writable open only when an upgrade is intentional. The CLI's
`copy --from-db` opens its source read-only and refuses an old-schema source;
its `--upgrade-source` flag opens the source writable and upgrades it, so apply
the same backup and maintenance-window planning described above. (The `migrate`
command, which always opened its source writable, was removed in v0.22.0.)

### Durability and runtime SQLite settings

Caller-selected journal and synchronous modes are delayed until schema
migration commits or an existing v5 schema passes no-write validation. (The
default, `JournalMode::Preserve`, selects nothing: it applies WAL only when
this open created the database, and otherwise leaves the journal mode as it
is, including through a migration.) A
file-backed migration requires an existing WAL or disk-backed rollback journal
and uses verified `synchronous=FULL` during the transaction. Requested
`synchronous` is applied and verified after commit, followed by requested
`journal_mode` as the last fallible configuration operation. Caller requests
for MEMORY journaling or synchronous OFF therefore cannot weaken the
migration transaction itself.

If post-commit runtime configuration fails, `open()` returns
`LocalFileCacheError::DatabaseState` whose message begins
`runtime configuration failed:`. Its fields include
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

### Schema v6 (v0.23.0): the use order, and one path index

The first **writable** open of a v5 database migrates it to v6 in one transaction. Two things
change in the file, and one in the meaning of a field.

* **Eviction follows uses, not reads.** The column `last_accessed_at` (Unix seconds of the last
  read, `0` for an entry never read) is replaced by `last_used_us`, a strict order of uses: a read
  hit, a `touch`, a write and an overwrite are all uses, and two uses never tie. An entry read once
  no longer outlives every entry that was only written. The migration places every existing entry
  by the second of its last use (`last_accessed_at` when above 0, otherwise `updated_at`), in the
  order of that second, then `updated_at`, then `id`, and gives entries that share a second
  consecutive values, so no two entries of a namespace share one. Entries never read are therefore
  ranked among the others by when they were written, which is the intended change and the one visible
  change in what a cache keeps.
* **`last_accessed_at` means the last use.** `EntryInfo::last_accessed_at`,
  `ExportRecord::last_accessed_at` and `SortKey::LastAccessed` keep their types and their unit
  (seconds) and now report the second an entry was last read **or written**; a stored entry no longer
  reports `0`. `localcache list` prints that second in a column named `LAST_USED`, and no longer
  prints `never`.
* **One path index.** `idx_files_namespace_path` duplicated the index SQLite already builds for
  `UNIQUE(namespace, path)`, and is dropped (with `idx_files_lru`, which `idx_files_use` replaces).
  Path lookups and the `path_in_dir` range query are served by the UNIQUE index with the same plans.

**What to do before you upgrade**

* **It is one way.** A migrated database is refused by v0.22.x and earlier (their classifier expects
  the dropped objects), and there is no downgrade command. To keep a way back, **export first**:
  `localcache export` (or `export_entries`) writes JSON Lines, which you can import into a fresh database of the older version.
* The migration runs in one transaction, takes about **8 seconds at 1,000,000 entries**, uses about
  20 MiB of memory, and needs free disk space of about **0.6 of the database's size** (measured at
  1,000,000 entries on one host: the file was 507 MiB and the footprint peaked at about 810 MiB). On a
  full disk, or on any failure, it leaves the database exactly as it was.
* **A migrated file does not shrink.** The dropped index and column leave free pages in it;
  `shrink_database()` (or SQLite's `VACUUM`) returns them. A *new* v6 database is smaller than a v5 one
  with the same entries, and writing is faster (see [Performance](./performance.md)).
* A read-only open of a v5 database reports `SchemaNotCurrent` until a writable open migrates it
  (`localcache copy --from-db … --upgrade-source` does that for a source). In v0.22.x, v5 is the
  current schema.
* A database whose stored recency is damaged (a last-use second that is negative, or that the use order
  cannot hold) is **refused**, not repaired: the migration fails with `DatabaseState` and changes nothing.
* **Export order and import.** An export now lists entries oldest use first, and an import keeps each
  entry's recency instead of treating the import as a use; see [Data Portability](./portability.md).
  An import refuses a record whose recency is out of range.
* Payload bytes, encryption, compression and the wire format do not change.

### Announced: encryption integrity (v0.24.0)

v0.24.0 will change what an engine with an encryption key accepts (RFC 029), and this page is where to find what to do about it. Nothing changes in v0.23.1, which only carries this announcement and the fix described in the CHANGELOG.

- **What v0.24.0 will do.** With a key, an entry will be readable only if it was sealed under that key **and bound to its path**; the namespace is not bound, so use one key per trust domain. A plain entry, or an entry sealed without binding, will be refused with a new error, `UnsealedEntry`, whose text names the remedy. A query on an engine with a key will never use the SQL pushdown. Entries written by a v0.24.0 engine carry new encoding tags (they end in `-aes256gcm-bound`).
- **Who must act, and how.** Everyone who has an encryption key and an existing database. After upgrading, call `rotate_encryption_key` once with the **same key**: it re-seals every entry of the namespace, in pages, in one atomic transaction. Or purge the entries. Databases without encrypted entries, and engines without a key, are not affected.
- **It is a one-way door for encrypted entries.** v0.23.x and earlier read the new tags as `UnknownEncoding`. **Do not call `rotate_encryption_key` with an older version once v0.24.0 has written to the database:** v0.23.0 returns `Ok(0)` for such a database and still switches its in-memory key, so a rotation that did nothing can look as if it worked, and the old key could be discarded by mistake. The way back is a purge, or an export made before.
- **What is not protected, and will not be.** The freshness of an entry (someone who can write the file can restore an older valid ciphertext of the same row, or delete entries); the confidentiality of metadata (paths, namespaces, sizes, times and the tag stay in the clear); isolation between namespaces that share one key; and anything against an attacker who can read the process's memory.
- **The wire-format guarantee below** ("Payloads written by any 0.x release are readable by every other 0.x release") will be scoped in place at v0.24.0, not annotated: it is about the payload codec bytes, and it will hold for every entry except a sealed entry read by an engine with a key, which needs the one rotation above. The page changes when the behaviour ships, not before.

The measured cost of binding is small: on a loaded host, a 64-byte `get` took about 0.19 µs (3%) longer in step-0 measurements.

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
