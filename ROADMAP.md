# Roadmap

## Phases 1–12 ✅  (see CHANGELOG for details)

## Phase 13 — Observability & Developer Experience (v0.13.x) ✅

- [x] `tracing` feature — `tracing::debug_span!` on `get`, `set`, `check_status`
      (zero-cost when feature is disabled)
- [x] `CacheEngine::explain(path)` → `Diagnosis` — structured staleness report:
      metadata diff, hash comparison, TTL remaining, payload version mismatch
- [x] `Diagnosis`, `MetadataDiff`, `PayloadVersionInfo` — new public types
- [x] `AsyncCacheEngine::explain(path)` — async variant
- [x] `ConnectionPool::explain(path)` — pooled variant
- [x] `QueryBuilder::order_by_last_accessed(ascending)` — sort by LRU timestamp
- [x] `QueryBuilder::then_by_field / then_by_updated_at / then_by_last_accessed /
      then_by_path` — multi-column sort (Vec<OrderBy> instead of Option<OrderBy>)
- [x] `rust-version = "1.85"` in `Cargo.toml` (MSRV, matches edition 2024)
- [x] CLI `inspect <PATH>` — human-readable staleness diagnosis using `explain()`

## Future / Unscheduled

- File-watching integration (`notify` crate)
- `async-std` / `smol` feature variants
- Query index hints / explain plan
- Read-only shared-memory DB mode
- `cargo publish` automation / release workflow
- Structured logging to metrics sinks (e.g. `metrics` crate)
