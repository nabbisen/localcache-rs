# RFC 024 Acceptance & QA Checklist — Part A (Q2c, Q2a, Q2d, Q2e)

Companion to `rfcs/handoffs/024-api-consistency/implementation-handoff.md`. This is what each
slice's review checks. A slice is accepted only when every box in its own section and in **E**
holds.

## A. Q2c — Names and deprecations (R4–R8)

- [ ] `SortKey` is public, `#[non_exhaustive]`, and has `Field` (`json` only), `Mtime`, `LastAccessed`, and `Path`. There is **no** `updated_at` variant
- [ ] `order_by(SortKey, SortOrder)` and `then_by(SortKey, SortOrder)` exist, and `SortKey` is re-exported from the crate root
- [ ] All eight bool sort methods are `#[deprecated(since = "0.21.5")]`. Each note names the exact replacement, and each is a one-line delegation
- [ ] The private `OrderBy::UpdatedAt` is renamed `Mtime`
- [ ] The equivalence tests (every key, both directions, ties included) pass. The existing ordering tests use the new API with unchanged expectations
- [ ] `SyncCacheEngine` replaces `ConnectionPool`. The deprecated alias compiles and works. The module doc no longer calls it a pool
- [ ] `SharedEngine` and `shared_engine` are deprecated, and `SharedEngine`'s doc is corrected
- [ ] The `Poisoned { resource: "ConnectionPool" }` string is **unchanged**, with a comment at each site
- [ ] The example is renamed away from `connection_pool`, and its `[[example]]` entry updated
- [ ] `namespace_copy` is deprecated, and the nonexistent parameters are gone from its doc. `import_from`'s rustdoc carries the cross-database example
- [ ] `CacheWatcher::entry_count()` returns `Err(Poisoned)` on a poisoned lock, where `watched_count()` returns `0` (failing-before shown). `watched_count` is deprecated
- [ ] The path-index API (engine trio, `index_hint`, async trio) is deprecated with unchanged behaviour. The legacy-index tests pass under `#[allow(deprecated)]`
- [ ] `docs/src/querying.md` and `docs/src/api.md` no longer present path indexes as a performance tool
- [ ] The migration table is in `docs/src/api.md`
- [ ] The deprecated-name `git grep` over `docs/src/`, `README.md`, `examples/`, and `benches/` shows only the migration table and the notes. The command and output are shown
- [ ] `#[allow(deprecated)]` appears only on tests whose subject is the deprecated item, on the narrowest item
- [ ] CHANGELOG has `### Added` (the new items) and `### Deprecated` (one line per item, with its replacement)

## B. Q2a — Wrappers and the parity test (R1–R3)

- [ ] `crates/localcache/tests/api_surface.rs` scans only the named types' inherent `impl` blocks, and skips `#[deprecated]` engine methods
- [ ] The manifest reasons are only the four permitted strings, plus `not Send (RFC 024 R2)` if established. Deprecated engine methods are skipped, not listed
- [ ] It fails on a missing delegation and on a stale entry, listing all violations at once. Both are demonstrated, then reverted
- [ ] The failing-before output lists exactly the Motivation table's gaps, minus the deprecated items
- [ ] Every R2 delegation is added. `AsyncCacheEngine::import_from` is a manifest entry, with the route documented in the module doc
- [ ] The async watchers' `Send`-ness is established by compile-time assertion, and they are delegated or recorded accordingly
- [ ] `AsyncCacheEngine::preload`'s `Send + 'static` factory bound is documented
- [ ] There is one integration test per delegation. The async ones run on every async backend the suite runs. Rotation through `SyncCacheEngine` uses the engine's assertions

## C. Q2d — `run_report` (R9)

- [ ] `QueryReport<T>` and `SkippedEntry` are public, `#[non_exhaustive]`, and re-exported
- [ ] `run()` and `run_report()` share one execution path; `run()` discards `skipped`. There is no duplicated materialization loop
- [ ] `skipped` lists every undecodable entry passed in scan order, including within the `offset` region. `offset`/`limit` still count returned entries only
- [ ] Wrong-key failing-before: `run()` returns `Ok(vec![])`. After: `run_report()` shows every row in `skipped`, and `run()` is unchanged
- [ ] With mixed good and bad rows and `offset`/`limit`, `run_report().entries` equals `run()`
- [ ] `query_run_report` is on all three wrappers
- [ ] `docs/src/querying.md` covers `run_report`, and announces B1 for v0.22.0

## D. Q2e — CLI (R10–R12)

- [ ] `copy --from-db` works across databases. Without it, `copy` is unchanged
- [ ] The source opens read-only. An old-schema source fails with the library message plus the `--upgrade-source` hint, exits non-zero, and its SHA-256 is unchanged
- [ ] `--upgrade-source` permits the upgrade and succeeds
- [ ] `migrate`'s behaviour is unchanged. It prints the deprecation warning first, and `--help` says the same
- [ ] The CLI test fixtures are inside `crates/cli/`, with provenance recorded. There is no `../localcache/` reference
- [ ] `atty_check` uses `IsTerminal` on all platforms, and there is one `NO_COLOR` helper used at both sites
- [ ] `NO_COLOR` unit tests cover unset, empty, and non-empty
- [ ] `doc = false` is on the bin target, with the collision warning shown before and absent after
- [ ] `crates/cli/README.md` is updated. CHANGELOG has `### Added`, `### Deprecated` (`migrate`), and `### Changed` (colour)

## E. Gates and reporting (every slice)

- [ ] `cargo fmt --all --check` clean
- [ ] `feature_matrix.py --run-all` green (clippy `-D warnings`)
- [ ] `cargo make msrv-check` green under 1.85, with every attempt reported
- [ ] The full suite count is reported as observed
- [ ] `RUSTDOCFLAGS="-D warnings" cargo doc -p localcache --no-deps --all-features --locked` clean
- [ ] `mdbook build docs` succeeds
- [ ] `source_integrity.py --require-tracked` OK
- [ ] **No removal, no behaviour change (except R11), no error-variant change, no MSRV or dependency change, no file move**
- [ ] The review request is `.git-exclude/review-request/NNN-dev-q2X-<topic>-<date>.md`, and names this handoff and the slice
- [ ] Every required step is listed, including the ones that pass
- [ ] Nothing is committed before the review lands
- [ ] Judgement calls are **reported, not absorbed**

## What will not count against you

- Finding that this handoff is wrong about a file, a line, a bound, or a mechanism, and saying so.
  The Q1a review is the precedent: evidence beats the handoff.
- Establishing that the async watchers are not `Send`, and recording them instead of forcing it.
- Stopping at a design question instead of guessing.
