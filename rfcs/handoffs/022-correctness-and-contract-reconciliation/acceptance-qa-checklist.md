# RFC 022 Acceptance & QA Checklist — Q0a–Q0e, Q0g–Q0i

Companion to `rfcs/handoffs/022-correctness-and-contract-reconciliation/implementation-handoff.md`.
This is what each slice's review checks. A slice is accepted only when every box in its own
section and in **G** holds.

## A. Q0a — Key rotation (R1)

- [ ] The failing-before output is attached for same-engine read, same-engine write, and query across rotation
- [ ] `encryption_key` is a `Cell<Option<[u8; 32]>>`. There is no other interior-mutability primitive, no `Mutex`, and no `unsafe`
- [ ] The new key is set **only after** `tx.commit()` succeeds. A failed rotation leaves the old key in place, and a test shows it
- [ ] `EngineCore` reads the key at decode time, not at `query()`
- [ ] Rotation works through `CacheEngine`, `ConnectionPool`, and `AsyncCacheEngine`, on every async backend the suite runs
- [ ] The auto traits (`Send` / `Sync` / `UnwindSafe` / `RefUnwindSafe`) observed on v0.21.3 are stated, and are asserted unchanged with no new dependency
- [ ] Rustdoc on both `rotate_encryption_key` methods states the reopen-other-engines contract
- [ ] It is confirmed, with the method stated, that the watcher helper never decodes payloads
- [ ] The `rotate_encryption_key` signatures are unchanged
- [ ] *(Amendment 2)* `Ok` always means the engine switched keys, including zero entries re-encrypted; a test with failing-before output shows it
- [ ] *(Amendment 2)* Rotation opens an `IMMEDIATE` transaction before loading and holds it to commit; the hook test shows a concurrent write is refused or preserved, with failing-before output
- [ ] *(Amendment 2)* Both rustdocs state the namespace scope: reopen engines on the same database **and namespace**; other namespaces keep their key; watchers need no action; `AsyncCacheEngine` clones share the engine

## B. Q0b — Query `offset` (R2)

- [ ] The failing-before output is attached for `[bad, a, b, c, d]`: pages overlap on v0.21.3 and are disjoint after the fix
- [ ] Tier 1/2 pages equal tier 3 pages on the same data (tier 3 forced by one non-`json` row)
- [ ] Bad rows at the start, in the middle, at the end, and every row bad, each with `offset > 0`
- [ ] An orphan (no payload row) behaves exactly like a corrupt payload
- [ ] `offset` beyond the materializable count returns an empty `Vec`, not `Err`
- [ ] Decode count ≤ `offset + limit + bad rows encountered`. The existing decode-count test passes **unmodified**
- [ ] An ungated integration test in `crates/localcache/tests/query.rs` covers `limit`, `offset`, and a primary `order_by_updated_at` with no features, making its bad row via `rusqlite` and not via a type-mismatched decode
- [ ] `execute_tier3` is unchanged
- [ ] `materialize`'s doc comment no longer calls positional offset "today's behaviour"

## C. Q0c — LRU eviction (R6, re-scoped by Amendment 2)

- [ ] The failing-before output is attached for the reproduction (`set(c)` returns `Ok`, then `contains(c) == false`) and for the batch-within-bound case
- [ ] `last_accessed_at` write behaviour is **unchanged** (`0` on insert, kept on overwrite); the stale "reset to 0" comment is corrected
- [ ] `import_rows` still preserves the exported `last_accessed_at`
- [ ] Eviction is one selection plus deletion by id in one transaction, ordered `last_accessed_at, updated_at, id`, excluding the protected ids
- [ ] `EXPLAIN QUERY PLAN` for the eviction selection is attached and shows `idx_files_lru` with no temporary sort
- [ ] One `pub(crate)` 500-id chunk constant in `repository.rs` is used by `payloads_for_ids`, `evict_lru`, and `materialize`; no local copies remain
- [ ] `on_evict` receives exactly the deleted paths, after commit
- [ ] `set` never evicts its own row. `batch_set` never evicts any row it wrote
- [ ] **Nothing new returns an error**: `max_entries(0)` keeps only the latest write; an oversized `batch_set` stores all its entries and the next `set` restores the bound — each with a test
- [ ] The same-second eviction test uses no `sleep` and asserts the exact survivors
- [ ] `max_entries_evicts_oldest`, `lru_evicts_least_recently_accessed`, `touch_protects_from_lru_eviction`, the `on_evict_*` tests, and the read-only `max_entries(0)` case pass **unmodified**
- [ ] The only edited existing test is `batch_set_respects_max_entries`, and its diff is in the review request
- [ ] Rustdoc for `EntryInfo`, `ExportRecord`, `order_by_last_accessed`, `CacheOptions::max_entries`, and `CacheEngineBuilder::max_entries` states the read-based policy, the two consequences with their v0.22.0 rejection notice, and the import exemption

## C2. Q0g — Size change is conclusive (R7)

- [ ] The failing-before output is attached: a grown file with unchanged head and tail is `Fresh` on v0.21.3
- [ ] Both metadata-then-hash modes return `Stale` on a size change **before** hashing; `StrictFullHash` and `explain()` are unchanged
- [ ] Unchanged files stay `Fresh` in both modes
- [ ] `ChangeDetectionMode::MetadataThenPartialHash`'s rustdoc states what it does and does not detect

## C3. Q0h — Async batch results (R8)

- [ ] The failing-before output is attached: 1 result for 3 paths on a poisoned engine
- [ ] All three batch methods return exactly one result per path for poisoning (`Poisoned { resource: "AsyncCacheEngine" }`) and for a panicking task (`AsyncTaskPanicked`), on every async backend the suite runs
- [ ] Lock handling is inside the blocking closure; the `AsyncTaskPanicked`-only invariant is stated in a comment and a `debug_assert!`; one helper, not three loops
- [ ] Each method's rustdoc states the one-per-path guarantee

## C4. Q0i — Watcher helper configuration (R9)

- [ ] The failing-before output is attached: a `Delete`-mode database reads `wal` after `watcher()` on v0.21.3
- [ ] After `watcher()` and after `debounced_watcher()`, the file's journal mode is unchanged
- [ ] One function builds the helper options; the helper holds no encryption key and no compression setting; `watcher()` opens exactly one helper connection
- [ ] The debounced callback sends no notification when it could not take the lock; the review request says this path is verified by review only
- [ ] No public signature changed (`new_with_paths` is `pub(crate)`)

## D. Q0d — Release tooling (R3)

- [ ] The version gate scans `README.md` plus every `docs/src/**/*.md` by glob, not a hand list
- [ ] Both declaration forms and column-aligned whitespace match; an unparseable declaration fails; prose is excluded — each with a test
- [ ] Evidence shows the widened gate **failing** on the five `"0.19"` examples before Q0e fixes them
- [ ] The four Makefile publish tasks and their header lines are removed; the header states `cargo make release`, then a manual `cargo publish --workspace --locked`; `git grep` shows no remaining reference
- [ ] `HTTPError` is caught before `OSError` in `live_fetch`, and its body read is bounded
- [ ] A local-server test drives `live_fetch` itself: 404 means one attempt and a failure, 503 means retried
- [ ] `docs.yaml` grants write scopes to `deploy` only; mdBook 0.5.4 is pinned and SHA-256-checked; every action SHA pin is kept; "verified at first push" is flagged
- [ ] A finding on the artifact actions' runtime retirement is reported with its source. The actions are not upgraded
- [ ] Every changed script is re-pinned in `scripts/release-tools.toml`, and the hash check passes
- [ ] Script tests pass normally and under the restricted `PATH` with an empty `CARGO_HOME`

## E. Q0e — Hygiene, docs, records (R4, R5)

- [ ] The CLI test module has moved to `crates/cli/src/main/tests.rs` as a pure move, with the same test count
- [ ] `isatty` FFI is replaced by `std::io::IsTerminal`; `git grep -n unsafe -- crates` shows comments only
- [ ] `contains_returns_true_for_cached_entry` runs with no features
- [ ] `PayloadVersionMismatch` is documented as reserved; `docs/src/errors.md` is corrected
- [ ] The `index_hint` summary line, the `path_filter_clauses` comment, the duplicate banner, and the `CacheEngine::query` rustdoc are fixed
- [ ] Every R5 documentation item is done. The review will spot-check each against the code, not against the handoff's wording
- [ ] Code examples in `docs/src` that were touched compile in principle: no `!Sync` sharing, and correct arities
- [ ] CLI docs describe current behaviour; **no CLI behaviour changed**
- [ ] Every CHANGELOG compare link uses unprefixed tags that `git tag` lists; 0.20.1–0.21.3 and `[Unreleased]` are present
- [ ] The CHANGELOG `[Unreleased]` section has entries for every slice, and Q0c's `### Changed` entry states the read-based policy, the oversized-batch behaviour, and the v0.22.0 rejections in plain words
- [ ] Every RFC 022 § R5 item marked *(Amendment 2)* is done
- [ ] Q0d's version gate passes

## F. Scope discipline (every slice)

- [ ] Only the slice's files changed (`git status --porcelain`)
- [ ] No public signature, schema, SQL-shape, wire-format, dependency, or MSRV change, and no new error for input v0.21.3 accepted
- [ ] No rename, deprecation, or error re-homing (Q2/Q3); no module split (Q2b); no move mixed with a fix
- [ ] No measurement, tuning, version bump, tag, publish, or unrequested push
- [ ] Existing tests are unmodified. Any that genuinely had to change is flagged and explained, not silently edited

## G. Gates and reporting (every slice)

- [ ] `cargo fmt --all --check` clean
- [ ] `cargo make matrix` green on every row (clippy `-D warnings`)
- [ ] `cargo make msrv-check` green under 1.85 (every code slice and Q0e), every attempt reported
- [ ] `python3 scripts/source_integrity.py --require-tracked` OK
- [ ] The full suite count is reported as observed, with the command
- [ ] The review request is filed as `.git-exclude/review-request/NNN-dev-q0X-<topic>-<date>.md`, naming this handoff and the slice
- [ ] Nothing is committed before the review lands
- [ ] Judgement calls and behaviour differences are **reported, not absorbed**

## What will not count against you

- Finding that the handoff is wrong about a line number, a file, or a mechanism, and saying so.
- Stopping at a design question instead of guessing: a signature, a variant, or a behaviour the
  RFC does not settle.
- Reporting that an existing test must change, with the reason.
- A deep `offset` getting slower after Q0b. The RFC accepts that cost.
