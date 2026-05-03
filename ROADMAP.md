# Roadmap

## Phases 1–13 ✅  (see CHANGELOG for details)

## Phase 14 — File Watching & Reactive Invalidation (v0.14.x) ✅

- [x] `watching` Cargo feature — `notify 8` (OS-native file events, zero-cost
      when disabled)
- [x] `CacheWatcher<T>` — background watcher type; keeps OS watcher alive for
      event delivery
- [x] `CacheEngine::watcher()` → `CacheWatcher<T>` — auto-registers all
      currently cached paths; opens a dedicated DB connection for the callback
      thread
- [x] `CacheWatcher::watch(path)` / `unwatch(path)` — add/remove paths at runtime
- [x] `CacheWatcher::events()` — borrow the `mpsc::Receiver<WatchEvent>`
- [x] `WatchEvent { path, reason }` — new public type
- [x] `InvalidationReason` — `FileModified` / `FileRemoved` / `FileRenamed`
- [x] `CacheEngine::preload(dir, opts, force, factory)` → `PreloadReport` —
      bulk-cache a directory; skips fresh entries unless `force = true`
- [x] `PreloadReport { stored, already_fresh, skipped, errors }` — new public type
- [x] CLI `watch` subcommand — prints live invalidation events; gracefully
      degrades when `watching` feature is absent
- [x] `watching` feature added to `localcache-cli` (opt-in)

## Future / Unscheduled

- `async-std` / `smol` feature variants
- Query index hints / explain plan
- Read-only shared-memory DB mode
- `cargo publish` automation / release workflow
- Structured logging to metrics sinks (e.g. `metrics` crate)
- Recursive directory watching (watch whole subtree)
- Debounced invalidation (batch events within a window)
