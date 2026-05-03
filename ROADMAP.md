# Roadmap

## Phases 1–14 ✅  (see CHANGELOG for details)

## Phase 15 — Production Hardening & API Refinement (v0.15.x) ✅

- [x] `metrics` Cargo feature — `metrics 0.24` counters and histograms on
      `get` (total / hit / miss) and `set` (total / bytes); label: `namespace`
- [x] `CacheEngine::namespace_list()` — list all distinct namespaces in the DB
- [x] `CacheEngine::namespace_copy(source)` — bulk-copy across namespaces /
      databases (alias of `import_from` with friendlier name)
- [x] `CacheEngine::debounced_watcher(window)` → `CacheDebouncedWatcher<T>` —
      OS events within `window` are merged, preventing floods from rapid writes
- [x] `CacheDebouncedWatcher<T>` — new public type (`watching` feature)
- [x] `notify-debouncer-mini 0.7` added to `watching` feature
- [x] CLI `namespaces` subcommand — tabular listing of all namespaces + entry
      counts in the target database
- [x] `Makefile.toml` — `cargo-make` task runner with tasks for:
      `check`, `test`, `bench`, `doc`, `pre-publish`, `publish-lib`,
      `publish-cli`, `publish-all`, `release-check`

## Future / Unscheduled

- `async-std` / `smol` feature variants
- Query index hints / explain plan
- Read-only shared-memory DB mode
- Recursive directory watching (watch whole subtree, `watching` feature)
- `metrics` integration tests with `metrics-util` recorder
- Structured logging to sink (e.g. `opentelemetry` spans)
