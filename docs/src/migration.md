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
from any version back to v0.1 are migrated transparently.

| DB version | localcache version | Change |
|---|---|---|
| 1 | 0.1 | Initial schema |
| 2 | 0.2 | Added `namespace` column |
| 3 | 0.4 | Added `payload_version`, `encoding` |
| 4 | 0.6 | Added `last_accessed_at` + LRU index |

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

`tests/compat.rs` opens the committed golden fixture
(`tests/fixtures/compat-v0_18.sqlite3`, written by v0.18.0) on every CI run
and asserts that all payloads decode to their expected bit-exact values.  A
change to the encoding path that breaks this test is caught before it reaches
any user database.
