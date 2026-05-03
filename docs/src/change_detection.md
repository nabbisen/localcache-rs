# Change Detection

`localcache` uses file metadata and optional content hashing to decide
whether a cached entry is still valid.

## Modes

### `MetadataOnly`

Compares `mtime` (last-modified timestamp) and `file_size`.

- **Fastest** — no file reads after the initial `stat` call.
- **Safe for most use cases** — false positives (unnecessary recomputation)
  are rare; false negatives (stale cache served as fresh) require an
  adversarial actor or a filesystem that lies about `mtime`.

```rust
.change_detection(ChangeDetectionMode::MetadataOnly)
```

### `MetadataThenFullHash`

Compares metadata first.  If metadata differs, computes a **full BLAKE3
hash** of the file and compares it with the stored hash.

- **Best overall trade-off** — only reads the file when metadata suggests
  a change.
- Use when you want to avoid unnecessary recomputation (e.g. a `touch`
  command that updates `mtime` without changing content).

```rust
.change_detection(ChangeDetectionMode::MetadataThenFullHash)
```

### `MetadataThenPartialHash`

Like `MetadataThenFullHash` but hashes only the **head and tail** of the
file (64 KiB each) using a `partial:` prefix on the stored hash.

- **Good for large files** — catches most real-world changes (appends,
  truncations, header rewrites) without reading the whole file.
- May miss changes in the middle of very large files.

```rust
.change_detection(ChangeDetectionMode::MetadataThenPartialHash)
```

### `StrictFullHash`

Always computes a full BLAKE3 hash, regardless of metadata.

- **Most reliable** — suitable for content-addressed workflows or when
  `mtime` is unreliable (network filesystems, some containers).
- Reads the entire file on every `get_if_fresh` / `check_status` call.

```rust
.change_detection(ChangeDetectionMode::StrictFullHash)
```

## Choosing a mode

| Scenario | Recommended mode |
|---|---|
| General purpose | `MetadataThenFullHash` |
| Maximum speed; trusted filesystem | `MetadataOnly` |
| Large files; partial-change detection acceptable | `MetadataThenPartialHash` |
| Content-addressed; unreliable mtime | `StrictFullHash` |

## Diagnosing mismatches

Use `explain()` to see exactly why an entry is stale:

```rust
let diag = engine.explain("file.txt")?;
if let Some(diff) = diag.metadata_diff {
    println!("mtime changed:     {}", diff.mtime_changed);
    println!("  stored:  {}", diff.stored_mtime);
    println!("  current: {}", diff.current_mtime);
    println!("file_size changed: {}", diff.size_changed);
}
if let Some(hash_match) = diag.hash_match {
    println!("hash matches: {hash_match}");
}
```
