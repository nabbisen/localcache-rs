# File Watching

Requires the `watching` Cargo feature.

`localcache` integrates OS-native file-system events via
[`notify 8`](https://crates.io/crates/notify).  When a watched file is
modified, renamed, or deleted, the corresponding cache entry is
automatically removed from the database and a `WatchEvent` is sent on
the event channel.

## Basic usage

```rust
use localcache::{CacheEngine, WatchEvent, InvalidationReason};

let engine = CacheEngine::<Vec<f32>>::builder()
    .database("cache.sqlite3")
    .build()?;

// Populate the cache (sets up paths to watch).
engine.set("document.txt", &embedding)?;

// Start watching — auto-registers all currently cached paths.
let watcher = engine.watcher()?;
let rx = watcher.events();

// In your event loop:
for event in rx.iter() {
    match event.reason {
        InvalidationReason::FileModified => println!("changed: {}", event.path.display()),
        InvalidationReason::FileRemoved  => println!("deleted: {}", event.path.display()),
        InvalidationReason::FileRenamed  => println!("renamed: {}", event.path.display()),
    }
    // The entry has already been removed from the DB.
    // Re-compute if needed:
    if event.path.exists() {
        let payload = recompute(&event.path)?;
        engine.set(&event.path, &payload)?;
        watcher.watch(&event.path)?;  // re-register
    }
}
```

> **Important**: `CacheWatcher` must remain alive for events to be
> delivered.  Dropping it stops the OS watcher and closes the channel.
> Use `events()` (which borrows `self`) rather than consuming the watcher.

## Watching additional paths

```rust
let mut watcher = engine.watcher()?;

// Watch a file that wasn't in the cache at construction time.
watcher.watch("new_file.txt")?;

// Stop watching a specific path.
watcher.unwatch("old_file.txt")?;
```

## Recursive directory watching

Instead of registering every file individually, watch an entire directory
subtree.  Only files that have a corresponding cache entry trigger
invalidation — uncached files are silently ignored by the callback.

```rust
let mut watcher = engine.watcher()?;

// All files under /data/docs and its subdirectories:
watcher.watch_dir("/data/docs")?;

// Stop watching a subtree:
watcher.unwatch_dir("/data/old")?;
```

`CacheDebouncedWatcher` exposes the same `watch_dir` / `unwatch_dir` pair.

### Builder flag: `watch_dirs(true)`

When set on the builder, `watcher()` and `debounced_watcher()` automatically
register each cached path's **parent directory** rather than individual files:

```rust
let engine = CacheEngine::<Vec<f32>>::builder()
    .database("cache.sqlite3")
    .watch_dirs(true)          // one OS watch per directory
    .build()?;

// After preload(), all directories in the cache are registered recursively.
engine.preload("/data", &ScanOptions { recursive: true, ..Default::default() }, |p| {
    Ok(compute(&p)?)
})?;
let watcher = engine.watcher()?;  // auto-registers parent dirs
```

This reduces O(n) per-file OS registrations to O(d) per-directory registrations,
where d is the number of distinct parent directories.

### Coexistence

Recursive directory registrations and per-file registrations can coexist on
the same watcher:

```rust
watcher.watch("specific_file.txt")?;   // per-file
watcher.watch_dir("/data/bulk")?;       // whole subtree
```

## Debounced watcher

Editors and build tools sometimes write a file many times per second.
`debounced_watcher` collapses rapid events into one:

```rust
use std::time::Duration;

let watcher = engine.debounced_watcher(
    Duration::from_millis(300)  // debounce window
)?;

// Events arrive at most once per 300 ms per path.
for event in watcher.events().iter() {
    println!("debounced: {}", event.path.display());
}
```

Internally this uses
[`notify-debouncer-mini`](https://crates.io/crates/notify-debouncer-mini).

## Spawning a watcher thread

To keep the watcher alive while your main thread does other work:

```rust
use std::sync::{Arc, Mutex};

let engine = Arc::new(engine);
let engine_watcher = Arc::clone(&engine);

let handle = std::thread::spawn(move || {
    let watcher = engine_watcher.watcher().unwrap();
    let rx = watcher.events();
    for event in rx.iter() {
        // handle events…
        let _ = event;
    }
    // watcher dropped here when thread exits
});

// Main thread continues using engine normally.
engine.set("file.txt", &payload)?;
```

## Platform support

| Platform | Backend |
|---|---|
| Linux | `inotify` |
| macOS | `kqueue` |
| Windows | `ReadDirectoryChangesW` |
| Other | fallback polling |

The backend is selected automatically via `notify::recommended_watcher()`.
