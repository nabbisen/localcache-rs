# RFC 022 — Correctness and Contract Reconciliation (v0.21.4)

| Field | Value |
|---|---|
| Status | Accepted (owner, 2026-09-23); amended the same day with R6 (see "Amendment" below) |
| Feature | *(core; `encryption` for R1; `json` affects R2's tiers)* |
| Touches | `crates/localcache/src/cache/engine.rs`, `crates/localcache/src/cache/query.rs`, `crates/localcache/src/db/repository.rs`, `crates/cli/src/main.rs`, `scripts/release.py`, `scripts/check_advisories.py`, `scripts/release-tools.toml`, `Makefile.toml`, `.github/workflows/docs.yaml`, `README.md`, `CHANGELOG.md`, `docs/src/`, `rfcs/README.md`, `ROADMAP.md` |
| Finding | Architect onboarding review, 2026-09-23 |
| Milestone | Phase 24 Q0 |
| Breaking | **No** — no public signature, schema, or wire-format change; one documented field meaning is corrected (R6.6); targets v0.21.4 |
| Authorship | High-capability model; **reviewed by the owner** (arrangement of 2026-08-01) |

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
- **R6** — `max_entries` eviction can remove the entry the same `set` just wrote.

R1, R2, and R6 are the reason this is a release rather than a docs sweep. They are designed here.
R3–R5 are mechanical, and are specified precisely enough that they need no further design.

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
2. `rotate_encryption_key` keeps `&self` and its signature. It sets the new key **only after
   `tx.commit()` returns `Ok`**. On any error the engine keeps the old key, which still matches the
   rolled-back database.
3. `EngineCore` must not snapshot the key when `query()` is called. It borrows the `Cell` and reads
   the key at decode time, so a `QueryBuilder` built before a rotation and run after it decodes
   with the current key.
4. Every other engine is **not** updated, and cannot be: other processes, other `ConnectionPool`
   instances, `ReadPool` slots, and a watcher's helper connection. The rustdoc of
   `CacheEngine::rotate_encryption_key` and `AsyncCacheEngine::rotate_encryption_key`, and the
   `docs/src/cookbook.md` encryption recipe, must say so: **after rotation, every other open
   engine on this database must be reopened with the new key.** The watcher helper only deletes
   rows and never decodes, but the implementer must verify that rather than assume it.

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
   wrong, not only the comment.
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
- **`rfcs/README.md`** → the architect does this when this RFC is filed; listed here for
  completeness.
- **`ROADMAP.md`** → the architect maintains it; no implementer action.

## R6 — A write is an access; `set` never evicts what it just wrote

> **Amendment, 2026-09-23, after acceptance.** R6 was added when the architect re-reviewed R4.4
> against the owner's principle that APIs must not confuse users. The defect below is **reproduced**
> (scratch probe under `.git-exclude/tmp/lru-probe/`), not inferred. It is within this RFC's theme,
> a correctness fix restoring a documented contract, but it adds scope, so the owner is asked to
> confirm it explicitly.

### Defect

A new entry is written with `last_accessed_at = 0`. An overwrite leaves `last_accessed_at`
unchanged. Eviction deletes in `last_accessed_at ASC, updated_at ASC` order. So once every other
entry has been read, **the entry a `set` just wrote is the first eviction candidate, and that same
`set` evicts it:**

```text
max_entries(2); set(a); set(b); get(a); get(b); set(c)
→ set(c) returns Ok(()), and contains(c) == false
```

The user sees `set` succeed and the next `get` miss. That contradicts:

- `docs/src/builder.md` ("true LRU based on `last_accessed_at`");
- `README.md` ("evicts the least recently accessed entries");
- `CacheOptions::max_entries`' rustdoc.

The existing tests pass only because they never read every surviving entry before a write. They
also depend on a tie at one-second `updated_at` resolution, which SQLite breaks in an unspecified
order.

### Design

1. **A write is an access.** Insert and overwrite (`upsert_in_tx`) both set `last_accessed_at` to the
   current time. That is the same clock and unit `get` uses (Unix seconds).
   `last_accessed_at` then means "last read or write". This is what "least recently used"
   means to every user of an LRU cache.
2. **Deterministic eviction order:** `last_accessed_at ASC, updated_at ASC, id ASC`. The final
   key removes the unspecified same-second tie. `id` is first-insert order, and an overwrite
   keeps its `id`. Apply the same order in `list_lru_n_paths`, so the `on_evict` callback reports
   exactly the rows deleted.
3. **`set` never evicts the entry it just wrote.** Exclude that row from the eviction candidates.
   `max_entries(0)` is the only case where a `set` cannot retain its entry. Decide in the
   implementation review whether to reject it at build time; it is not silently allowed to look
   like a successful write.
4. **`batch_set`** excludes its own entries from eviction, **unless the batch alone exceeds
   `max_entries`**. In that case its earliest items, in input order, are evicted first, and
   `BatchSetReport` must not report them as retained. Specify the exact reporting in the handoff;
   the requirement is that no path is reported stored while absent.
5. **No schema change and no migration.** Rows written before 0.21.4 keep `last_accessed_at = 0`
   until next read or written, so they are evicted first. That is correct, because they are the
   least recently used.
6. **Documented meaning change.** `EntryInfo::last_accessed_at`, `ExportRecord::last_accessed_at`,
   `order_by_last_accessed`, the CLI `list` "never" label, `docs/src/architecture.md`, and
   `docs/src/builder.md` all say "last read or write". `0` then means only "written by an earlier
   version and never read since". That changes a documented meaning, so it goes under
   `### Changed`, and the note must say plainly that **localcache no longer distinguishes "never
   read" from "written"**. Deriving it from `updated_at` is unreliable at one-second resolution,
   and the docs must not suggest it. Callers who need that signal track it themselves.
   **Rejected alternative:** keep `last_accessed_at` as read-only and evict by
   `MAX(last_accessed_at, updated_at)`. It preserves the signal, but no index can serve that
   expression, so every eviction would sort the whole namespace. N4 measured eviction at 1M rows,
   and this would regress it without a schema change to add an expression index.

### Tests

- The reproduction above fails on v0.21.3 and passes after the fix.
- Eviction order holds with same-second ties: no reliance on sleeps or unspecified order.
- The `on_evict` callback paths equal the deleted paths.
- `batch_set` within `max_entries`, and a batch larger than `max_entries`. In both, every path
  reported stored is present.
- Overwriting an entry makes it most-recent.
- An imported `ExportRecord` keeps its exported `last_accessed_at` (unchanged behaviour).

## Non-goals

- Splitting `UnsupportedFeature`, or deciding `PayloadVersionMismatch`'s fate (Phase 24 Q2, breaking).
- Paging `rotate_encryption_key`'s full load of encrypted payloads into memory (registered; RFC
  020's paging would apply).
- Any MSRV or dependency change (Phase 24 Q1).
- `aggregate-ci` evidence-manifest cardinality, re-run output-directory handling, and
  `upload-artifact`/`download-artifact` major versions (registered in `ROADMAP.md`).

## Compatibility and release

No public signature, schema, SQL shape, or wire-format change.

**Two observable behaviour corrections**, both restoring a documented contract:
- After R1, a rotating engine keeps working after the rotation.
- After R2, `offset` pages in tiers 1 and 2 match tier 3 and pre-0.21.3 behaviour when undecodable
  rows exist.

Both go under `### Fixed` in the 0.21.4 changelog, each with a sentence on who could have been
affected. This is non-breaking work at its own breaking point, so it ships as a patch.

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
