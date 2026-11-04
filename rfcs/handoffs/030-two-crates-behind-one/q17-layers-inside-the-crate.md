# Q17 Handoff — The Layers, Inside the Crate: Types Down, Layer Errors, a `Store` (RFC 030 slices 1 to 3)

RFC: `rfcs/accepted/030-two-crates-behind-one.md` (R2, R3, R8; read R8 first). Step 0: `rfcs/handoffs/030-two-crates-behind-one/step0-verification.md` and review `.git-exclude/reviewed/089-architect-rfc030-step0-review-2026-11-04.md`. Your sketches under `.git-exclude/tmp/rfc030/sketch/` and the golden-table test of the prototype are the starting points.
Format of the review request: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6. **Local commits; do not push.** Two review requests (after slice 2, after slice 3).
Authorship: high-capability model. Authorized: the owner (RFC 030; slices 1 to 3 proceed now, the cuts are decided later).

## 0. What this is, and the one rule

Three refactors inside `crates/localcache`, with **no new crate, no change of public API, behaviour, feature, stored byte, MSRV or error text**. The rule: after **each** slice every test passes unmodified (except the moves named below), and the public rustdoc compared with the v0.24.0 tag (the HTML method of the last releases) shows no difference but doc text. Start from the v0.24.0 candidate or its tag, whichever exists; a rebase onto documentation commits of mine is expected.
This touches the engine, the repository and the error type heavily; it is the largest change since Q12. Take it in small commits, run the whole suite per commit, and stop and ask if a behaviour changes.

## 1. Slice 1 — the five types go down, and a layering test

1. Move `EntryInfo`, `FileMetadata`, `JournalMode`, `SynchronousMode` into `db` (the storage side) and `Codec` (with `DEFAULT_MAX_DECOMPRESSED_BYTES` if it belongs with it) into `serialization`; `CacheStatus` and `ChangeDetectionMode` move into `detection` (they are freshness types), so that `detection` imports nothing from `cache`. The crate root re-exports every public one exactly as today (`pub use`; the public paths do not change). Docs of moved types that link to facade items become plain code spans only if rustdoc `-D warnings` requires it **in the in-crate form** (it should not yet; this matters at the cut).
2. **A layering test** (`tests/layering.rs`, or a unit test that reads `src/`): a table of allowed imports, failing with a readable message when a lower layer names a higher one. Layers, lowest first: `error`, `key`, `path` → `db`, `serialization`, `detection` → `cache` (engine, query, options, builder) → `pool`, `read_pool`, `cache::async_engine`, `cache::watcher`. `db` may use only `error` (and `std`/`rusqlite`/`serde`); `serialization` only `error` and `key`; `detection` only `error`. State the table in the test's header comment and in `docs/src/architecture.md` (the section on modules), and show the test failing on a deliberate violation (then remove it).
3. Report: the list of moved items, the rustdoc comparison, and the test.

## 2. Slice 2 — `FormatError`, `StorageError`, the private lift, the golden table

1. `FormatError` (7 variants: `Io`, `Serialization`, `UnknownEncoding`, `PayloadTooLarge`, `EncryptionError`, `MissingEncryptionKey`, `UnsealedEntry`, feature-gated as the public ones are) in `serialization`; `StorageError` (5: `Database`, `SchemaNotCurrent`, `IncompatibleSchema`, `DatabaseState`, `InvalidArgument` for `index_name` and `records`) in `db`; `DatabaseError` moves with storage. They are `pub(crate)` for now (they become public only at the cut, by the RFC's rules).
2. **The lift is a crate-private trait**, like today's `SqliteResultExt`; **no `From` impl on the public enum** (step 0 C2: it would be public API and rustdoc would list it). About 74 call sites change from `?` to the lifting call. `DatabaseError::from_sqlite` is `pub(crate)`.
3. Where a variant is produced by two layers (`Io`, `Serialization`, `MissingEncryptionKey`, `InvalidArgument`), the layer returns its own and the facade produces its own; the public variant, its payload and its `Display` do not change.
4. **The golden table as a committed test:** the 19-row table (variant, feature gate, `Display`) you generated on the base tree is committed as expected text in the test (`error/golden.rs`), the exhaustive `match` with no wildcard that makes a new variant a compile error, and the second test that builds every layer variant and checks the public variant after the lift. Show the negative control (change one text, see it fail, revert).
5. Report after this slice (first review request): slices 1 and 2, the call-site count, the golden table, the rustdoc comparison (it must show **no** new `impl From`), the gates.

## 3. Slice 3 — a `Store` in front of the repository; the engine stops using `rusqlite`

This is the large step and the one that makes the storage boundary real. Implement the `Store` of your sketch inside `db` (`StoreConfig`, `Location`, `Store::open`, `begin_write`/`WriteTx`, `vacuum`, and the methods of the mapping table in request 066 § 1), then change the engine, the adapters, the watcher and the CLI-facing paths to use it.
* **The condition that defines done:** `git grep -n "rusqlite" -- crates/localcache/src ':!crates/localcache/src/db'` prints nothing except what a test justifies (list each hit), and the layering test of slice 1 gets the line "above `db`, no `rusqlite`". The five `Transaction::new_unchecked(.., Immediate)` sites, the eighteen `.db()` calls, the `VACUUM` and the connection open (flags, shared-cache URIs, pragmas, about 100 lines of `engine.rs`) are behind the `Store`.
* **Behaviour is not allowed to change:** the open order and the side effects of RFC 025 Amendment 1 (a rejected configuration has no effect), read-only opens, `JournalMode::Preserve`, the busy timeout, the transaction behaviours (which `IMMEDIATE`, which deferred), the failure-injection hooks the engine tests use (they stay in the engine and fire at the same points), the eviction callback after commit. Every existing test passes; the cross-version harness of the last two releases is rerun at the end (databases written by v0.24.0 read by this tree and the reverse, all four tags, compressed and not, plus a v1, v4 and v5 fixture migrated).
* **Test seams (RFC 030 R3):** no `test-hooks` feature and no public clock. The stuck-clock and backwards-clock variants of the use-order tests move down to `db` tests of the `Store` (where the use order lives; `test_clock` stays private to `db`); the engine keeps its real-clock variant. The rotation tests build their legacy-sealed rows by SQL from the committed v0.23.1 fixture instead of a helper. The two engine assertions in `indexes/tests.rs` that use `engine.query()` stay as facade tests; the other eight index tests become `db` tests over a bare connection.
* **Measure** the hit path and `set` (64 B and 1 MiB, with and without a key, alternating arms against the v0.24.0 tree, at least seven rounds, `/proc/loadavg` logged): the `Store` adds a layer inside one crate, so inlining is unchanged, but show it.
* The 1 MiB `get` effect you found (the baseline with `codegen-units=1` takes 490 µs against about 900 µs at 16): do **not** act on it here. Record it in the request as an observation; the architect registers it.
* Report after this slice (second review request): the `Store` as built against the sketch (what changed and why), the grep result, the list of tests that moved, the hit-path table, the cross-version harness, the gates.

## 4. Gates

All gates of Q15 § 3, plus `gate-docs` and `gate-archive` on the committed tree, per slice on the last commit of the slice and per commit where it is cheap. Rerun the rustdoc comparison after each slice.

## 5. Not yours

The two new crates and the cut (slices 4 and 5, decided by the owner after slice 3), the release pipeline and gate changes (slice 6), anything about publication (slice 7), the version, `ROADMAP.md`, `docs/src/roadmap.md`.
