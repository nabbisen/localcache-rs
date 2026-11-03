# Data Portability

## Export and import

`localcache` can export and import cache entries as **JSON Lines**
(one record per line).  Each record contains all metadata and the
Base64-encoded payload — making it fully text-portable and easy to
inspect with standard tools.

**The order of an export is the order of use, oldest first** (ties by the order the entries
were stored). From v0.23.0 this is what `export_entries` and `localcache export` produce; before,
the order was by write time, newest first, and was not documented. It matters because a record's
`last_accessed_at` is in seconds: entries used within the same second are told apart only by their
position, and an import stores records in order, so importing an export in order keeps their
relative recency. An export made before v0.23.0 carries no finer information to keep.

### Exporting

```rust
// Export to a Vec<ExportRecord>.
let records = engine.export_entries()?;

// Serialise to JSON Lines manually.
for record in &records {
    println!("{}", serde_json::to_string(record)?);
}
```

Via the CLI:

```sh
localcache -d cache.sqlite3 export > backup.jsonl
localcache -d cache.sqlite3 -n embeddings export -o embeddings.jsonl
```

### Importing

```rust
// Import from a Vec<ExportRecord>.
// Existing entries for the same path are replaced.
let imported = engine.import_entries(&records)?;
println!("imported: {imported}");
```

**`max_entries` applies to imports.** On an engine with `max_entries(n)`, records for more than `n`
**distinct** paths (the stored key; the same path twice counts once) are rejected before anything
is written, with `InvalidArgument { argument: "records", .. }`: split the import, or raise the
bound. An import that fits is stored together with the eviction it triggers in one transaction:
the least recently used older entries of the namespace make room, and the imported entries are
never the ones evicted. `import_from`, and the `import` and `copy` CLI commands built on it,
follow the same rule. (Before v0.22.0 an import ignored the bound and could leave the namespace
over it.) The CLI has no `max_entries` option, so it is unaffected.

**With an encryption key, an import accepts only bound records.** On an engine that has a key,
`import_entries` and `import_from` (and so the records `copy` moves into it) refuse the **whole**
import before writing, with `InvalidArgument { argument: "records", .. }`, if any record is not
sealed bound to its path (its tag does not end in `-aes256gcm-bound`: it is plain, or was sealed
by v0.23 or earlier). The check is of the tag, not the authenticity: a record sealed under another
key is imported and fails closed on read, like any corrupt entry. To bring older records forward,
import them into an engine **without** a key and call `rotate_encryption_key` on an engine with
the key. Without a key, records are stored verbatim, sealed or plain, and so are the CLI's `import`
and `copy`; the CLI takes no key. **An export is always verbatim** (the ciphertext as stored, Base64),
and a sealed namespace exported and imported, or copied, into another namespace of the same
database or into another database reads under the same key: the namespace is not part of what an
entry is bound to (use one key per trust domain).

**An import is a restore, not a use.** Each imported entry keeps the recency it was exported with, as
its `last_accessed_at` seconds (for a record from before v0.23.0 that was never read, whose
`last_accessed_at` is 0, its `updated_at`); an entry used after the import ranks above every imported
one. A record whose recency is negative, or too large to hold (the second times a million must stay below
2^62), makes the **whole** import fail before anything is written, with
`InvalidArgument { argument: "records", .. }`.

Via the CLI:

```sh
localcache -d new_cache.sqlite3 import < backup.jsonl
localcache -d new_cache.sqlite3 import -i embeddings.jsonl
```

### Piping between databases

```sh
# Copy one namespace to a new database in one command.
localcache -d src.sqlite3 -n embeddings export \
  | localcache -d dst.sqlite3 -n embeddings_v2 import
```

## Cross-engine copy

`import_from` copies directly between two `CacheEngine` instances without
the Base64 round-trip — faster for large caches:

```rust
let src = CacheEngine::<Vec<f32>>::builder()
    .database("old_cache.sqlite3")
    .namespace("embeddings")
    .build()?;

let dst = CacheEngine::<Vec<f32>>::builder()
    .database("new_cache.sqlite3")
    .namespace("embeddings")
    .build()?;

let copied = dst.import_from(&src)?;
println!("copied {copied} entries");
```

## CLI copy

```sh
# Copy within the same database (namespace to namespace).
localcache -d cache.sqlite3 copy --from old_ns --to new_ns

# Copy from another database file.
localcache -d new.sqlite3 copy --from-db old.sqlite3 --from embeddings --to embeddings
```

`copy` opens its source read-only and its destination writable, so it never
modifies the source. A source with an older schema is refused and left
unchanged; add `--upgrade-source` to upgrade it first, after backing it up.

(The `migrate` command, which opened both sides writable, was removed in v0.22.0; `copy --from-db`
replaces it.)

## Preloading a directory

Cache an entire directory at once, skipping files that are already fresh:

```rust
use localcache::{CacheEngine, ScanOptions};

let report = engine.preload(
    "./corpus",
    ScanOptions {
        recursive: true,
        extensions: vec!["txt".into(), "md".into()],
        ..Default::default()
    },
    false,  // skip already-fresh entries
    |path| {
        let content = std::fs::read_to_string(path)?;
        Ok(compute_embedding(&content))
    },
)?;

println!("stored={} skipped_fresh={} errors={}",
    report.stored, report.already_fresh, report.skipped);

for (path, err) in &report.errors {
    eprintln!("error {}: {}", path.display(), err);
}
```

## Glob patterns

`ScanOptions` supports `*`, `?`, and nested/multiple `{a,b}` alternatives.
Wildcards match Unicode scalar values, case-sensitively on every platform,
without Unicode normalization:

```rust
use localcache::ScanOptions;

// Match *.txt and *.md files.
let opts = ScanOptions {
    recursive: true,
    glob_pattern: Some("*.{txt,md}".into()),
    ..Default::default()
};

// Multi-group expansion.
let opts2 = ScanOptions {
    glob_pattern: Some("report_{2024,2025}_{q1,q2,q3,q4}.pdf".into()),
    ..Default::default()
};
```

Malformed braces, NUL, patterns over 16,384 UTF-8 bytes, nesting over 32
levels, and expansion beyond 256 alternatives return
`InvalidArgument { argument: "path_glob", .. }` before directory iteration. File names that are not valid UTF-8 return
`InvalidPath` rather than being skipped or converted lossily.
