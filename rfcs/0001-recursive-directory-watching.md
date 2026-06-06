# RFC 0001 — Recursive Directory Watching

| Field    | Value |
|----------|-------|
| Status   | Proposed |
| Feature  | `watching` |
| Touches  | `src/cache/watcher.rs`, `src/cache/engine.rs` |

## Summary

Extend `CacheWatcher` and `CacheDebouncedWatcher` to support watching an
**entire directory subtree**, not just individually registered file paths.
This allows callers to start one watcher and automatically receive
invalidation events for any cached file under a given root, including files
added after the watcher starts.

## Motivation

Currently every path must be registered explicitly via
`CacheWatcher::watch(path)`.  When `preload` is used to bulk-cache a
large directory, the caller must then iterate `engine.keys()` and call
`watch()` on each path individually — O(n) registrations, fragile if new
files are added after startup.

`notify 8` already supports `RecursiveMode::Recursive` with a single
`watcher.watch(dir, RecursiveMode::Recursive)` call, so the OS-level
cost is the same as watching one file on Linux (`inotify` watches the
directory inode, not each file).

## Design

### New API

```rust
impl<T> CacheWatcher<T> {
    /// Watch all files under `dir` recursively.
    ///
    /// Any OS event for a file under `dir` that has a corresponding cache
    /// entry will trigger invalidation.  Files without a cache entry are
    /// silently ignored by the callback.
    pub fn watch_dir<P: AsRef<Path>>(
        &mut self,
        dir: P,
    ) -> Result<(), LocalFileCacheError>;

    /// Stop watching the directory `dir` (and its subtree).
    pub fn unwatch_dir<P: AsRef<Path>>(
        &mut self,
        dir: P,
    ) -> Result<(), LocalFileCacheError>;
}

impl<T> CacheDebouncedWatcher<T> {
    // Same pair of methods.
    pub fn watch_dir<P: AsRef<Path>>(&mut self, dir: P) -> Result<(), LocalFileCacheError>;
    pub fn unwatch_dir<P: AsRef<Path>>(&mut self, dir: P) -> Result<(), LocalFileCacheError>;
}
```

`CacheEngineBuilder` gets a new option:

```rust
impl<T> CacheEngineBuilder<T> {
    /// Pre-register every cached path's parent directory for recursive
    /// watching when `watcher()` / `debounced_watcher()` is called.
    ///
    /// Default: `false` (per-file registration as before).
    pub fn watch_dirs(mut self, enable: bool) -> Self;
}
```

### Internal changes

**`watcher.rs` — `CacheWatcher::new_with_paths`**

Add a second code path: instead of calling
`os_watcher.watch(path, NonRecursive)` per file, collect unique parent
directories and call `os_watcher.watch(dir, Recursive)` per directory.

```rust
// Current (per-file)
for path in &paths {
    if path.exists() {
        let _ = os_watcher.watch(path, RecursiveMode::NonRecursive);
    }
}

// New (per-directory, when watch_dirs = true)
let dirs: HashSet<&Path> = paths.iter()
    .filter_map(|p| p.parent())
    .collect();
for dir in dirs {
    if dir.exists() {
        let _ = os_watcher.watch(dir, RecursiveMode::Recursive);
    }
}
```

**Callback filter**

With recursive watching, OS events arrive for *all* files in the tree,
including those not in the cache.  The callback must filter:

```rust
// In the notify callback:
for path in &ev.paths {
    if let Ok(eng) = inner_cb.lock() {
        // Only invalidate if the path is actually cached.
        if eng.contains(path).unwrap_or(false) {
            let _ = eng.remove(path);
            let _ = tx.try_send(WatchEvent { path: path.clone(), reason });
        }
    }
}
```

`contains()` is a single `SELECT COUNT(*)` — cheap.

### `CacheDebouncedWatcher`

Same approach: replace per-file `watcher.watch(path, NonRecursive)` with
per-directory `watcher.watch(dir, Recursive)` inside
`CacheDebouncedWatcher::new_with_paths`.

### Backward compatibility

Default behaviour (per-file) is unchanged.  The new `watch_dirs` builder
option defaults to `false`.  Callers opt in explicitly.

## Test plan

- `watch_dir` emits events for files created after the watcher starts.
- `watch_dir` does **not** emit events for uncached files.
- `unwatch_dir` stops events for that subtree.
- Recursive and non-recursive registrations can coexist on the same watcher.
- `watch_dirs(true)` builder option: `preload` + `watcher()` combination
  auto-registers directories.
- Debounced variant: same coverage.

## Open questions

- Should `CacheEngine::watcher()` automatically enable `watch_dirs` when
  the number of unique parent directories is much smaller than the number
  of cached paths (e.g., > 100 paths sharing < 10 directories)?  Likely
  too magical — keep explicit opt-in.
