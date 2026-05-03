# Roadmap

## Phases 1-10 ✅ (see CHANGELOG for details)

## Phase 11 — Query Enhancement & Hardening (v0.11.x) ✅

- [x] `QueryBuilder::order_by_field(field, ascending)` — sort by JSON payload field
- [x] `QueryBuilder::order_by_updated_at(ascending)` — sort by write timestamp
- [x] `QueryBuilder::order_by_path(ascending)` — sort by stored path string
- [x] `QueryBuilder::offset(n)` — skip first `n` matches (pagination)
- [x] `SortOrder` enum — `Asc` / `Desc`
- [x] `AsyncCacheEngine::query_run(|q| …)` — async query execution
- [x] `CacheEngine::touch(path)` — manually update `last_accessed_at` (LRU warm-up)
- [x] `CacheEngine::create_path_index(name)` — create `lc_user_*` SQLite index
- [x] `CacheEngine::drop_path_index(name)` — drop user index
- [x] `CacheEngine::list_path_indexes()` — enumerate user indexes
- [x] Async versions of `touch`, `contains`, `keys`, `create_path_index`,
      `drop_path_index`, `list_path_indexes`
- [x] CLI `query [--path-like PATTERN]` — list matching entries with status

## Future / Unscheduled

- File-watching integration (`notify` crate)
- `async-std` / `smol` feature variants
- `QueryBuilder`: `order_by_last_accessed`, multi-column sort
- Query index hints
- Read-only shared-memory DB mode
