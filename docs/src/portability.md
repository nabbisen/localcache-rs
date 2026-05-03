# Data Portability

## Export and import

`localcache` can export and import cache entries as **JSON Lines**
(one record per line).  Each record contains all metadata and the
Base64-encoded payload — making it fully text-portable and easy to
inspect with standard tools.

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

`namespace_copy` is an alias with a more descriptive name:

```rust
let copied = dst.namespace_copy(&src)?;
```

## CLI copy and migrate

```sh
# Copy within the same database (namespace to namespace).
localcache -d cache.sqlite3 copy --from old_ns --to new_ns

# Migrate to a new database.
localcache migrate \
    --src-db old.sqlite3 --src-ns embeddings \
    --dst-db new.sqlite3 --dst-ns embeddings
```

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

`ScanOptions` supports glob patterns with `*`, `?`, and `{a,b}` expansion:

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
