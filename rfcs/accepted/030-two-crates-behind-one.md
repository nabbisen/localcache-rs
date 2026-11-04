# RFC 030 — Two Crates Behind One: `localcache-format` and `localcache-storage`

| Field | Value |
|-------|-------|
| Status | Accepted (owner, 2026-11-04), all four decisions as recommended |
| Feature | *(core)* |
| Touches | the workspace layout (`Cargo.toml`, two new crates), `crates/localcache/src/{serialization*,key.rs,db/**,error.rs}` (moved or split), `crates/localcache/tests` (relocated where they test an internal), `Makefile.toml` and `.github/workflows/*.yaml` (per-crate gate rows, the release digest guard), `scripts/check_versions.py`, `docs/src/` (architecture, source archives, security), `deny.toml` |
| Release | **v0.25.0**, after v0.24.0 is released (a minor release: the public API is unchanged, the packaging is not) |
| Scope document | `.git-exclude/reviewed/086-architect-crate-split-question-2026-11-04.md` (the owner accepted the analysis and asked for this RFC, 2026-11-04) |

## Summary

The library becomes three crates with one public face. `localcache-format` owns what is written into a payload and read back (the codec, compression, JSON, sealing, the key). `localcache-storage` owns the one thing that touches the database (schema, migration, classifier, repository). `localcache` keeps the engine, the query layer, the options, the runtime adapters and freshness detection, and re-exports the same public API it has today. The dependency direction is enforced by Cargo, not by convention.

## Motivation

From review 086, stated as the owner's test (*finally clean, safe and secure, robust and sophisticated; less is more; nothing that confuses*), not as build speed (the measured gain there is about a second):

1. **The layering is a convention, and it has already drifted.** Inside one crate `pub(crate)` lets any module reach any other. Four upward references exist (`db` uses `cache::entry::EntryInfo`; `detection` uses `CacheStatus` and `ChangeDetectionMode`; `serialization` uses `Codec`) and no gate noticed. Cargo rejects cyclic crate dependencies: the layering becomes a checked invariant.
2. **Least privilege at compile time.** The code that parses hostile bytes (RFC 028) and seals payloads (RFC 029) shares a crate with SQLite, the async runtimes and the file watcher. In a crate of its own, with no `rusqlite`, no async and no file system, it cannot reach the database, carries its own `#![forbid(unsafe_code)]`, is small enough to audit, and can be fuzzed through its public entry points (in the present crate the codec is `pub(crate)`, so a fuzz target needs a hook). **Step 0 found** that Miri already runs the codec's and sealing's tests in the present crate when filtered to the tests that never call SQLite, so that gain is small. The same boundary, drawn around storage, means the engine, the adapters and the watcher can no longer run SQL: today the facade held four transaction sites (step 0 counted five), eighteen `.db()` calls and the whole connection setup. This is not a runtime trust boundary and is never described as one.
3. **A visible home for the permanent commitment.** The wire-format guarantee is a book paragraph and a fixture. In `localcache-format` its golden vectors, fixtures and tests sit beside the code they pin.
4. **Features and tests stop being run where they do not apply.** `compression`, `json` and `encryption` live with the code they change, and `localcache-storage` has no features. **Step 0 measured the CI claim and it is weaker than this RFC first hoped:** the 530 s feature matrix is test *execution*, not compilation, and the split saves about 60 to 70 s of it (11 to 14%) because storage's 82 tests would run once instead of in every facade row. That gain is available without a split, so it is not a reason for one.

## Goals and non-goals

Goals: R1 to R6. Non-goals, stated so they stay out: any change of behaviour, of the public API, of a feature name, of MSRV or of the stored format; splitting `error`, `detection`, the query layer or the engine (review 086 § 6); the runtime adapters (R7: a later stage, not decided here); independent versions for the internal crates.

## Design

### R1 — Three crates, one direction

| Crate | Owns | Depends on | Features |
|---|---|---|---|
| `localcache-format` | the payload codec (`payload_codec`), compression, the JSON codec, the tag table, sealing and opening (RFC 029), `Key` and its erasure, `Codec`, the golden vectors | `serde`, and per feature `zstd`, `serde_json`, `aes-gcm`, `zeroize` | `compression`, `json`, `encryption` |
| `localcache-storage` | schema, migration, classifier, configuration, indexes, repository (all statements), **the connection open and the write transactions** (`Store::open`, `begin_write`, `vacuum`), `EntryInfo`, `FileMetadata`, `JournalMode`, `SynchronousMode`, the row types, `DatabaseError` | `rusqlite` | **none**: the repository functions that are gated today (`json` pushdown, `encryption` rotation pages) are always compiled; unused public functions do not warn |
| `localcache` | engine, query, options, builder, glob, freshness detection, the adapters (`pool`, `read_pool`, `async_engine`, `watcher`), the public error enum, the facade | the two above, `blake3`, `base64`, `thiserror`, the runtimes, `notify`, instrumentation | every feature it has today, same names; `compression`, `json` and `encryption` are forwarded to `localcache-format` |

`localcache-format` and `localcache-storage` do not depend on each other. Neither depends on `localcache`. The CLI depends on `localcache` only, as today.

### R2 — Types go to the layer that owns them; errors are per layer

* **Five** shared types move **down** (step 0 found five, not four): `EntryInfo`, `FileMetadata`, `JournalMode` and `SynchronousMode` to storage, `Codec` to format; `CacheStatus` and `ChangeDetectionMode` stay with freshness detection in the facade. The facade re-exports every public type under its present path with `#[doc(inline)]` (without it the facade shows a link, not the page), so `use localcache::EntryInfo` is unchanged. The docs of the moved types that linked to facade items (`EntryInfo`, `FileMetadata`, `JournalMode`, `DatabaseError`) become plain code spans: four types lose a link, said in the CHANGELOG.
* `FormatError` (serialization, payload too large, unknown encoding, encryption error, missing key, unsealed entry, I/O from the compressor) and `StorageError` (database, incompatible schema, schema not current, database state, invalid configuration) are the internal crates' error types. The facade's `LocalFileCacheError` is unchanged in variants, messages, `#[non_exhaustive]` and feature gating, and is built from the two by `From`. A test pins the mapping **one to one**: every variant of the public enum, its `Display` text and its feature gate are identical before and after (a golden table, generated on the 0.24.0 tree and compared on the new one). No shared `error` crate exists (review 086 § 6).
* **The conversion is crate-private.** A `From<FormatError>` or `From<StorageError>` for the public enum is public API (rustdoc lists the impl and names the internal type); it is a private trait, as `SqliteResultExt` is today, and each call site (52 in the built slice 2, against the 74 step 0 estimated) changes from `?` to a lifting call. `DatabaseError::from_sqlite` is private for the same reason.
* Cross-layer values that carry a path or text (`SkippedEntry`, `QueryReport`) stay in the facade.

### R3 — The internal crates have real APIs, not leaked internals

The rule: the public items of an internal crate are a **designed** API (types and functions named for what they do, documented), not the present `pub(crate)` free functions made `pub`. Concretely:
* no `#[doc(hidden)]` item, no `pub mod internals`;
* a public function takes and returns only types defined in its own crate, `std`, or `serde` (so the two internal crates never name each other's types);
* each crate's public API is sketched in step 0 and must fit one rustdoc page; if it does not, that crate stays a module (the stop rule below).

Storage presents a `Store` (the connection plus the statements) with methods, in place of ~28 free functions over `&Connection`; format presents `seal`/`open` over a `Key`, `encode`/`decode` over a codec and a tag, and the tag table.
**Test seams do not cross the boundary and are not features.** The use-order clock and the legacy-sealing helper were reached across layers by the engine's tests. Decision: the stuck-clock and backwards-clock variants of the use-order tests move down to storage, where the use order lives (the engine keeps its real-clock variant); the rotation tests build their legacy rows by SQL from the committed v0.23.1 fixture (a legacy-sealed payload opens at any path). No `test-hooks` cargo feature, no public clock.

The crates are published (Cargo requires it) with a `description` and a first README line that say what they are: *"Implementation crate of `localcache`. Depend on `localcache`; this crate's API is not a separate stability promise."* Their versions are **lockstep** with the facade, pinned `=` between them (`version.workspace = true`, as the CLI has). Stated honestly: lockstep means the format crate's version carries no signal of its own; the commitment is enforced by its golden vectors and fixtures, not by its version number.

### R4 — The facade changes nothing a user can see

The public API, the feature names, the re-export paths, the `docs/src` examples, the CLI and MSRV 1.85 are unchanged; `cargo-semver-checks`-style comparison of the rustdoc item declarations against the v0.24.0 tag shows no difference except crate-of-origin in the "source" links. The workspace gains no external dependency.

### R5 — What is enforced, and how it is tested

* The layering by Cargo itself; `#![forbid(unsafe_code)]` in the two new crates and the facade.
* Each crate's features are tested in that crate: `localcache-format` with `--each-feature` (three rows plus all), `localcache-storage` with one row. The facade's rows (the runtimes, `watching`, `tracing`, `metrics`, `opentelemetry`, and the forwarding of the three format features) remain. The per-crate gate rows are added to `gate-features`, `gate-clippy`, `gate-msrv` and the CI jobs.
* Miri over `localcache-format`'s unit tests and a fuzz target for `from_slice` and `open` are added **only** if step 0 shows they run clean in reasonable time; they are not made gates before that.

### R6 — The release pipeline, with the one thing it cannot rehearse

* `cargo publish --workspace --locked` orders the crates by dependency (cargo's overlay registry already carries a workspace dry run). The release notes list **four** `.crate` digests (the guard in `github-release` is made to require exactly four, valid, in the notes' fixed order); `check_versions.py`, the archive and the advisories gate learn the two crates. A half-published state (the internal crates published, the facade not) is harmless: the facade pins them `=` and is the last to publish.
* **What a dry run cannot rehearse: the first publication of a new crate name.** Step 0 (reading the crates.io development updates of 2025-07-11 and 2026-01-21; the documentation page itself could not be fetched) found that Trusted Publishing is set up **after** the first release, which must be published manually, and no "pending publisher" feature. So the `Release` workflow cannot publish the first version of a new name. The owner's steps, before the release that introduces the crates: (1) create an API token with the `publish-new` scope; (2) publish by hand a placeholder `0.0.1` of each of `localcache-format` and `localcache-storage` (a minimal crate whose description says it is reserved for `localcache`), so that the real `0.25.0` is still unpublished; (3) on each crate's settings add the trusted publisher (repository `nabbisen/localcache-rs`, workflow `release.yaml`, **no environment**, exactly as for the two existing crates: the workflow's publish job runs in the `release` Environment, and the records do not name it; the first text of this RFC said otherwise and was wrong); (4) revoke the token; (5) run the release as usual. This is a one-time exception to "no long-lived registry token", for the minutes it exists, and `docs/src/security.md`'s Publishing section says so when it happens. The names are free on crates.io (checked 2026-11-04: not found). Not rehearsable; step 0's dry run of four crates passes.

### R7 — The adapters: a second stage, not decided here

Checked in review 086: `async_engine`, `pool`, `read_pool` and `watcher` (18% of the code) use none of the engine's internals; the async layer mirrors the engine's 36 public methods. A cut would isolate `tokio`, `async-std`, `smol` and `notify`, would remove the runtime-priority rule (`async` over `async-std` over `smol`: a rule with an exception, which exists only because features are additive) and four of the ten feature rows; it costs a core crate, adapter crates and a facade (seven or more crates in all). **This RFC does not do it.** Stage 1 (R1 to R6) must first produce the measurements that decide it: the public-surface sketch of the engine as a library (what an adapter crate would call), the per-row CI time with two crates cut, and the publish rehearsal at the larger crate count. The owner decides stage 2 on those numbers, in a short RFC of its own. Step 0 adds a caution: the feature rows cost test *execution*, so moving the adapters to a crate shortens CI only if their tests move with them.

### R8 — The order of the work, and the two decision points

Step 0 showed that most of the benefit is in work that needs no new crate and is worth doing whether or not any crate is cut. The slices, each building and passing all tests with no change of public API (checked after each by the rustdoc comparison against the v0.24.0 tag):
1. Move the five shared types down inside `localcache` and add a layering test (a test that fails when `db`, `serialization` or `detection` import a higher layer).
2. `FormatError` and `StorageError` inside the crate, the private lift, the golden table as a committed test.
3. A `Store` in front of the repository, inside the crate: the engine, the adapters and the watcher stop using `rusqlite` (the connection open, the four transaction sites and the `.db()` calls move behind it); the test seams of R3 are resolved here. This is the large step.
4. Cut `localcache-format` (mechanical, as the prototype shows).
5. Cut `localcache-storage` (mechanical).
6. The gates, the release pipeline (four digests, the exact-pin check in `check_versions.py`), the book.
7. The owner's first publications and trusted-publisher records (R6).

**Decision point 1, after slice 3:** the architect reports the facts (the size of the `Store`, whether the engine truly no longer names `rusqlite`, the hit path) and recommends; the owner confirms or stops before slices 4 and 5. **Decision point 2, after slice 4,** before storage: format is the case with the strongest independent reason (fuzzable through its public API, no SQLite, a home for the golden vectors); storage's reason is compile-time enforcement of what slice 3 already achieves by design, at the cost of a second manual publication. Slices 1 to 3 ship in whichever release comes next, whatever is decided.


## Test plan

* **The proof of "nothing changes":** rustdoc item declarations, method signatures and `impl` headers of the facade identical to the v0.24.0 tag (the method used for every release since 0.22.1), the error-mapping golden table (R2), and every existing test passing; tests that exercise an internal move with it, and the list of what moved is in the review request.
* The golden vectors, the codec differential and the compat fixtures stay byte-for-byte valid and run in the crate that owns them.
* Per-crate feature rows (R5); the cross-version compatibility harness of releases 0.23.1 and 0.24.0 (databases written by the previous release read by the new one and the reverse, all four tags, compressed and not); the measurements of step 0 repeated on the final tree.
* A `cargo package` and workspace `cargo publish --dry-run` of the four crates from a clean checkout.

## Step 0, done (request 066, 2026-11-04): findings folded into R1 to R7 above and into R8

A prototype split (`localcache-format`, `localcache-storage`, the facade rewired) passes all 720 tests, keeps the 19 error variants byte for byte against a golden table (a negative control fails it), builds and dry-publishes four crates in the order format, storage, facade, CLI, and does not slow the small-payload hit path (|difference| 0.2 µs or less on 64 bytes, no `#[inline]` needed). Both internal API sketches compile with no `#[doc(hidden)]` item and no foreign type (format: 9 types, 8 functions; storage: 21 types, 31 methods on `Store` and 7 on `WriteTx`). Miri runs clean on 33 of the format crate's 38 tests (four need `cfg(miri)` variants, one asserts wall-clock time); two fuzz targets ran ten minutes each, 68 million executions, no crash.
What the RFC had wrong or under-stated, now corrected above: five shared types, not four; the layer errors must reach the public enum through a private lift (about 74 sites), not a public `From`; **the facade also touches SQLite** (four transaction sites, eighteen `.db()` calls, the connection setup), so the real work is the `Store`, not the cut; two test seams crossed the boundary; moved types lose four doc links; the CI gain is a run-once effect available without a split; Miri already runs in the present crate; and a new crate name cannot be published by the workflow.
**The three stop rules:** (a) format: its API is plain, yes; storage: plain on paper, not proved in code (the `Store` is not implemented); (b) the error mapping is one to one: yes, with the private lift; (c) not triggered, but the margin is modest: the case rests mainly on enforced layering and least privilege, which is what the RFC said it would then do.


## Alternatives considered

* **One crate, with a test that asserts the layering** (after moving the four types down). Cheap, no release effect, and it fixes motivation 1 only as a convention checked by a test; motivation 2 (a crate that cannot reach SQLite) is not available. It is the fallback if stop rule (c) triggers, and it is worth doing on its own.
* **A shared `localcache-types` or `localcache-error` kernel.** Rejected: a grab bag every change touches; the lowest layer would mirror the features of the upper ones (review 086 § 6).
* **Independent versions for the internal crates.** Rejected: more rules (when does each bump, which pins move) for a signal the golden vectors already give; the release pipeline would publish a varying set.
* **Splitting the runtime adapters now.** Deferred (R7) until stage 1 supplies the numbers.
* **`pub(crate)` made `pub` function by function.** Rejected: that is the leak R3 forbids.
* **Git or path dependencies instead of publishing.** Not possible for a crate on crates.io.

## Attack on this design (found while drafting, before the owner is asked)

1. *The visible-home benefit shrinks under lockstep versions.* The format crate's version does not signal changes of the format; the vectors and fixtures do. Said in R3, not hidden.
2. *Internal crates on crates.io are a documented exception* to "one rule, no exceptions": users can see and depend on crates the project does not support separately. Kept to two, named plainly, with the first README line above.
3. *`FormatError` and `StorageError` duplicate texts that now live once.* Hence the golden table: the public enum's messages cannot drift.
4. *The first publication of two new names cannot be rehearsed.* R6 makes it a step 0 question and an owner action before the release.
5. *The CI gain is a hypothesis.* Step 0 item 4 tests it; stop rule (c) says what happens if it fails.
6. *A split during RFC 029 would have collided with it.* The RFC waits for v0.24.0; no code is in flight in these modules after it.

## Slices 1 to 3, built (requests 067 and 068, review 091)

Done and gated, local until the next release: the five types moved down; `tests/layering.rs` (a source-reading check; the layers and their allowed edges are in `docs/src/architecture.md`); `FormatError`, `StorageError` and the crate-private lift with the golden table committed; `db::Store` and `WriteTx` (36 methods; the sketch had 38) with the engine, adapters and watcher off `rusqlite` (no production file outside `db` names it, and a test fails if one does). Two edges the sketch did not have, both accepted: `detection` may use `db` (a stored `FileMetadata` is a `db` value) and the module that holds the public error enum sits above `db` and `serialization` (the lift names their errors); three named exceptions let the engine and builder name the adapter types they return.
The public rustdoc is identical to v0.24.0 (74 pages, 0 differing declarations or impls) after each slice; the cross-version harness is identical both ways; the hit path is unchanged at 64 B (every difference 0.08 µs or less).

## Compatibility and rollback

No API, feature, MSRV or stored-format change: not breaking, so no announcement release is owed. It is a minor release because every user's dependency graph gains two crates. Rollback: a later release folds the two crates back into `localcache`; the published crates stay on crates.io (they can be yanked) and the facade stops depending on them.

## Decisions

Accepted by the owner on 2026-11-04 with the recommended answers:
1. Stage 1 (R1 to R6) as scoped, with the stop rules; stage 2 (R7, the adapters) is a later, separate decision.
2. Lockstep versions, exact pins, and the plain wording of the internal crates' first README line (R3).
3. Release as v0.25.0, after v0.24.0.
4. The names `localcache-format` and `localcache-storage`.

**Revised 2026-11-04 after step 0, for the owner's confirmation (R8):** slices 1 to 3 proceed now under this acceptance; the cuts (slices 4 and 5) are confirmed at the two decision points, with the then-measured facts. Decision 1 above stands as the goal; what changes is that the owner decides each cut when its cost is known.

Handoffs: `../handoffs/030-two-crates-behind-one/` (step 0 first; the slices after the architect's review of step 0). Step 0 is done (request 066); the slices are handed off in `q17-…`.

**Decision point 1, 2026-11-04 (review 091):** the owner answered **yes to slice 4, the format crate**, on the architect's recommendation; storage (slice 5) is left to decision point 2 and is expected to stay a module. Slice 4 and the gates, pipeline and book (slice 6) are handed off in `q18-the-format-crate.md`; the release is v0.25.0 with three published crates (`localcache-format`, `localcache`, `localcache-cli`) and one new name to publish by hand (R6).

## Open questions

None for the owner. The eight items of step 0 are verifications with stop rules, not design questions.
