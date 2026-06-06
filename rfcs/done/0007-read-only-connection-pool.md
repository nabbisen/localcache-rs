# RFC 0007 — Read-only Connection Pool (`ReadPool<T>`)

| Field    | Value |
|----------|-------|
| Status   | Implemented (v0.19.0) |
| Feature  | *(core, no feature flag)* |
| Touches  | new `src/pool/read_pool.rs` (or `src/read_pool.rs`), `src/pool.rs` (docs cross-reference), `src/lib.rs` (re-export) |
| Depends on | RFC 0004 (Implemented, v0.17.0) — shared-cache open mode is one of the two pool backends |

## Summary

Add `ReadPool<T>`: a cloneable, `Send + Sync`, **read-only** pool of N
independent SQLite connections over one cache database.  Read operations
check out a slot (round-robin with fallback scanning) and execute
concurrently — WAL allows unlimited simultaneous readers — instead of
serializing on the single mutex inside today's `ConnectionPool<T>`.

Write methods are **absent from the type**, making read-onlyness a
compile-time property rather than a runtime `Err(ReadOnly)`.

## Background

Requested by a downstream adopter (arama, RFC 002 Q2).  Their gallery
path fans out thousands of point lookups across worker threads; their
in-house engine served these from an `r2d2` read pool.  `localcache`'s
`ConnectionPool` is `Arc<Mutex<CacheEngine>>` — correct, but all
operations serialize on one connection.

v0.17.0's `shared_cache()` (RFC 0004) already enables the *pattern*:
open one read-only engine per thread and fan out manually.  This RFC
turns the pattern into a supported type so adopters don't each rebuild
the slot management, and closes Q2 completely.

Feasibility note: `CacheEngine<T>` is `Send` (rusqlite `Connection` is
`Send`/`!Sync`; this is what already lets `ConnectionPool` clones cross
threads), so a `Vec<Mutex<CacheEngine<T>>>` shared via `Arc` is sound.

## Requirements

- `ReadPool<T>` is `Clone + Send + Sync`; clones share the same slots.
- Pool size fixed at construction (`size >= 1`).
- Two connection backends, selectable at construction:
  - **independent** (default): each slot opens with plain
    `read_only` flags — fully independent page caches, maximum read
    concurrency (no shared-cache table locks);
  - **shared-cache**: each slot opens per RFC 0004
    (`file:…?mode=ro&cache=shared` + `PRAGMA query_only`) — one shared
    page cache, lower memory for large pools.
- Read-side API only: `get`, `get_if_fresh`, `batch_get`,
  `batch_get_fresh`, `check_status`, `check_status_batch`, `contains`,
  `keys`, `entry_count`, `cache_stats`, `list_entries`, `explain`, and a
  closure-based `query_run` / `query_dry_run` (mirroring
  `AsyncCacheEngine`).  No `set` / `remove` / `cleanup_*` / index
  mutation — these do not exist on the type.
- `:memory:` databases are rejected at construction with a clear error:
  N independent connections to plain `:memory:` would each see a
  *different* empty database (same pitfall as the watcher gotcha), and a
  read-only shared in-memory DB is permanently empty.  The error message
  points users to `shared_cache()` engines or `ConnectionPool` instead.
- Checkout never deadlocks and degrades gracefully under contention.

## Design

### Type and construction

```rust
/// A cloneable pool of read-only SQLite connections over one cache database.
pub struct ReadPool<T> {
    slots: Arc<Vec<Mutex<CacheEngine<T>>>>,
    next: Arc<AtomicUsize>,
}

impl<T> Clone for ReadPool<T> { /* Arc clones */ }

impl<T: Serialize + DeserializeOwned> ReadPool<T> {
    /// Open `size` read-only connections using `options`.
    ///
    /// `options.read_only` is forced to `true`; `options.shared_cache`
    /// selects the backend (see module docs).  Errors if
    /// `options.database_path` is `:memory:` or `size == 0`.
    pub fn open(options: CacheOptions, size: usize) -> Result<Self, LocalFileCacheError>;
}
```

Builder integration for ergonomics:

```rust
impl<T> CacheEngineBuilder<T> {
    /// Consume the builder and open a read-only pool of `size` connections.
    pub fn build_read_pool(self, size: usize) -> Result<ReadPool<T>, LocalFileCacheError>;
}
```

### Checkout strategy

Round-robin start index from an `AtomicUsize` (`fetch_add(1) % size`),
then scan all slots with `try_lock()`; if every slot is busy, blocking
`lock()` on the round-robin slot.  Properties:

- uncontended case: one atomic op + one uncontended `try_lock`;
- contended case: work spreads across slots before anyone blocks;
- no ordering across slots → no lock-ordering deadlock;
- guard is held only for the duration of one operation (all read ops are
  short, bounded SQLite calls).

```rust
fn checkout(&self) -> MutexGuard<'_, CacheEngine<T>> {
    let start = self.next.fetch_add(1, Ordering::Relaxed) % self.slots.len();
    for i in 0..self.slots.len() {
        if let Ok(g) = self.slots[(start + i) % self.slots.len()].try_lock() {
            return g;
        }
    }
    // All busy — block on the round-robin slot (poisoning handled like
    // ConnectionPool: propagate as UnsupportedFeature("mutex poisoned")).
    self.slots[start].lock().unwrap_or_else(/* poison recovery */)
}
```

### Read API delegation

Each method checks out a slot and delegates, exactly like
`ConnectionPool`'s wrappers minus the write surface.  `query_run` /
`query_dry_run` use the same hold-the-guard closure pattern as
`AsyncCacheEngine` (no `unsafe` needed in the sync case — the closure
runs on the caller's stack while the guard is alive):

```rust
pub fn query_run<F>(&self, build: F) -> Result<Vec<CacheEntry<T>>, LocalFileCacheError>
where
    F: for<'e> FnOnce(QueryBuilder<'e, T>) -> QueryBuilder<'e, T>,
{
    let guard = self.checkout();
    let q = build(guard.query());
    q.run()
}
```

### WAL prerequisites

A read-only connection to a WAL database needs the `-wal`/`-shm`
sidecars to be initializable.  This holds whenever a read-write engine
has opened the database at least once (the normal producer/consumer
deployment, and what RFC 0004's tests already exercise).  `ReadPool::open`
surfaces the underlying SQLite error unchanged if the database has never
been initialized; the module docs state the prerequisite explicitly.

### Relationship to existing types (docs table)

| Scenario | Recommended |
|---|---|
| Single writer, single thread | `CacheEngine<T>` |
| Mixed read/write across threads | `ConnectionPool<T>` |
| Read-heavy fan-out, separate writer | **`ReadPool<T>`** (this RFC) |
| Async | `AsyncCacheEngine<T>` |

## Test plan

- `open(size = 4)` on a populated file DB: all read methods return the
  same results as a direct engine.
- Concurrency: 8 threads × 1 000 `get_if_fresh` point lookups against a
  4-slot pool — completes without error; results correct.
- Concurrent reads while a separate writer engine inserts: readers see
  consistent snapshots (WAL semantics), no SQLITE_BUSY surfaced to
  callers.
- `:memory:` → constructor error with guidance text.
- `size == 0` → constructor error.
- Both backends (independent / shared-cache) pass the same suite
  (parameterized helper).
- Compile-fail assurance: the type has no write methods (API review;
  a `trybuild` test is optional).
- Criterion bench (non-gating): 10 k entries, fan-out lookups,
  `ConnectionPool` vs `ReadPool(4)` — documents the improvement and
  gives arama their Phase B numbers.

## Security considerations

Each slot is opened with `SQLITE_OPEN_READONLY` (plus
`PRAGMA query_only = ON` on the shared-cache backend), so even a bug in
the pool wrapper cannot mutate the database.  No new SQL surface.

## Open questions

1. Should checkout expose a `try_` variant returning
   `Err(WouldBlock)`-style for latency-sensitive callers?  Proposed: no
   for v1 — blocking on short read ops is bounded; add later if a real
   user needs it.
2. Slot count auto-tuning (e.g. default to `available_parallelism()`)?
   Proposed: no magic; explicit `size` parameter, document
   `available_parallelism()` as the natural choice.
