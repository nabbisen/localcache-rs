# Roadmap

See the live
[ROADMAP.md](https://github.com/nabbisen/localcache-rs/blob/main/ROADMAP.md)
on GitHub for the current backlog.

## Completed phases

| Phase | Version | Theme |
|---|---|---|
| 1 | 0.1 | Foundation — SQLite, bincode, BLAKE3 |
| 2 | 0.2 | Ergonomics — namespaces, batch ops, TTL |
| 3 | 0.3 | Performance — partial hash, streaming |
| 4 | 0.4 | Async & ecosystem — `AsyncCacheEngine`, zstd |
| 5 | 0.5 | Polish — JSON codec, `max_entries`, glob scan |
| 6 | 0.6 | Security — AES-256-GCM, true LRU |
| 7 | 0.7 | Operational — builder API, `cache_stats` |
| 8 | 0.8 | Workspace — CLI tool, `on_evict` |
| 9 | 0.9 | Portability — export / import |
| 10 | 0.10 | Queries — `QueryBuilder`, `contains`, `keys` |
| 11 | 0.11 | Query sorting — multi-column, `offset`, indexes |
| 12 | 0.12 | Release readiness — benchmarks, `ConnectionPool` |
| 13 | 0.13 | Observability — `tracing`, `explain()`, DX |
| 14 | 0.14 | File watching — `CacheWatcher`, `preload()` |
| 15 | 0.15 | Production hardening — `metrics`, debounce, namespaces |
| 16 | 0.16 | Documentation overhaul |

## Future directions

- `async-std` / `smol` feature variants
- Query index hints / explain plan
- Read-only shared-memory DB mode
- Recursive directory watching
- OpenTelemetry spans
