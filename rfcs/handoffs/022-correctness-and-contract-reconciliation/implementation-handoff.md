# RFC 022 Implementation Handoff — Correctness and Contract Reconciliation (v0.21.4)

RFC: `rfcs/accepted/022-correctness-and-contract-reconciliation.md` (accepted 2026-09-23; R6
amendment and Amendment 2 authorized the same day)
Milestones: Phase 24 **Q0a–Q0e and Q0g–Q0i**. Q0f, the release, gets its own handoff later.
**Revised 2026-09-23 for Amendment 2:** § 2 gained Q0a's corrections, § 4 (Q0c) was re-specified,
and §§ 4a–4c (Q0g, Q0h, Q0i) are new. If you read this handoff earlier, re-read §§ 1, 2, 4–4c, 6,
and 7.
QA companion: `rfcs/handoffs/022-correctness-and-contract-reconciliation/acceptance-qa-checklist.md`

## 0. What this is

This is a **non-breaking patch**. It fixes seven correctness defects that no current test catches,
repairs release tooling, and brings documentation and records back in line with the code.

**No public signature, schema, SQL-shape, wire-format, or dependency change, and no input that
v0.21.3 accepted may start returning an error.** If you find that one is needed, stop and file a
design request. Do not work around it.

Two standing principles from the owner govern every judgement call here:

- **"Finally clean, safe and secure, robust and sophisticated design."**
- **Public APIs must not confuse or mislead users.** When a name or doc and the behaviour disagree,
  ask which one a user would believe.

## 1. How to work through the slices

| Slice | RFC | Files (primary) |
|---|---|---|
| **Q0a** key rotation | R1 | `crates/localcache/src/cache/engine.rs`, `crates/localcache/src/cache/query.rs` (`EngineCore` use), tests |
| **Q0b** query `offset` | R2 | `crates/localcache/src/cache/query.rs`, `crates/localcache/src/cache/query/tests.rs`, `crates/localcache/tests/query.rs` |
| **Q0c** LRU eviction | R6 | `crates/localcache/src/cache/engine.rs`, `crates/localcache/src/db/repository.rs`, `crates/localcache/src/cache/entry.rs`, `crates/localcache/src/cache/options.rs`, `crates/localcache/src/cache/builder.rs`, `crates/localcache/tests/codec_lru.rs` |
| **Q0g** size change is conclusive | R7 | `crates/localcache/src/detection/strategy.rs`, `crates/localcache/src/cache/options.rs`, `crates/localcache/tests/storage.rs` |
| **Q0h** async batch results | R8 | `crates/localcache/src/cache/async_engine.rs`, `crates/localcache/tests/pool_observe.rs` |
| **Q0i** watcher helper configuration | R9 | `crates/localcache/src/cache/engine.rs`, `crates/localcache/src/cache/watcher.rs`, `crates/localcache/tests/watching.rs` |
| **Q0d** release tooling | R3 | `scripts/release.py`, `scripts/check_advisories.py`, `scripts/release-tools.toml`, `scripts/tests/`, `Makefile.toml`, `.github/workflows/docs.yaml` |
| **Q0e** hygiene, docs, records | R4, R5 | `crates/cli/src/main.rs` (+ new `crates/cli/src/main/tests.rs`), `crates/localcache/tests/query.rs`, rustdoc sites, `docs/src/`, `README.md`, `CHANGELOG.md` |

**Order: Q0a → Q0b → Q0c → Q0g → Q0h → Q0i → Q0d → Q0e, one slice at a time.** Slice letters
are identifiers, not positions. Q0f is the release, which is why the new slices start at g. Each slice is an independent review
point, and the working tree must hold exactly one slice's changes when you file its review request.
Follow the project cadence:

1. implement;
2. file the review request describing the **uncommitted** tree;
3. wait for the review in `.git-exclude/reviewed/`;
4. commit with the message the review gives;
5. start the next slice.

**Never commit before the review lands.** Q0e comes last because it documents the contracts the
code slices establish, and Q0d's widened gate must check Q0e's corrected install examples the first time.

**Reproduce first, for every code slice: Q0a, Q0b, Q0c, Q0g, Q0h, Q0i.** Write the new tests, run them against the unfixed
code, and **capture the failing output** before you change any production code. That output is
required evidence. A test that passes on v0.21.3 does not prove the defect is fixed. When the handoff
leaves a test parameter open (an offset, a size, a position), choose the value that makes the test
**fail on the unfixed code**, and say why in the review request. A test that passes on both trees
is still a welcome regression guard, but it is not a reproduction and must not be presented as one.

**CHANGELOG.** **Each slice writes its own entries** under the `## [Unreleased]` heading at the
top of `CHANGELOG.md`, in the same working tree as its code, and shows the CHANGELOG diff in its
review request. Where § 6 describes a code slice's CHANGELOG content, that describes what the slice
itself writes. Q0e only fixes the preamble and links. Q0f renames the heading to the version.

---

## 2. Q0a — Key rotation keeps the rotating engine working (RFC 022 R1)

### The defect

`CacheEngine::rotate_encryption_key(&self, new_key)` (`crates/localcache/src/cache/engine.rs`,
around line 624) re-encrypts rows and commits, but `self.encryption_key` still holds the old key.
The same engine then fails to read rotated rows and writes new rows under the old key.

### Implementation

1. Field: `encryption_key: Option<[u8; 32]>` → `encryption_key: std::cell::Cell<Option<[u8; 32]>>`,
   still `#[cfg(feature = "encryption")]`. Construct it in `open` with `Cell::new(...)`.
2. Readers take a copy with `.get()`:
   - `encode` and `decode` in `engine.rs`: `self.encryption_key.get().as_ref()`, via a local binding;
   - the watcher helper construction (around line 749): `self.encryption_key.get().map(|k| k.to_vec())`;
   - `rotate_encryption_key`: read `old_key` from `.get()`.
3. **`EngineCore`** (`engine.rs`, around line 1058): change
   `encryption_key: Option<&'e [u8; 32]>` to `encryption_key: &'e Cell<Option<[u8; 32]>>`.
   `decode_with` reads `core.encryption_key.get()` **at decode time**, so a `QueryBuilder` created
   before a rotation and run after it uses the current key. Update `CacheEngine::core()` and the
   `AsyncCacheEngine` query path, which builds `guard.core()` (`async_engine.rs`, around line 313).
4. In `rotate_encryption_key`, call `self.encryption_key.set(Some(new_key_arr))` **only after
   `tx.commit()?` returns**. On any earlier error, the cell is untouched.
5. *(Corrected by Amendment 2; see "Corrections from the Q0a review" below.)* Rustdoc on
   `CacheEngine::rotate_encryption_key` and `AsyncCacheEngine::rotate_encryption_key` must state:
   - this engine continues with the new key, even when nothing needed re-encryption;
   - rotation covers **one namespace**. Every other open engine on the same database **and
     namespace** must be reopened with the new key. Engines on other namespaces keep their key, a
     watcher needs no action, and `AsyncCacheEngine` clones share the engine.
6. **Verify, don't assume:** confirm that the `CacheWatcher` helper engine never decodes payloads,
   and so is unaffected by a stale key. Say in the review request how you confirmed it.

### Auto traits must not change

`CacheEngine` is `!Sync` already (`rusqlite::Connection`), so `Cell` should change nothing.
**Prove it.**

1. Before touching production code, determine on v0.21.3 whether `CacheEngine<Vec<f32>>` is
   `Send`, `Sync`, `UnwindSafe`, and `RefUnwindSafe`.
2. Assert exactly that set, with no new dependency, in `crates/localcache/tests/core.rs`:
   - positive: `fn assert_send<T: Send>() {}` and so on;
   - negative: the ambiguity trick below, which compiles only when `T` does **not** implement the
     trait:

```rust
trait AmbiguousIfSync<A> { fn check() {} }
impl<T: ?Sized> AmbiguousIfSync<()> for T {}
impl<T: ?Sized + Sync> AmbiguousIfSync<u8> for T {}
#[test]
fn cache_engine_is_not_sync() {
    let _ = <localcache::CacheEngine<Vec<f32>> as AmbiguousIfSync<_>>::check;
}
```

Report the four results you observed on v0.21.3. If any differs from after the change, stop.

### Tests (`crates/localcache/tests/builder_ops.rs`, in the existing encryption module)

1. **Same-engine read after rotation**: every rotated row decodes. Must fail on v0.21.3.
2. **Same-engine write after rotation**: reopen with the **new key only**, and every row decodes,
   including the one written after rotation.
3. Tests 1–2 through `ConnectionPool`.
4. Tests 1–2 through `AsyncCacheEngine`, on each backend the suite already runs. Follow the
   `macro_rules!` pattern in `crates/localcache/tests/pool_observe.rs` if you need all three.
5. **Failed rotation keeps the old key.** Make one encrypted row undecryptable, for example by
   overwriting its content with bytes that fail authentication. Rotation returns `Err`, and the
   engine still reads the other rows with the old key.
6. **QueryBuilder across rotation**: build `engine.query()`, rotate, `.run()`, and the rotated rows
   come back decoded. Both are shared borrows of the engine, so this compiles. It fails on v0.21.3
   only through test 1's defect, and after the fix it proves that `EngineCore` reads the key at
   decode time rather than at `query()`.

---

### Corrections from the Q0a review (RFC 022 R1 items 4–6, Amendment 2)

The review is `.git-exclude/reviewed/002-architect-q0a-key-rotation-review-2026-09-23.md`. It holds
the full detail, and these points are durable here:

- **`Ok` means the engine switched keys**, including when nothing needed re-encryption. Remove the
  early `Ok(0)` that skips `set`.
- **One `IMMEDIATE` transaction from load to commit.** Open it before
  `load_encrypted_payloads`, and load through it.
- **Scope:** rotation covers one namespace. Reopen engines on the same database **and namespace**.
  Engines on other namespaces keep their key. Watchers need no action. `AsyncCacheEngine` clones
  share the engine.
- Two new tests: one for the no-rows rotation, and one `#[cfg(test)]` hook test for the concurrent
  write. Both need failing-before output.

---

## 3. Q0b — `offset` counts only rows that materialize (RFC 022 R2)

### The defect

`materialize` (`crates/localcache/src/cache/query.rs`, around line 778) starts at `order[offset]`
**positionally**, then skips undecodable or payload-less rows and backfills. Tier 3
(`execute_tier3`) skips first, then applies `offset`. With `[bad, a, b, c, d]` and `limit(2)`, tier 1
returns `a, b`, then `b, c`: **`b` repeats.**

### Implementation

Rewrite `materialize`'s loop so that `offset` counts successes:

```text
to_skip   = q.offset
target    = q.limit (None → unbounded)
out       = []
idx       = 0
while out.len() < target and idx < order.len():
    want       = to_skip + (target - out.len())          // saturating; unbounded → rest
    window_end = min(idx + min(want, 500), order.len())  // 500 = payloads_for_ids' chunk
    fetch payloads for order[idx..window_end]
    for each candidate in that window, in order:
        missing payload or decode error → continue        // never counted
        if to_skip > 0 → to_skip -= 1; continue            // counted toward offset
        out.push(entry); stop the window if out.len() == target
    idx = window_end
```

- **Tier 3 is not changed.** The non-`json` `execute_query` and tiers 1/2 all route through
  `materialize`, so one fix covers every positional path.
- Keep the `DECODE_CALLS` counter at the single decode choke point.
- Update `materialize`'s doc comment. It currently describes the positional behaviour as
  "matching today's behaviour", which is the error being fixed.

### Tests

Unit tests in `crates/localcache/src/cache/query/tests.rs`. Reuse the orphan/corrupt fixture of
`undecodable_payload_and_missing_payload_row_are_skipped_and_backfilled`.

1. Candidates in path order `bad, a, b, c, d` (bad = corrupt), `order_by_path(true).limit(2)` at
   offsets 0, 2, 4: pages `[a,b]`, `[c,d]`, `[]`. **Must fail on v0.21.3.**
2. The same data forced into **tier 3**: add one non-`json` row (as the mixed-encoding test does)
   plus a field predicate that matches every good row. The pages must be identical to test 1.
3. Bad rows at the start, in the middle, at the end, and every row bad, each with offset > 0.
4. An orphan (no payload row) behaves exactly like a corrupt row.
5. `offset` beyond the materializable count returns an empty `Vec`, not `Err`.
6. The decode bound for `offset > 0` is `decode_calls ≤ offset + limit + bad_rows_encountered`.
   The existing `decode_count_is_bounded_by_limit_not_namespace_size` must still pass unmodified.

Integration tests in `crates/localcache/tests/query.rs`, in an **ungated** module, so they run with
no features (this closes the no-features gap `ROADMAP.md` notes under P2b):

7. `limit`, `offset`, and a primary `order_by_updated_at`, with no field predicate, including one
   undecodable row. Use a file-backed database. Make the bad row by opening it with `rusqlite`
   directly and deleting that row's `payloads` entry, or overwriting its content. Several
   integration tests already manipulate fixtures this way (`crates/localcache/tests/core.rs`,
   `crates/localcache/tests/storage.rs`). **Do not rely on a type-mismatched decode:** bincode's
   legacy format can decode foreign bytes into a wrong value instead of failing. The engine under
   test is still exercised only through its public API.

**Existing tests must pass unmodified.** If one encodes the positional behaviour, stop and report
it. It means the contract moved.

---

## 4. Q0c — `set` never evicts what it just wrote; eviction is deterministic (RFC 022 R6, re-scoped)

### What changed in this section, and why

This section was **re-specified on 2026-09-23** by RFC 022 Amendment 2. The first version made a
write count as an access. `last_accessed_at` has one-second resolution, so a same-second `touch`
could no longer protect an entry. The first version also added two new errors. Both would have
broken existing tests you were told to keep unmodified, and the new errors would have broken
callers in a patch release. **Do not implement the earlier version.** In v0.21.4 the recency
signal stays read-based, and nothing new returns an error. A true LRU and the rejections come in
v0.22.0 (RFC 026 and RFC 025).

### The defect (reproduced)

A probe is kept at `.git-exclude/tmp/lru-probe/`. `max_entries(2); set(a); set(b); get(a); get(b);
set(c)` returns `Ok(())`, and then `contains(c) == false`. New rows start at
`last_accessed_at = 0`, so a write is the first eviction candidate for its own `set`. `batch_set`
has the same defect: a batch within the bound can evict its own rows.

### Implementation

1. **`upsert_in_tx`** (`crates/localcache/src/db/repository.rs`, around line 151):
   - `last_accessed_at` behaviour is **unchanged**: `0` on insert, and an overwrite keeps it.
   - Return the row id, `Result<i64, _>`. It is already queried right after the insert. `upsert`
     passes it through.
   - Replace the stale comment ("reset to 0 on write") with the truth: `0` on insert means never
     read, and an overwrite keeps the last read time.
2. **`import_rows` is unchanged.** Imports keep the exported `last_accessed_at` on purpose.
3. **Eviction is one selection, then deletion by id.** Replace `delete_lru_n` and
   `list_lru_n_paths` with one repository function:

```rust
/// RFC 022 R6: evict up to `n` rows of `namespace` in least-recently-read
/// order (last_accessed_at, updated_at, id), never touching `protected`
/// ids. Returns the paths of the rows actually deleted.
pub(crate) fn evict_lru(
    conn: &Connection,
    namespace: &str,
    n: usize,
    protected: &[i64],
) -> Result<Vec<PathBuf>, LocalFileCacheError>
```

   - Select `id, path … WHERE namespace = ?1 AND id NOT IN (…protected…) ORDER BY
     last_accessed_at ASC, updated_at ASC, id ASC LIMIT ?`.
   - Then `DELETE … WHERE id IN (…)`, both inside **one** transaction. Chunk the `IN` lists at
     500.
   - **One chunk constant** (from the Q0b review). Add a single `pub(crate) const` for the 500-id
     chunk in `crates/localcache/src/db/repository.rs`, with a doc comment giving the reason: it
     stays below SQLite's historical `SQLITE_MAX_VARIABLE_NUMBER` of 999. Use it in
     `payloads_for_ids` (replacing its local `CHUNK`), in `evict_lru`, and in `materialize` in
     `crates/localcache/src/cache/query.rs` (replacing its local `WINDOW_CHUNK`). This is
     behaviour-neutral: three uses, one definition.
   - `idx_files_lru` is `(namespace, last_accessed_at, updated_at)`, and SQLite appends the rowid,
     so `id ASC` should be index-served with no temporary sort. **Check with
     `EXPLAIN QUERY PLAN` and include the plan in the review request.** A `USE TEMP B-TREE` line
     is a finding to report, not to absorb.
   - If fewer unprotected rows exist than `n`, delete all of them. This is not an error.
   - *(From the Q0c review.)* `evict_lru` takes the caller's `&Transaction` and opens none of its
     own.
4. **One `IMMEDIATE` transaction per write, eviction included** (RFC 022 R6 item 3, from the Q0c
   review):
   - `set` opens
     `rusqlite::Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)`, calls
     `upsert_in_tx`, enforces the bound in the same transaction, and commits. `batch_set` does the
     same with its whole batch.
   - `enforce_max_entries` counts and evicts inside that transaction, with `protected` = the
     call's own ids. It returns the evicted paths.
   - `on_evict` callbacks run **after** commit, with exactly those paths.
   - `Ok` means stored and bounded; `Err` means nothing changed. Say so in the `set`/`batch_set`
     rustdoc.
   - A test proves `Err` means nothing changed: a `#[cfg(test)]` hook fails the eviction step.
     See the Q0c review § 2 C1.
   - Write this slice's own CHANGELOG entries (see § 1 and § 6).
5. **Nothing is rejected.** `max_entries(0)` and a `batch_set` larger than `max_entries` follow
   from the one rule (RFC 022 R6 design item 4):
   - an oversized batch stores all of its entries, and the next write brings the namespace back
     within the bound;
   - `max_entries(0)` keeps only the most recent write.
6. **Docs in code.** Each states the read-based policy exactly (RFC 022 R6 design item 1):
   - `EntryInfo::last_accessed_at` and `ExportRecord::last_accessed_at`
     (`crates/localcache/src/cache/entry.rs`, around lines 50 and 106): Unix seconds of the last
     **read** (`get`, `get_if_fresh`, `touch`); `0` = never read; an overwrite does not change it.
   - `QueryBuilder::order_by_last_accessed` (`query.rs`, around line 428).
   - `CacheOptions::max_entries` (`options.rs`, around line 158) and
     `CacheEngineBuilder::max_entries`, which must state:
     - eviction removes the least recently **read** entries: never-read first, then by oldest
       write, then first insertion;
     - a write never evicts the entries it wrote;
     - the two consequences in item 5, each with "rejected with an error from v0.22.0";
     - the bound is enforced by `set`/`batch_set`, not by `import_entries`, `import_from`, or
       `namespace_copy`.
7. **CLI `list`** (`crates/cli/src/commands/read.rs`): unchanged. `never` is accurate.

### Tests (`crates/localcache/tests/codec_lru.rs`, LRU section)

1. **The reproduction**, as a test. **Must fail on v0.21.3.**
2. `batch_set` within the bound, with older **read** entries present: none of the batch is
   evicted, and older rows go instead. **Must fail on v0.21.3.**
3. Same-second determinism: fill, read, and write within one second, with no `sleep`. The victim
   is fully determined by `(last_accessed_at, updated_at, id)`. Assert the exact survivor set.
4. `on_evict` receives exactly the deleted paths. Assert equality with the set that disappeared.
5. Oversized `batch_set` (5 distinct entries, `max_entries(2)`): `report.succeeded == 5`, and all
   5 are present. Then one `set`: `entry_count() == 2`, and the new entry is present.
6. `max_entries(0)`: `set(a); set(b)` leaves only `b`.
7. An imported `ExportRecord` keeps its `last_accessed_at` (unchanged behaviour).
8. **These must pass unmodified:**
   - `max_entries_evicts_oldest` and `lru_evicts_least_recently_accessed` (`codec_lru.rs`);
   - `touch_protects_from_lru_eviction` (`crates/localcache/tests/query.rs`);
   - the `on_evict_*` tests in `crates/localcache/tests/builder_ops.rs`;
   - the read-only `max_entries(0)` case in `crates/localcache/tests/read_only_contract.rs`.

   If one fails, stop and report it.
9. **The one authorized edit to an existing test:** rewrite `batch_set_respects_max_entries`
   (`codec_lru.rs`). Its assertion `entry_count() <= 2` encodes the defect: three entries reported
   stored are silently gone. Assert item 5's contract instead, and show the diff in the review
   request.

---

## 4a. Q0g — A size change is conclusive in the metadata-then-hash modes (RFC 022 R7)

### The defect (reproduced)

Under `MetadataThenPartialHash`, a file that grew from 335 872 to 438 272 bytes, with its first and
last 64 KiB unchanged, reports `Fresh`, and `get_if_fresh` returns the old payload. The probe is
`.git-exclude/tmp/arch-probe/`, check `[2]`.

### Implementation

- `crates/localcache/src/detection/strategy.rs`, in `detect_metadata_then_partial_hash` and
  `detect_metadata_then_full_hash`: once the metadata differs, if
  `stored.file_size != current.file_size`, return `Ok(CacheStatus::Stale)` **before any hashing**.
- `detect_strict_full_hash` is unchanged.
- `explain()` is unchanged. Its `status` already comes from `check_status`, and its `hash_match`
  stays a diagnostic.
- Rustdoc on `ChangeDetectionMode::MetadataThenPartialHash`: it detects any size change and any
  change within the first or last 64 KiB. It does not detect a same-size change confined to the
  middle.

### Tests (`crates/localcache/tests/storage.rs`)

1. The reproduction: a file over 128 KiB, rewritten with a longer middle and the same head and
   tail. `check_status` is `Stale` and `get_if_fresh` is `None`. **Must fail on v0.21.3.**
2. `MetadataThenFullHash` with a size change → `Stale`.
3. An unchanged file → `Fresh` in both modes.

---

## 4b. Q0h — `AsyncCacheEngine` batch methods return one result per path (RFC 022 R8)

### The defect (reproduced)

After the engine lock is poisoned, `batch_get`, `batch_get_fresh`, and `check_status_batch` each
return 1 result for 3 requested paths (`vec![Err(e)]` in
`crates/localcache/src/cache/async_engine.rs`). The probe is `.git-exclude/tmp/arch-probe/`, check
`[1]`.

### Implementation

For each of the three methods:
- Move lock handling **inside** the blocking closure. On a poisoned lock, return one
  `Poisoned { resource: "AsyncCacheEngine" }` per path, as `ConnectionPool` does. The closure
  then cannot return `Err`.
- If `spawn` still returns `Err`, the only possible cause is the runtime's `AsyncTaskPanicked`.
  Return one `AsyncTaskPanicked` per path, and state that invariant in a comment and a
  `debug_assert!(matches!(e, LocalFileCacheError::AsyncTaskPanicked))`.
- Use one small private helper for "n copies of an error", not three copies of the loop.
- Add to each method's rustdoc the one-per-path sentence that `ConnectionPool`'s batch methods
  carry.

### Tests (`crates/localcache/tests/pool_observe.rs`)

Put these in the existing per-backend modules, using the `macro_rules!` pattern there.
1. Poison the engine the way `poisoned_mutex_recovers_on_subsequent_calls` does, then call each
   batch method with 3 paths. Each returns 3 results, all
   `Poisoned { resource: "AsyncCacheEngine" }`. **Must fail on v0.21.3.**
2. A payload type whose `Deserialize` impl panics, with 3 entries stored: `batch_get` returns 3
   results, all `AsyncTaskPanicked`.

---

## 4c. Q0i — Watcher helpers inherit the engine's database configuration (RFC 022 R9)

### The defect (reproduced)

A database opened with `JournalMode::Delete` has no `-wal` file before `watcher()` and has one
after. The helper connection opened with default options and switched the file to WAL. The probe
is `.git-exclude/tmp/arch-probe/`, check `[3]`.

### Implementation

1. `CacheEngine` gains `journal_mode: JournalMode` and `synchronous: SynchronousMode` fields,
   gated on `watching` exactly as `database_path` is. `open` sets them from the options.
2. One `#[cfg(feature = "watching")]` private function in `engine.rs` builds the helper's
   `CacheOptions` from the parent. It sets:
   - `database_path`, `namespace`, `change_detection_mode`, `codec`, `ttl`, and
     `payload_version`;
   - `journal_mode` and `synchronous` from the parent;
   - `read_only: false`;
   - everything else at its default. In particular: **no encryption key**, no compression, and
     no `max_entries`.
3. `CacheWatcher::new_with_paths` and `CacheDebouncedWatcher::new_with_paths` take that
   `CacheOptions` instead of an engine or nine separate arguments. Both are `pub(crate)`, so no
   public signature changes. `CacheEngine::watcher()` opens **exactly one** helper connection;
   delete the discarded first one.
4. In `CacheDebouncedWatcher`'s callback, when the helper lock cannot be taken, send no
   notification. That matches `CacheWatcher` and RFC 015 R5. Keep the RFC 018 R4 comment.

### Tests (`crates/localcache/tests/watching.rs`)

1. File-backed database opened with `JournalMode::Delete` and one entry. After `watcher()`, a raw
   `rusqlite` connection reads `PRAGMA journal_mode` = `delete`. **Must fail on v0.21.3.**
2. The same for `debounced_watcher()`.
3. The poisoned-lock path cannot be reached through the public API. Say in the review request
   that it is verified by review only.

---

## 5. Q0d — Release tooling (RFC 022 R3)

1. **Version gate covers every install example** (`scripts/release.py`, `VERSION_REFERENCE_TARGETS`
   / `VERSION_REFERENCE_PATTERN`, around lines 69–79):
   - Targets: `README.md` plus every `docs/src/**/*.md`, discovered by glob. Not a hand list, which
     would drift.
   - A **declaration line** is a line starting, after optional indentation, with `localcache`,
     optional spaces, `=`, then either `"X"` or `{ … version = "X" … }`. Every declaration line
     must name the exact coming version.
   - A declaration line whose version cannot be parsed **fails** the gate. It is never skipped.
   - Prose is out of scope: a backticked mention mid-line, e.g. `docs/src/dependency_security.md:110`.
   - Tests in `scripts/tests/`: both forms, column-aligned whitespace (`localcache         = {`),
     an unparseable declaration, and the prose exclusion.
   - **Do not fix the five stale `"0.19"` examples in this slice.** The gate must fail on them.
     Include that failing output as evidence, then leave them for Q0e.
2. **Retire the Makefile publish tasks** (`Makefile.toml`):
   - Remove `[tasks.pre-publish]`, `[tasks.publish-lib]`, `[tasks.publish-cli-only]`, and
     `[tasks.publish-all]`, and their header comment lines 11–14.
   - Replace those header lines with the procedure: `cargo make release` runs the RFC 009 R12
     canonical gates; publication is owner-only and manual, `cargo publish --workspace --locked`
     (RFC 009 R15).
   - `git grep` for any remaining reference and report the result.
3. **`live_fetch` must not retry HTTP error responses** (`scripts/check_advisories.py`, around
   line 566):
   - Add `except urllib.error.HTTPError as error:` **before** the `OSError`/`URLError` clause.
   - Return `(error.code, headers, body)`, with the body read bounded exactly like the success path
     (`MAX_RESPONSE_BYTES + 1`).
   - The existing status logic in `fetch_with_retry` then retries 5xx and fails fast on anything
     else.
   - Test `live_fetch` **itself** against a local `http.server` on `127.0.0.1:0` in a thread:
     404 → one attempt and a gate failure; 503 → retried. Hermetic, with no network.
4. **Pages workflow least privilege** (`.github/workflows/docs.yaml`):
   - Workflow-level `permissions: contents: read` only.
   - `build` job: `contents: read`, `pages: read`.
   - `deploy` job: `pages: write`, `id-token: write`.
   - Replace the `cargo install mdbook --vers "^0.5"` step with the pinned mdBook 0.5.4 download
     and SHA-256 check copied from `.github/workflows/ci.yaml` (around lines 178–189).
   - Keep all action SHA pins.
   - This workflow runs only on push to `main`, so it cannot be verified locally. Flag it in the
     review request as **verified at the first push**. If `configure-pages` needs more than
     `pages: read`, that becomes a correction.
5. **Artifact actions.** Check whether GitHub has announced a retirement date for the runtime used
   by `actions/upload-artifact@v4` / `actions/download-artifact@v4` in `ci.yaml`. **Report what you
   find, with the source link. Do not upgrade in this slice.** A dated constraint becomes an
   architect scheduling decision.
6. **Re-pin** every changed script in `scripts/release-tools.toml`.
7. Script tests pass normally **and** under a `PATH` stripped of `cargo`/`rustc`/`mdbook`/
   `rustup`/`cargo-audit`, with `CARGO_HOME` pointed at an empty directory. That is the standing
   `scripts/` requirement from Phase 21.

---

## 6. Q0e — Hygiene, documentation, records (RFC 022 R4, R5)

### Code hygiene (R4)

1. Move `crates/cli/src/main.rs`'s inline `#[cfg(test)] mod tests { … }` (around lines 362–419) to
   `crates/cli/src/main/tests.rs`, declared `#[cfg(test)] #[path = "main/tests.rs"] mod tests;`,
   the pattern `crates/cli/src/text.rs` uses. Pure move: same test count.
2. Replace the `isatty` FFI (`crates/cli/src/main.rs`, around lines 345–360) with
   `std::io::IsTerminal`. Behaviour must be identical on unix. Check what the non-unix branch did
   and keep it equivalent. After this, `git grep -n unsafe -- crates` must show only comments.
3. Remove the stray `#[cfg(feature = "async")]` above `contains_returns_true_for_cached_entry`
   (`crates/localcache/tests/query.rs:11`). Confirm the test now runs with no features.
4. *(R4.4 is superseded by R6 and done in Q0c.)*
5. `PayloadVersionMismatch` (`crates/localcache/src/error.rs`): its doc says it is **reserved and
   not currently returned**. A version mismatch makes `get_if_fresh` return `None` and
   `check_status` return `Stale`. Do not remove it; that is Q3.
6. Give `QueryBuilder::index_hint`'s rustdoc its missing one-line summary. Its doc block starts
   with an empty `///` after `path_glob`. Fix `path_filter_clauses`' comment in `repository.rs`
   that says `path_like` uses "no ESCAPE". Remove the empty duplicate "Builder entrypoint"
   section banner in `engine.rs` (around line 885).
7. `CacheEngine::query`'s rustdoc (`engine.rs`, around line 929) still says every query is a
   linear scan that routes through `serde_json::Value`. Describe the three RFC 021 tiers in two
   sentences and point to `dry_run()`.

### Documentation (R5)

Every item was verified against the code on 2026-09-23. RFC 022 § R5 has the list. The details
below are what you need to act on it.

- **Install examples**: `docs/src/features.md:8`, `docs/src/async.md:154,157`,
  `docs/src/cookbook.md:210,235` → the same version as `README.md`'s Quick Start. Q0d's gate must
  then pass.
- **`docs/src/architecture.md`**:
  - Schema v5 DDL, with both built-in indexes (from `crates/localcache/src/db/schema/migration.rs`,
    `create_fresh`).
  - `mtime` in nanoseconds; `last_accessed_at` = last **read**, `0` = never read (Q0c).
  - `ON CONFLICT … DO UPDATE` rather than `INSERT OR REPLACE`.
  - Decoding is driven by the stored `encoding` tag, and configuration supplies only the key. This
    replaces lines 120–122, which contradict line 48.
  - The eviction policy and order from Q0c, including "a write never evicts what it wrote".
- **`docs/src/watching.md`**:
  - `preload` has 4 arguments (`dir, ScanOptions, force, factory`).
  - `let mut watcher` where `watch(&mut self)` is called.
  - Replace the `Arc<CacheEngine>` thread example (around lines 139–157, which cannot compile
    because `CacheEngine` is `!Sync`) with the thread-ownership pattern the page recommends: open
    the engine inside the thread.
  - macOS backend → FSEvents; the same fix in `docs/src/features.md`.
- **`docs/src/async.md`**:
  - "`CacheEngine` is `Send` but not `Sync`" (around line 139).
  - Fix the anchor `#alternative-async-runtimes-v0170` (line 10) against the heading at around
    line 147.
  - Remove the duplicated H1.
- **`docs/src/querying.md`**:
  - The heading around line 48: only payload predicates and field ordering need `json`.
  - Add one sentence for Q0b's contract: *"`offset` and `limit` count only entries that decode
    successfully; entries that cannot be decoded are skipped, never counted."*
- **`docs/src/migration.md`**: the bincode 2 upgrade happened at 0.13.2 (line 3), consistent with
  line 220.
- **`docs/src/errors.md`**: a version mismatch → `None`/`Stale`, not `PayloadVersionMismatch`
  (around lines 21 and 94–95); the variant is reserved.
- **`docs/src/roadmap.md`**: current through Phase 23 (v0.21.1–v0.21.3), plus the Phase 24 outline.
  Take the milestones and version plan from `ROADMAP.md`; do not invent. Remove "M7 next"
  (around lines 79 and 156). Figures must match `docs/src/performance.md`.
- **`docs/src/cookbook.md`**:
  - "Query + export" (around lines 194–196) must not call `export_entries()` per result.
  - Remove the `bytes` span field (around line 264).
  - Number the recipes consistently.
  - In the encryption recipe, add the reopen-other-engines note from Q0a.
- **`docs/src/builder.md`**:
  - Add `journal_mode` / `synchronous`.
  - The `max_entries` paragraph, per Q0c § 4 item 6:
    - least recently **read**, never-read first;
    - a write never evicts what it wrote, and the two consequences;
    - the v0.22.0 rejections;
    - the bound is not enforced on import.
  - The `ttl` paragraph: one-second resolution; rejected under one second from v0.22.0.
  - Fix the rustdoc-style links around lines 157/161.
- **`docs/src/api.md`**:
  - Add `FileMetadata`, `SharedEngine`, `PathRegistrationError`, `CacheWatcher::watched_count`,
    `ConnectionPool::with`/`with_mut`, and `CacheEngine::scan_dir`/`scan_dir_filtered`.
  - Mark the `watching`-gated types.
- **`docs/src/cli.md`**:
  - `migrate` copies and does not move; document the defaults of `--src-ns`, `--dst-db`, and
    `--dst-ns`.
  - `copy --to` defaults to `-n`.
  - `inspect` output as printed; `check`/`inspect` force full-hash detection.
  - `watch` is writable and may initialize or migrate the database.
  - `watch` output spacing as printed; `import` prints the skipped count only when non-zero.
  - **Document the CLI as it behaves. Do not change CLI behaviour in this slice.** Naming is Q2.
- **`crates/cli/src/main.rs:1-19`** module doc: list all 15 subcommands, or point to `--help`
  rather than listing them.
- **`README.md`**:
  - Features table with all 10 library features.
  - Add `MetadataThenPartialHash` and `ReadPool` to Design Highlights.
  - Qualify "no background threads" (`watching` starts one).
  - Move "Project source archives" below Design Highlights.
  - Add a link to the mdBook user guide. Use the Pages URL from `.github/workflows/docs.yaml`'s
    deployment; if you cannot determine it, ask rather than guess.
- **`CHANGELOG.md`**:
  - The `## [Unreleased]` preamble says "closing Phase 24 Q0a–Q0e"; make it "Phase 24 Q0
    (RFC 022)", since Q0 now also includes Q0g–Q0i.
  - Correct the 13 release headings dated 2025 (0.1.0 through 0.13.0) to their tag dates in 2026
    (`git tag --format='%(refname:short) %(creatordate:short)'`).
  - *(From the Q0i review, N1.)* Add one clause to Q0i's `### Fixed` entry: watcher construction
    can no longer return `Poisoned { resource: "CacheWatcher" }`, because the lock that produced it
    no longer exists. In `docs/src/errors.md`, make sure the current variant table and patterns do
    not present watcher construction as a poisoning site. Leave the v0.21.0 migration note as it
    is, because it is history.
  - Rewrite every compare link to the repository's **unprefixed** tag names (`git tag` shows
    `0.19.1`, not `v0.19.1`).
  - Add 0.20.1 through 0.21.3, plus `[Unreleased]: …/compare/0.21.3...HEAD`.
  - Q0c's own `### Changed` entry (written **in Q0c**, not here) must state:
    - the eviction policy is least recently **read**, with never-read entries first;
    - an oversized `batch_set` now keeps everything it reports as stored;
    - `max_entries(0)`, an oversized `batch_set`, and a TTL under one second are rejected with
      an error from v0.22.0.
- **Amendment 2 rustdoc and docs items**: every bullet marked *(Amendment 2)* in RFC 022 § R5.
  These are what encryption covers (payload content only; paths and metadata stay unencrypted),
  the `path_like` escape character, `ReadPool::get`/`cache_stats`, the
  `AsyncCacheEngine` type doc, `CacheWatcher::watch`, `EncryptionError`, TTL resolution,
  `change_detection.md`, and the CLI's journal-mode note.

---

## 7. Not yours — do not do these

- No MSRV change, no dependency added, removed, or upgraded (Phase 24 Q1). This includes
  `static_assertions` and `zeroize`.
- No renames or deprecations: `order_by_updated_at`, `SortOrder`, CLI `migrate` (Q2).
- No change to which error variant any existing failure returns (Q3). **No new error for any
  input v0.21.3 accepted.** Rejecting `max_entries(0)`, oversized batches, and sub-second TTLs is
  RFC 025, in v0.22.0.
- No "write counts as access" change and no schema change. A true LRU is RFC 026, in v0.22.0.
- No module splits (Q2b). **Never mix a move with a fix.**
- No performance measurement or tuning. A deep `offset` getting slower in Q0b is the accepted cost.
- No release action: no version bump, tag, or publish. That is Q0f, which is owner-authorized.
- No push, unless the review asks for one to exercise CI.

## 8. Gates, for every slice

- `cargo fmt --all --check` clean
- `cargo make matrix` (`scripts/feature_matrix.py`) green on every row. Clippy is `-D warnings`
  everywhere.
- `cargo make msrv-check` under the 1.85 toolchain, for every code slice (Q0a–Q0c, Q0g–Q0i)
  and Q0e. Use a fresh `--output-dir` per run until the re-run defect in the Phase 24 register is
  fixed. Report every attempt, including failed ones.
- `python3 scripts/source_integrity.py --require-tracked` OK
- Script tests (`python3 -m unittest discover -s scripts/tests`) for Q0d, including the
  restricted-`PATH` run
- The full suite: **report the count you observe.** The architect's baseline on 2026-09-23 was 425
  under `cargo test --workspace --all-features`. Report yours; do not restate this one.
- `git status --porcelain` shows only the slice's files, with no scratch residue. Scratch belongs
  in `.git-exclude/tmp/`.

## 9. Review request, for every slice

File it as `.git-exclude/review-request/NNN-dev-q0X-<topic>-<YYYY-MM-DD>.md`:

- `NNN` is the next free number in that folder, which keeps its own sequence.
- Name this handoff, the slice, and the RFC requirement at the top.

Contents, per the organization workflow § 9.2:

1. implementation summary;
2. the RFC 022 requirements addressed;
3. changed files;
4. important implementation decisions;
5. differences from this handoff;
6. tests added and run;
7. **the failing-before output** (Q0a–Q0c, Q0g–Q0i) and the passing-after output, plus the
   slice's CHANGELOG diff;
8. gate results, with the commands as run;
9. unresolved issues;
10. known limitations;
11. requested review focus.

Report any judgement call. Do not absorb it silently.
