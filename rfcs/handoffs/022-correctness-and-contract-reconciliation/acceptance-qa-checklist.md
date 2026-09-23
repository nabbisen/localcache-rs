# RFC 022 Acceptance & QA Checklist — Q0a–Q0e

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

## C. Q0c — LRU recency (R6)

- [ ] The failing-before output is attached for the reproduction (`set(c)` returns `Ok`, then `contains(c) == false`)
- [ ] Insert and overwrite both set `last_accessed_at` from **the same** `now_secs()` reading as `updated_at`
- [ ] `import_rows` still preserves the exported `last_accessed_at`
- [ ] Eviction is one selection plus deletion by id in one transaction, ordered `last_accessed_at, updated_at, id`
- [ ] `EXPLAIN QUERY PLAN` for the eviction selection is attached and shows `idx_files_lru`
- [ ] `on_evict` receives exactly the deleted paths, after commit
- [ ] `set` never evicts its own row. `batch_set` never evicts its own rows
- [ ] `max_entries(0)` is rejected at `open`, tested through `build()`, `ConnectionPool::open`, and `ReadPool::open`
- [ ] An oversized `batch_set` (**distinct** prepared paths > `max_entries`) returns `Err` and writes nothing. A duplicate-path batch that fits is accepted
- [ ] The same-second eviction test uses no `sleep` and asserts the exact survivors
- [ ] `max_entries_evicts_oldest` and `lru_evicts_least_recently_accessed` pass **unmodified**
- [ ] Rustdoc for `EntryInfo`, `ExportRecord`, `order_by_last_accessed`, and `CacheOptions::max_entries` says "last read or write" and states the rejections and the import exemption

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
- [ ] The CHANGELOG `[Unreleased]` section has entries for Q0a–Q0e, and the R6.6 meaning change is under `### Changed` in plain words
- [ ] Q0d's version gate passes

## F. Scope discipline (every slice)

- [ ] Only the slice's files changed (`git status --porcelain`)
- [ ] No public signature, schema, SQL-shape, wire-format, dependency, or MSRV change
- [ ] No rename, deprecation, or error re-homing (Q2/Q3); no module split (Q2b); no move mixed with a fix
- [ ] No measurement, tuning, version bump, tag, publish, or unrequested push
- [ ] Existing tests are unmodified. Any that genuinely had to change is flagged and explained, not silently edited

## G. Gates and reporting (every slice)

- [ ] `cargo fmt --all --check` clean
- [ ] `cargo make matrix` green on every row (clippy `-D warnings`)
- [ ] `cargo make msrv-check` green under 1.85 (Q0a–Q0c, Q0e)
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
