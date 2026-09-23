# RFC 022 Implementation Handoff — Correctness and Contract Reconciliation (v0.21.4)

RFC: `rfcs/accepted/022-correctness-and-contract-reconciliation.md` (accepted 2026-09-23, R6
amendment accepted the same day)
Milestones: Phase 24 **Q0a–Q0e**. Q0f, the release, gets its own handoff later.
QA companion: `rfcs/handoffs/022-correctness-and-contract-reconciliation/acceptance-qa-checklist.md`

## 0. What this is

This is a **non-breaking patch**. It fixes three correctness defects that no current test catches,
repairs release tooling, and brings documentation and records back in line with the code.

**No public signature, schema, SQL-shape, wire-format, or dependency change.** If you find that one
is needed, stop and file a design request. Do not work around it.

Two standing principles from the owner govern every judgement call here:

- **"Finally clean, safe and secure, robust and sophisticated design."**
- **Public APIs must not confuse or mislead users.** When a name or doc and the behaviour disagree,
  ask which one a user would believe.

## 1. How to work through the slices

| Slice | RFC | Files (primary) |
|---|---|---|
| **Q0a** key rotation | R1 | `crates/localcache/src/cache/engine.rs`, `crates/localcache/src/cache/query.rs` (`EngineCore` use), tests |
| **Q0b** query `offset` | R2 | `crates/localcache/src/cache/query.rs`, `crates/localcache/src/cache/query/tests.rs`, `crates/localcache/tests/query.rs` |
| **Q0c** LRU recency | R6 | `crates/localcache/src/cache/engine.rs`, `crates/localcache/src/db/repository.rs`, `crates/localcache/tests/codec_lru.rs` |
| **Q0d** release tooling | R3 | `scripts/release.py`, `scripts/check_advisories.py`, `scripts/release-tools.toml`, `scripts/tests/`, `Makefile.toml`, `.github/workflows/docs.yaml` |
| **Q0e** hygiene, docs, records | R4, R5 | `crates/cli/src/main.rs` (+ new `crates/cli/src/main/tests.rs`), `crates/localcache/tests/query.rs`, rustdoc sites, `docs/src/`, `README.md`, `CHANGELOG.md` |

**Order: Q0a → Q0b → Q0c → Q0d → Q0e, one slice at a time.** Each slice is an independent review
point, and the working tree must hold exactly one slice's changes when you file its review request.
Follow the project cadence:

1. implement;
2. file the review request describing the **uncommitted** tree;
3. wait for the review in `.git-exclude/reviewed/`;
4. commit with the message the review gives;
5. start the next slice.

**Never commit before the review lands.** Q0e comes last because it documents the contracts Q0a–Q0c
establish, and Q0d's widened gate must check Q0e's corrected install examples the first time.

**Reproduce first, for Q0a, Q0b, and Q0c.** Write the new tests, run them against the unfixed
code, and **capture the failing output** before you change any production code. That output is
required evidence. A test that passes on v0.21.3 does not prove the defect is fixed.

**CHANGELOG.** Each slice adds its own entries under a `## [Unreleased]` heading at the top of
`CHANGELOG.md` (create it in Q0a). Q0f renames the heading to the version.

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
5. Rustdoc on `CacheEngine::rotate_encryption_key` and `AsyncCacheEngine::rotate_encryption_key`
   must state two things:
   - this engine continues with the new key;
   - **every other open engine on this database** (other processes, other `ConnectionPool`
     instances, `ReadPool` slots, a watcher's helper connection) must be reopened with the new key,
     and until then it returns `EncryptionError` on rotated rows.
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

## 4. Q0c — A write is an access; `set` never evicts what it just wrote (RFC 022 R6)

### The defect (reproduced)

A probe is kept at `.git-exclude/tmp/lru-probe/`. `max_entries(2); set(a); set(b); get(a); get(b);
set(c)` returns `Ok(())`, and then `contains(c) == false`. New rows start at
`last_accessed_at = 0`, so they are the first eviction candidates.

### Implementation

1. **`upsert_in_tx`** (`crates/localcache/src/db/repository.rs`, around line 151):
   - Use **one** `now_secs()` reading for both `updated_at` and `last_accessed_at` on insert.
   - Add `last_accessed_at = excluded.last_accessed_at` to the `ON CONFLICT DO UPDATE` list.
   - Return the row id: `Result<i64, _>`. It is already queried right after the insert.
   - `upsert` passes the id through.
   - Delete the stale comment claiming a reset to 0.
2. **`import_rows` is unchanged.** Imports keep the exported `last_accessed_at` on purpose.
3. **Eviction becomes one selection, then deletion by id**, so `on_evict` reports exactly what was
   deleted by construction, not by two queries agreeing. Replace `delete_lru_n` +
   `list_lru_n_paths` with one repository function:

```rust
/// RFC 022 R6: evict up to `n` least-recently-used rows of `namespace`,
/// never touching `protected` ids. Order: last_accessed_at, updated_at, id.
/// Returns the paths of the rows actually deleted.
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
     500, as `payloads_for_ids` does.
   - `idx_files_lru` is `(namespace, last_accessed_at, updated_at)`, and SQLite appends the rowid,
     so `id ASC` is index-served. **Check with `EXPLAIN QUERY PLAN` and include the plan in the
     review request.**
4. **`enforce_max_entries(&self, protected: &[i64])`**: `set` passes its own id; `batch_set` passes
   every id it wrote. Callbacks run **after** the transaction commits, with the returned paths.
5. **`max_entries == Some(0)` is rejected in `CacheEngine::open`**, before any database work, with
   `LocalFileCacheError::UnsupportedFeature("max_entries must be at least 1".into())`. Every
   constructor routes through `open`: `ConnectionPool::open`, `shared_engine`,
   `AsyncCacheEngine::open`, `ReadPool::open`, and `CacheEngineBuilder::build`/`build_read_pool`.
   One check covers all of them, but test at least `build()`, `ReadPool::open`, and
   `ConnectionPool::open`.
6. **An oversized `batch_set` is rejected before writing.**
   - After preparation (the loop that fills `prepared`), count **distinct** `path_str` values.
   - If `max_entries` is `Some(m)` and that count exceeds `m`, return
     `Err(UnsupportedFeature(format!("batch of {n} distinct entries exceeds max_entries {m}")))`
     and write nothing.
   - This check runs **after** `guard_write()` and preparation, and **before** the transaction.
7. **Docs in code.** These all say "last read or write". `0` now means only "written by a version
   before 0.21.4 and never read since".
   - `EntryInfo::last_accessed_at` and `ExportRecord::last_accessed_at`
     (`crates/localcache/src/cache/entry.rs`, around lines 50 and 106);
   - `QueryBuilder::order_by_last_accessed` (`query.rs`, around line 428);
   - `CacheOptions::max_entries` (`options.rs`, around line 158). Also state the two rejections
     and that the bound is enforced by `set`/`batch_set`, not by imports.
8. **CLI `list`** (`crates/cli/src/commands/read.rs`, around line 30): the `LAST_ACCESS` column
   keeps its values, and `0` still prints `never`. With 1–7 in place, `never` now only appears for
   entries written by an older version and never read, which is accurate.

### Tests (`crates/localcache/tests/codec_lru.rs`, LRU section)

1. **The reproduction**, as a test. **Must fail on v0.21.3.**
2. Same-second determinism: fill, read, and write within one second, with no `sleep`. The eviction
   victim is fully determined by `(last_accessed_at, updated_at, id)`. Assert the exact survivor
   set.
3. Overwriting an existing entry makes it most recent. It survives the next eviction.
4. `on_evict` receives exactly the deleted paths. Assert equality with the set that disappeared.
5. `batch_set` within the bound: none of the batch is evicted, and older rows go instead.
6. `batch_set` over the bound: `Err`, and `entry_count()` is unchanged. Also one case where
   duplicate paths make a nominally oversized batch fit.
7. `max_entries(0)` is rejected through `build()`, `ConnectionPool::open`, and `ReadPool::open`.
8. An imported `ExportRecord` keeps its `last_accessed_at` (unchanged behaviour).
9. **Existing tests** `max_entries_evicts_oldest` and `lru_evicts_least_recently_accessed` must
   still pass **unmodified**. If one fails, stop and report it. Do not edit it to pass.

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
  - `mtime` in nanoseconds; `last_accessed_at` = last read or write (Q0c).
  - `ON CONFLICT … DO UPDATE` rather than `INSERT OR REPLACE`.
  - Decoding is driven by the stored `encoding` tag, and configuration supplies only the key. This
    replaces lines 120–122, which contradict line 48.
  - The eviction order from Q0c.
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
  - The `max_entries` paragraph: true LRU where a write counts as access; the two rejections; not
    enforced on import.
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
  - Rewrite every compare link to the repository's **unprefixed** tag names (`git tag` shows
    `0.19.1`, not `v0.19.1`).
  - Add 0.20.1 through 0.21.3, plus `[Unreleased]: …/compare/0.21.3...HEAD`.
  - The `### Changed` entry for Q0c must say plainly that localcache no longer distinguishes
    "never read" from "written" (RFC 022 R6.6).

---

## 7. Not yours — do not do these

- No MSRV change, no dependency added, removed, or upgraded (Phase 24 Q1). This includes
  `static_assertions` and `zeroize`.
- No renames or deprecations: `order_by_updated_at`, `SortOrder`, CLI `migrate` (Q2).
- No change to which error variant any existing failure returns (Q3). The two new
  `UnsupportedFeature` rejections in Q0c are new failures, not re-homed ones.
- No module splits (Q2b). **Never mix a move with a fix.**
- No performance measurement or tuning. A deep `offset` getting slower in Q0b is the accepted cost.
- No release action: no version bump, tag, or publish. That is Q0f, which is owner-authorized.
- No push, unless the review asks for one to exercise CI.

## 8. Gates, for every slice

- `cargo fmt --all --check` clean
- `cargo make matrix` (`scripts/feature_matrix.py`) green on every row. Clippy is `-D warnings`
  everywhere.
- `cargo make msrv-check` under the 1.85 toolchain, for Q0a–Q0c and Q0e
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
7. **the failing-before output** (Q0a–Q0c) and the passing-after output;
8. gate results, with the commands as run;
9. unresolved issues;
10. known limitations;
11. requested review focus.

Report any judgement call. Do not absorb it silently.
