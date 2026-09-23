# RFC 022 — Correctness and Contract Reconciliation (v0.21.4)

| Field | Value |
|---|---|
| Status | Accepted (owner, 2026-09-23); R6 amendment accepted the same day; **Amendment 2** (R1 items 4–6, R6 re-scoped, new R7–R9) authorized by the owner the same day |
| Feature | *(core; `encryption` for R1; `json` affects R2's tiers; async features for R8; `watching` for R9)* |
| Touches | `crates/localcache/src/cache/engine.rs`, `crates/localcache/src/cache/query.rs`, `crates/localcache/src/db/repository.rs`, `crates/localcache/src/detection/strategy.rs`, `crates/localcache/src/cache/async_engine.rs`, `crates/localcache/src/cache/watcher.rs`, `crates/localcache/src/cache/entry.rs`, `crates/localcache/src/cache/options.rs`, `crates/localcache/src/read_pool.rs`, `crates/localcache/src/error.rs`, `crates/cli/src/main.rs`, `scripts/release.py`, `scripts/check_advisories.py`, `scripts/release-tools.toml`, `Makefile.toml`, `.github/workflows/docs.yaml`, `README.md`, `CHANGELOG.md`, `docs/src/`, `rfcs/README.md`, `ROADMAP.md` |
| Finding | Architect onboarding review, 2026-09-23; architect re-onboarding and Q0a review, 2026-09-23 (Amendment 2) |
| Milestone | Phase 24 Q0 |
| Breaking | **No** — no public signature, schema, wire-format, or dependency change, and no input v0.21.3 accepted now returns an error; targets v0.21.4 |
| Authorship | High-capability model; **reviewed by the owner** (arrangement of 2026-08-01) |
| Handoffs | [`../handoffs/022-correctness-and-contract-reconciliation/`](../handoffs/022-correctness-and-contract-reconciliation/implementation-handoff.md) — implementation handoff and QA checklist |

## Summary

The 2026-09-23 review of v0.21.3 found three latent correctness defects that no test catches, one
release-tooling path that cannot pass, and documentation that tells users to install a release the
project itself calls broken. This RFC fixes all of them in one non-breaking patch, **v0.21.4**:

- **R1** — `rotate_encryption_key` leaves the engine that performed the rotation holding the old key.
- **R2** — since RFC 021, `offset` counts undecodable rows in tiers 1 and 2 but not in tier 3, so
  paging repeats rows.
- **R3** — release tooling: the version gate misses most install examples; the Makefile publish
  path cannot pass; the advisory fetcher retries 404s; the Pages workflow over-grants write scopes.
- **R4** — small code-hygiene items with no behaviour change.
- **R5** — documentation and records reconciliation.
- **R6** — `max_entries` eviction can remove the entry the same `set` just wrote. *(Re-scoped by
  Amendment 2.)*
- **R7** — partial-hash detection reports a file `Fresh` after its size changed, and serves the old
  payload. *(Amendment 2.)*
- **R8** — `AsyncCacheEngine`'s batch methods return one result for many paths on failure.
  *(Amendment 2.)*
- **R9** — starting a watcher switches the database's journal mode to WAL. *(Amendment 2.)*

R1, R2, and R6–R9 are the reason this is a release rather than a docs sweep. They are designed
here. R3–R5 are mechanical, and are specified precisely enough that they need no further design.

## R1 — Key rotation must update the rotating engine

### Defect

`CacheEngine::rotate_encryption_key(&self, new_key)` re-encrypts every `-aes256gcm` payload in one
transaction, but never updates the engine's own `encryption_key` field. It cannot, because it takes
`&self` and the field is a plain `Option<[u8; 32]>`. After a successful rotation, **the same
instance**:

- fails every read of a rotated row with `EncryptionError`;
- encrypts every new write with the **old** key.

The database then holds rows under two keys, and no single key reads all of them. Long-lived
wrappers make this worse: `ConnectionPool` and `AsyncCacheEngine` hold one engine for their whole
life, so after `AsyncCacheEngine::rotate_encryption_key` every later operation runs on the stale key.

The tests (`crates/localcache/tests/builder_ops.rs`) only ever read through a freshly reopened
engine, which is why they pass. Verified by reading the code; the Q0a tests must reproduce the
defect before fixing it.

### Design

1. Store the key as `Cell<Option<[u8; 32]>>`. `CacheEngine` is already `!Sync`, because
   `rusqlite::Connection` is, so a `Cell` changes no auto trait. `Send`, `!Sync`, `UnwindSafe`, and
   `RefUnwindSafe` must all be **asserted by a compile-time test** before and after the change.
   *(Recorded at Q0a.)* On v0.21.3 the observed set was `Send`, `!Sync`, `!UnwindSafe`,
   `!RefUnwindSafe`. The two unwind-safety traits were already absent, because of the connection's
   `RefCell` statement cache and the `dyn Fn` eviction callback. The `Cell` changes none of the
   four. They are asserted in `crates/localcache/tests/core.rs`.
2. `rotate_encryption_key` keeps `&self` and its signature. It sets the new key **only after
   `tx.commit()` returns `Ok`**. On any error the engine keeps the old key, which still matches the
   rolled-back database.
3. `EngineCore` must not snapshot the key when `query()` is called. It borrows the `Cell` and reads
   the key at decode time, so a `QueryBuilder` built before a rotation and run after it decodes
   with the current key.
4. *(Corrected by Amendment 2.)* Rotation covers **only the rotating engine's namespace**. Every
   other open engine on the same database **and namespace** is not updated, and cannot be. That
   includes engines in other processes, other `ConnectionPool` instances, and `ReadPool` slots.
   Each must be reopened with the new key. Until then it returns `EncryptionError` on rotated
   entries. Engines on **other namespaces** are unaffected and must keep their own key. A watcher
   needs no action: its helper connection holds no key and never decodes payloads (verified in Q0a).
   `AsyncCacheEngine` clones share one engine, so they continue with the new key. The rustdoc of
   `CacheEngine::rotate_encryption_key` and `AsyncCacheEngine::rotate_encryption_key`, and the
   `docs/src/cookbook.md` encryption recipe, must say exactly this. The first version of this item
   said "every other open engine on this database", which is wrong for other namespaces. A user
   following it would lose access to their unrotated rows.
5. *(Amendment 2.)* **`Ok` means the engine switched keys**, including when no entry needed
   re-encryption. The first implementation returned `Ok(0)` early on an empty set and kept the old
   key. Every later write then used a key the caller believed retired (reproduced in the Q0a
   review).
6. *(Amendment 2.)* **The read-modify-write is one transaction.** Rotation opens an `IMMEDIATE`
   transaction **before** loading the encrypted rows, and holds it until commit. Before this, rows
   were loaded in autocommit mode and written back by id in a later transaction. A concurrent write
   landing between the two was overwritten with the old payload re-encrypted, under the new
   metadata: a Fresh entry holding stale data. The rustdoc already promised "a single SQLite
   transaction". Now the code keeps that promise. A concurrent writer waits or gets `SQLITE_BUSY`;
   it is never lost.

**Rejected:** changing the method to `&mut self`. It is the more obvious signature, but it is a
breaking signature change. Every caller holding `&CacheEngine`, including closures passed to
`ConnectionPool::with`, stops compiling. It would force a minor release for a correctness fix
whose `&self` form is equally sound.

### Tests

- Rotate and then read through the **same** engine: every rotated row decodes. This must fail on
  v0.21.3.
- Rotate, write a new entry through the same engine, reopen with the new key only: every row decodes.
- The same two checks through `ConnectionPool` and through `AsyncCacheEngine`, on each async
  backend the suite already covers.
- Force a failed rotation (for example, one row that cannot be decrypted with the current key): the
  engine still reads existing rows with the **old** key.
- A `QueryBuilder` created before a rotation and run after it returns the rotated rows, not an
  empty result.
- The compile-time auto-trait assertions from design item 1.
- *(Amendment 2.)* Rotating a namespace with nothing to re-encrypt, then writing through the same
  engine: the row decodes after reopening with the new key only (item 5).
- *(Amendment 2.)* A write from another connection between the load and the update is refused or
  preserved, never overwritten (item 6). This needs a `#[cfg(test)]` interleaving hook, following
  the pattern in `crates/localcache/src/db/indexes.rs`.

## R2 — `offset` counts only rows that materialize, in every tier

### Defect

RFC 021 promised that "`offset` + `limit` interaction is unchanged". Tier 3, like all code before
RFC 021, drops rows that have no payload or fail to decode **first**, then applies `offset`/`limit`.
Tiers 1 and 2 (`materialize` in `crates/localcache/src/cache/query.rs`) apply `offset` to the
**positional** candidate order first, then skip failures and backfill from later candidates.

With candidates `[bad, a, b, c, d]` and `limit(2)`:

| | page 1 (`offset 0`) | page 2 (`offset 2`) |
|---|---|---|
| tier 3, and pre-RFC-021 behaviour | `a, b` | `c, d` |
| tiers 1/2 today | `a, b` | **`b`**, `c` — `b` repeats |

Rows fail to decode in ordinary use: an encrypted namespace read by an engine without the key, or
payloads written before the application changed `T`, which is exactly what `payload_version` exists
for. The same query then returns different pages depending on which tier it takes, and the tier
depends on unrelated rows in the namespace (RFC 021's `namespace_all_json` check).

### Design

`offset` counts rows that **materialize successfully**, in every tier. That is the pre-RFC-021
contract, and it is what RFC 021 promised to keep.

`materialize` walks `order` from position 0. It decodes each candidate, discards the first `offset`
successes, then collects `limit` successes. It fetches payloads in windows, as it does today, sized
to `(offset − skipped_so_far) + (limit − collected)`, capped at the existing 500-id chunk.

**Cost.** `offset = 0` is unchanged. A deep `offset` now decodes `offset + limit` rows instead of
`limit`. That is still bounded by the page position, not by namespace size, and it never exceeds
what every query decoded before RFC 021. A correct page is worth more than a fast wrong one.

**Rejected:**

- Making tier 3 positional as well. It changes a contract that shipped for 20 releases, to match
  one that shipped for one.
- Positional pages with no backfill. Pages become short and unpredictable.

### Tests

Run each against a namespace forced into each tier:

- `[bad, a, b, c, d]`, `limit 2`, offsets 0/2/4: the pages are disjoint, their union is
  `{a, b, c, d}`, and results are identical across tiers.
- Undecodable rows at the start, in the middle, at the end, and as every row.
- A file row with no payload row, which behaves like an undecodable row.
- `offset` beyond the materializable count: an empty result, not an error.
- The existing RFC 021 decode-count test still holds for `offset = 0`. A new bound holds for
  `offset > 0`: decode count ≤ `offset + limit + (undecodable rows encountered)`.

The tier-1 cases, plus a primary `order_by_updated_at`, must also run **without** the `json`
feature. That closes the one real no-features query coverage gap, the narrower one behind
`ROADMAP.md`'s P2b note.

`docs/src/querying.md` must state the contract in one sentence.

## R3 — Release tooling

1. **Version gate covers every install example.** `VERSION_REFERENCE_TARGETS` in
   `scripts/release.py` becomes `README.md` plus every `docs/src/**/*.md`. The pattern must match
   both declaration forms at line start, with any whitespace before `=`:
   - `localcache = "X"`
   - `localcache = { version = "X", … }`

   Every matching line must name the exact coming version. A declaration line whose version cannot
   be parsed fails the gate. Prose mentions, such as `docs/src/dependency_security.md`'s backticked
   `localcache = "0.19"`, are not at line start and stay out of scope, as R10/R11 intend. Add
   tests for both forms, for column-aligned whitespace, and for the prose exclusion.
2. **Retire the Makefile publish path.** `pre-publish` cannot pass: `msrv-check` requires the 1.85
   toolchain and `doc-package-check` must not run under it. It also skips the security and archive
   gates. `publish-lib` / `publish-cli-only` / `publish-all` use the per-package publish that
   v0.20.1 showed can silently skip a crate. **Remove all four tasks** and their header comment
   lines. The procedure is `cargo make release` (the RFC 009 R12 canonical gates), then the owner
   runs `cargo publish --workspace --locked` by hand, per R15. Put this two-line procedure where
   the Makefile header lists tasks.
3. **The advisory fetcher must not retry HTTP error responses.** `urllib.error.HTTPError` is a
   subclass of `OSError`, so `live_fetch` currently wraps a 404 as `TransientFetchError` and
   retries it three times. That contradicts `fetch_with_retry`'s docstring. Catch `HTTPError`
   first and return its status, headers, and bounded body as a response, so the existing status
   logic decides: 5xx retries, and any other non-200 fails fast. Add a test that drives `live_fetch`
   itself against a local HTTP 404 server, not only the fake fetcher.
4. **Pages workflow least privilege.** In `.github/workflows/docs.yaml`, move `pages: write` and
   `id-token: write` from workflow level to the `deploy` job. Give the `build` job only what
   `configure-pages` needs; verify that with a real workflow run, and record the run. Replace
   `cargo install mdbook --vers "^0.5"` with the same pinned mdBook 0.5.4 download and SHA-256 check
   that `.github/workflows/ci.yaml` uses.
5. Re-pin every changed script in `scripts/release-tools.toml`.

## R4 — Code hygiene, no behaviour change

1. Move the inline `#[cfg(test)] mod tests { … }` at `crates/cli/src/main.rs:362-419` into
   `crates/cli/src/main/tests.rs`, as the Rust project rule requires.
2. Replace the CLI's `unsafe extern "C" fn isatty` (`crates/cli/src/main.rs`, around lines 345-360)
   with `std::io::IsTerminal` (stable since 1.70). This removes the workspace's last `unsafe`.
3. `crates/localcache/tests/query.rs:11` attaches `#[cfg(feature = "async")]` to the synchronous
   `contains_returns_true_for_cached_entry`. Remove the attribute so the test runs in every
   configuration.
4. *(Superseded by R6.)* This item first proposed keeping the write path's `last_accessed_at`
   behaviour and correcting only the comments. Re-reviewing that choice against the owner's
   "APIs for users not to be confused" principle exposed the R6 defect: the behaviour itself is
   wrong, not only the comment. R6 as re-scoped by Amendment 2 corrects that comment.
5. `PayloadVersionMismatch` is never constructed. Removing it would be breaking. Document it as
   reserved and currently unused, and fix `docs/src/errors.md`: a version mismatch makes
   `get_if_fresh` return `None` and `check_status` return `Stale`. Whether to use or remove the
   variant is left to Phase 24's error-taxonomy RFC.
6. Add the missing one-line summary to `QueryBuilder::index_hint`'s rustdoc; the doc block currently
   begins with an empty `///`. Correct `path_filter_clauses`' comment claiming `path_like` has "no
   ESCAPE".
7. Update `CacheEngine::query`'s rustdoc: it still says every query is a linear scan that routes
   through `serde_json::Value`, which RFC 021 made untrue for tiers 1 and 2.

## R5 — Documentation and records

Every item was verified against the code on 2026-09-23.

- **Install examples** at `docs/src/features.md:8`, `docs/src/async.md:154,157`,
  `docs/src/cookbook.md:210,235` → the coming version. R3.1 then keeps them current.
- **`docs/src/architecture.md`** → schema v5, `mtime` in nanoseconds, both built-in indexes, the
  `ON CONFLICT … DO UPDATE` upsert, and a corrected statement that decoding is driven by the stored
  encoding tag and that configuration supplies only the key. Remove the internal contradiction
  between line 48 and lines 120–122.
- **`docs/src/watching.md`**:
  - Fix the `preload` call (4 arguments).
  - Make the `watch` binding `mut`.
  - Replace the `Arc<CacheEngine>` thread example, which cannot compile, with the
    thread-ownership pattern the page already recommends.
  - macOS backend → FSEvents, the notify 8 default. Same fix in `docs/src/features.md`.
- **`docs/src/async.md`** → `CacheEngine` is `Send` but not `Sync`. Fix the broken
  `#alternative-async-runtimes-v0170` anchor and the duplicated H1.
- **`docs/src/querying.md`** → only payload predicates and field ordering need `json`. Add the R2
  sentence.
- **`docs/src/migration.md`** → the bincode 2 upgrade happened at 0.13.2, not "0.13.x → 0.14.x".
- **`docs/src/errors.md`** → per R4.5.
- **`docs/src/roadmap.md`** → current through Phase 23, with the Phase 24 plan in outline. Remove
  "M7 next", and replace the figures with ones matching `docs/src/performance.md`.
- **`docs/src/cookbook.md`**:
  - Fix "Query + export", which calls `export_entries()` once per result.
  - Remove the `bytes` span field, which the span does not record.
  - Number the recipes consistently.
  - Add R1's reopen-after-rotation note.
- **`docs/src/cli.md`**:
  - `migrate` copies and does not move; document its defaults.
  - `copy --to` defaults to `-n`.
  - `inspect` output as the code prints it; `check`/`inspect` force full hashing.
  - `watch` is writable and may initialize or migrate the database.
  - `watch` output spacing; `import` prints the skipped count only when non-zero.
  - Update the stale module doc at `crates/cli/src/main.rs:1-19`.
- **`docs/src/api.md` / `docs/src/builder.md`** → add the undocumented items: `FileMetadata`,
  `SharedEngine`, `PathRegistrationError`, `CacheWatcher::watched_count`,
  `ConnectionPool::with`/`with_mut`, builder `journal_mode`/`synchronous`, and
  `CacheEngine::scan_dir`/`scan_dir_filtered`. Mark the `watching`-gated types.
- **`README.md`**:
  - Features table → all 10 features.
  - Add `MetadataThenPartialHash` and `ReadPool` to Design Highlights.
  - Qualify "no background threads" for `watching`.
  - Move "Project source archives" below Design Highlights.
  - Link the mdBook user guide.
- **`CHANGELOG.md`** → add compare links for 0.20.1–0.21.4, and correct every existing link to the
  repository's actual, unprefixed tag names, as the Rust project rule requires.
- *(Amendment 2)* **Rustdoc that misstates behaviour**, each verified against the code on
  2026-09-23:
  - `QueryBuilder::path_like`, `CacheEngine::keys`, and the CLI `query --path-like` help: the
    pattern uses `\` as its `LIKE` escape character. A literal `%`, `_`, or `\` must be written as
    `\%`, `\_`, or `\\`. This matters for Windows paths. The same sentence goes in
    `docs/src/querying.md` and `docs/src/cli.md`.
  - `ReadPool::get`: the paragraph about `last_accessed_at` contradicts itself. State that a
    read-only slot never updates `last_accessed_at`.
  - `ReadPool::cache_stats`: says "hit-rate", which the method does not return.
  - `AsyncCacheEngine` type doc: says every operation runs on `tokio::task::spawn_blocking`. It
    runs on whichever runtime feature is active.
  - `CacheWatcher::watch`: says "no effect if the path does not exist". It returns an error.
  - `LocalFileCacheError::EncryptionError`: lists "missing `encryption` feature" as a cause. That
    case returns `UnknownEncoding`, and a missing key returns `UnsupportedFeature`. State both.
  - `CacheOptions::ttl` and `CacheEngineBuilder::ttl`: TTL has one-second resolution, and a
    duration under one second makes every entry immediately stale. It is rejected from v0.22.0
    (RFC 025).
  - `CacheOptions::max_entries` and `CacheEngineBuilder::max_entries`: per R6 design items 1 and 4.
  - `ChangeDetectionMode::MetadataThenPartialHash`: per R7.
- *(Amendment 2)* **`docs/src/change_detection.md`** → R7's contract. **`docs/src/cli.md`** → the
  writable commands open the database with WAL and `synchronous = NORMAL`, and WAL persists in the
  file. Document this; do not change it. Whether an engine should leave an existing journal mode
  alone is an API question for Q2.
- **`rfcs/README.md`** → the architect does this when this RFC is filed; listed here for
  completeness.
- **`ROADMAP.md`** → the architect maintains it; no implementer action.

## R6 — `set` never evicts what it just wrote; eviction is deterministic

> **Amendment history.** R6 was added on 2026-09-23 after acceptance, when the architect
> re-reviewed R4.4 against the owner's principle that APIs must not confuse users. The defect below
> is **reproduced** (scratch probe under `.git-exclude/tmp/lru-probe/`). The owner accepted it the
> same day.
>
> **Amendment 2, 2026-09-23 — R6 re-scoped, authorized by the owner.** The first design made a
> write count as an access. `last_accessed_at` has one-second resolution, so under that design a
> write and a later `touch` or `get` in the same second tie. The `id` tiebreak then evicts the
> entry the caller just touched. `touch` is documented as protection from eviction, and would stop
> providing it within the second; `touch_protects_from_lru_eviction`
> (`crates/localcache/tests/query.rs`) would fail. The first design also rejected `max_entries(0)`
> and oversized batches. That turns calls that returned `Ok` in v0.21.3 into `Err`, in a patch
> release. It would also have broken `batch_set_respects_max_entries`
> (`crates/localcache/tests/codec_lru.rs`) and a read-only `max_entries(0)` case in
> `crates/localcache/tests/read_only_contract.rs`. RFC 022's compatibility section had not
> disclosed any of this.
>
> The principled end state is still a true least-recently-used policy, where reads and writes
> both count. It needs a recency signal finer than one second, which is a schema change. It also
> needs rejection errors that use the variants Phase 24 Q3 defines. Both belong in **v0.22.0**, as
> **RFC 026** (recency) and **RFC 025** (rejections). This patch fixes the reproduced defect with
> one rule, and makes every document tell the truth about the policy it actually implements.

### Defect

A new entry is written with `last_accessed_at = 0`. An overwrite leaves `last_accessed_at`
unchanged. Eviction deletes in `last_accessed_at ASC, updated_at ASC` order. So once every other
entry has been read, **the entry a `set` just wrote is the first eviction candidate, and that same
`set` evicts it:**

```text
max_entries(2); set(a); set(b); get(a); get(b); set(c)
→ set(c) returns Ok(()), and contains(c) == false
```

The user sees `set` succeed and the next `get` miss. `batch_set` has the same defect at a larger
scale: its new rows all start at `0`, so a batch that fits within `max_entries` can evict its own
rows while older, read entries survive. The same-second `updated_at` tie is broken in an order
SQLite leaves unspecified.

### Design

1. **The recency signal is unchanged in v0.21.x.** `last_accessed_at` is the Unix-second time of
   the last **read** (`get`, `get_if_fresh`, `touch`). It is `0` if the entry was never read. An
   overwrite keeps it. Eviction therefore removes the **least recently read** entries, and entries
   never read go first. Every document states exactly that policy:
   - `CacheOptions::max_entries`, `CacheEngineBuilder::max_entries`, and
     `docs/src/builder.md`: replace "true LRU based on `last_accessed_at`";
   - `README.md`: replace "least recently accessed";
   - `docs/src/architecture.md`;
   - `EntryInfo::last_accessed_at` and `ExportRecord::last_accessed_at`: say "last read", and
     "`0` = never read"; an overwrite does not reset it;
   - `QueryBuilder::order_by_last_accessed`.

   The stale comment in `upsert_in_tx`, which claims a reset to `0` on every write, is corrected.
   The code only writes `0` on insert.
2. **A write never evicts the entries it wrote.** One rule, with no exception, for `set` (its one
   row) and `batch_set` (every row it wrote). Eviction chooses only among the namespace's other
   rows.
3. **Deterministic, exact eviction.**
   - Order: `last_accessed_at ASC, updated_at ASC, id ASC`. `id` is first-insert order, and an
     overwrite keeps its `id`.
   - One repository function selects the victims (excluding the protected ids) and deletes them
     by id, both in **one** transaction.
   - `on_evict` receives exactly the deleted paths, **after** commit.
   - The selection must be served by `idx_files_lru` without a temporary sort. SQLite appends the
     rowid to the index, so `id ASC` is covered. Check this with `EXPLAIN QUERY PLAN`.
4. **Consequences of rule 2, documented in the rustdoc and in `docs/src/builder.md`.** These are
   consequences, not exceptions:
   - A `batch_set` that stores more distinct entries than `max_entries` stores all of them, and
     reports all of them as stored. The namespace stays above the bound until the next write,
     which evicts down to `max_entries`.
   - `max_entries(0)` keeps only the entry written most recently.

   Both inputs are rejected with an explicit error **from v0.22.0** (RFC 025). The rustdoc and the
   `### Changed` entry announce this now. v0.21.3 silently deleted entries it had just reported as
   stored. Storing more than the bound for one call is the safe direction: no data is lost, and
   no `Ok` is false.
5. **No schema change, no new error, no public signature change.**

### Tests

- The reproduction above fails on v0.21.3 and passes after the fix.
- `batch_set` within the bound, with older read entries present: none of the batch is evicted,
  and older rows go instead. This fails on v0.21.3.
- Eviction order with same-second ties: the victim is fully determined, with no `sleep` and no
  reliance on unspecified order. Assert the exact survivor set.
- The `on_evict` paths equal the set of rows that disappeared.
- An oversized `batch_set` stores every reported entry, and the next `set` restores the bound.
- `max_entries(0)`: after a `set`, only that entry remains.
- An imported `ExportRecord` keeps its exported `last_accessed_at` (unchanged behaviour).
- These pass **unmodified**: `max_entries_evicts_oldest`, `lru_evicts_least_recently_accessed`,
  `touch_protects_from_lru_eviction`, and the read-only `max_entries(0)` case in
  `crates/localcache/tests/read_only_contract.rs`.
- **One existing test's premise is withdrawn:** `batch_set_respects_max_entries`
  (`crates/localcache/tests/codec_lru.rs`). It asserts that a 5-entry batch under `max_entries(2)`
  leaves at most 2 entries. That assertion encodes the defect: three entries reported as stored
  are silently gone. It is rewritten to assert rule 4 (all 5 present, then the bound restored by
  the next `set`). This is the only existing test this RFC authorizes changing, and the review
  request must show the diff.

## R7 — A size change is conclusive in the metadata-then-hash modes

*(Amendment 2, authorized by the owner 2026-09-23.)*

### Defect

`MetadataThenPartialHash` hashes the first and last 64 KiB and ignores the length. When a file's
size changes but its head and tail do not, for example after an insertion in the middle,
`check_status` returns **`Fresh`** and `get_if_fresh` **serves the old payload**. Reproduced on
v0.21.3 (`.git-exclude/tmp/arch-probe/`): 335 872 → 438 272 bytes, `status=Fresh`,
`size_changed=true`. The mode is documented as "may miss changes in the middle". A reader expects
that to mean same-size edits, not a file that grew by 100 KiB. A size change is certain evidence
that the content changed.

### Design

In `MetadataThenPartialHash` and `MetadataThenFullHash`, when the stored and current
`file_size` differ, the status is `Stale` **without hashing**. For the full-hash mode this changes
no result, because the digest would differ anyway, but it skips a full read of a file already
known to be changed. `StrictFullHash` is unchanged: its name promises a hash on every check, and
its result is identical anyway.

`docs/src/change_detection.md` and the `ChangeDetectionMode` rustdoc state the contract. Partial
hashing detects any size change and any change within the first or last 64 KiB. It does not detect
a same-size change confined to the middle.

### Tests

- The reproduction above, as an integration test in `crates/localcache/tests/storage.rs`. It must
  fail on v0.21.3.
- A size change under `MetadataThenFullHash` → `Stale`.
- Unchanged file → `Fresh` in both modes (regression).

## R8 — `AsyncCacheEngine` batch methods return one result per path

*(Amendment 2, authorized by the owner 2026-09-23.)*

### Defect

`AsyncCacheEngine::batch_get`, `batch_get_fresh`, and `check_status_batch` return
`vec![Err(e)]`, **one** element, when the engine lock is poisoned or the blocking task panics,
whatever the number of paths requested. A caller doing `paths.iter().zip(results)` silently loses
every path but the first. This is the defect v0.21.1 fixed for `ConnectionPool`; the third wrapper
was missed. Reproduced on v0.21.3: 3 paths requested, 1 result from each method.

### Design

The three methods return exactly one result per requested path on every path.
- Lock poisoning → one `Poisoned { resource: "AsyncCacheEngine" }` per path, built inside the
  blocking closure, as `ConnectionPool` does. The closure then cannot fail.
- A failed blocking task → one `AsyncTaskPanicked` per path. Once the closure cannot fail, the
  only error `spawn` can return is the runtime's `AsyncTaskPanicked`. The code states that
  invariant in a comment and a `debug_assert!`.
- The rustdoc of each method states the one-per-path guarantee, matching `ConnectionPool` and
  `ReadPool`.

### Tests

On every async backend the suite runs, using the `macro_rules!` pattern in
`crates/localcache/tests/pool_observe.rs`:
- Poison the engine (a panic inside `query_run`, as the existing poisoning test does), then call
  each batch method with 3 paths: 3 results each, all `Poisoned`. This must fail on v0.21.3.
- A payload type whose `Deserialize` panics: `batch_get` over 3 stored paths returns 3
  `AsyncTaskPanicked`.

## R9 — Watcher helpers inherit the engine's database configuration

*(Amendment 2, authorized by the owner 2026-09-23.)*

### Defect

`watcher()` and `debounced_watcher()` open a helper connection with **default** options. Opening
a writable engine applies its journal mode and `synchronous` setting, and journal mode is
persistent in the database file. A database the application opened with `JournalMode::Delete` is
therefore **switched to WAL** as a side effect of starting a watcher. Reproduced on v0.21.3: no
`-wal` file before `watcher()`, one after. That can matter a great deal: WAL does not work on
network filesystems, which is a common reason to choose `Delete`.

Two smaller defects in the same code:
- `watcher()` opens **two** helper connections. The first carries the encryption key and is
  discarded after its fields are read. The second, the one actually used, carries no key.
- `CacheDebouncedWatcher`'s callback, on a poisoned helper lock, skips the removal but still sends
  a `WatchEvent` claiming invalidation. `CacheWatcher` sends nothing in that case, and RFC 015 R5
  requires that no notification claim an invalidation that did not happen.

### Design

1. `CacheEngine` records the journal mode and `synchronous` setting it was opened with.
2. One internal function builds the helper's `CacheOptions` from the parent engine. It carries
   database path, namespace, detection mode, codec, TTL, payload version, journal mode, and
   `synchronous`. It carries **no** encryption key and no compression setting: the helper never
   encodes or decodes, so it should not hold key material.
3. Both watcher constructors use that function. `watcher()` opens exactly one helper connection.
4. The debounced callback sends nothing when it could not take the lock, matching `CacheWatcher`.

### Tests

- A database opened with `JournalMode::Delete`: after `watcher()`, and separately after
  `debounced_watcher()`, `PRAGMA journal_mode` on the file (read through a raw `rusqlite`
  connection) is still `delete`. This must fail on v0.21.3.
- The poisoned-lock path is not reachable through the public API. It is verified by review, and
  the review request says so.

## Non-goals

- Splitting `UnsupportedFeature`, or deciding `PayloadVersionMismatch`'s fate (Phase 24 Q3, breaking).
- **Any new error for input v0.21.3 accepted.** This covers the `max_entries(0)`,
  oversized-batch, and sub-second-TTL rejections. They go to RFC 025 (v0.22.0), under Phase 24's
  rule that a newly rejected input ships only in a minor release.
- **A true least-recently-used policy, where writes count.** It needs recency finer than one
  second, which is a schema change. That goes to RFC 026 (v0.22.0).
- Paging `rotate_encryption_key`'s full load of encrypted payloads into memory (registered; RFC
  020's paging would apply).
- Any MSRV or dependency change (Phase 24 Q1).
- `aggregate-ci` evidence-manifest cardinality, re-run output-directory handling, and
  `upload-artifact`/`download-artifact` major versions (registered in `ROADMAP.md`).

## Compatibility and release

No public signature, schema, SQL shape, wire-format, or dependency change. **No input that
v0.21.3 accepted now returns an error.**

**Observable behaviour corrections**, each restoring a documented or reasonably expected
contract:
- **R1:** after rotation, the rotating engine uses the new key, including when nothing needed
  re-encryption. A concurrent write during rotation waits or fails with busy; it is no longer
  overwritten.
- **R2:** `offset` pages in tiers 1 and 2 match tier 3 and pre-0.21.3 behaviour when undecodable
  rows exist.
- **R6:** a write never evicts what it wrote, and eviction order is deterministic. The one visible
  change: an oversized `batch_set` keeps every entry it reports as stored, instead of silently
  deleting some of them.
- **R7:** a file whose size changed is `Stale` under partial hashing. Entries previously and
  wrongly reported `Fresh` are recomputed once.
- **R8:** `AsyncCacheEngine` batch methods return one result per path on failure.
- **R9:** starting a watcher no longer changes the database's journal mode.

All go under `### Fixed` in the 0.21.4 changelog, except R6's documentation of the read-based
policy and its v0.22.0 notice, which go under `### Changed`. Each entry has a sentence on who could
have been affected. This is non-breaking work at its own breaking point, so it ships as a patch.

## Resolved: the two non-RFC handoff directories

RFC 000, as revised on 2026-09-23, requires every `rfcs/handoffs/` directory to correspond to an
RFC number. `rfcs/handoffs/phase-23-p0/` and `rfcs/handoffs/phase-23-p1/` did not. The owner asked
the architect to decide, under the principle of a finally clean design. The decision applies one
rule with no exceptions: **a tracked handoff exists only as the companion of its governing RFC.**

- `phase-23-p1/p1e-release-preparation.md` **moves** to
  `rfcs/handoffs/020-batched-maintenance-deletes/`. It prepared RFC 020's release, and RFC 021's
  `p2d-release-preparation.md` already sets that precedent.
- `phase-23-p0/` (3 files) and `phase-23-p1/p1a-*` (4 files) have no governing RFC, so they
  **leave the tracked tree**. Git history keeps them in full, and `ROADMAP.md` records the commit
  to retrieve them from. A documented exception was rejected: a policy with a standing exception
  in its own index is exactly the "status field that lies" RFC 000 warns against.
- **Going forward**, a milestone with no RFC gets its handoff in `.git-exclude/reviewed/`, not in
  `rfcs/handoffs/`. The rule is recorded under Phase 24 in `ROADMAP.md`.

The architect carries this out when this RFC is filed as Accepted. No implementer action.
