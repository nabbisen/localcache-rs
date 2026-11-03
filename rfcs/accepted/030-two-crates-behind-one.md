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
2. **Least privilege at compile time.** The code that parses hostile bytes (RFC 028) and seals payloads (RFC 029) shares a crate with SQLite, the async runtimes and the file watcher. In a crate of its own, with no `rusqlite`, no async and no file system, it cannot reach the database, carries its own `#![forbid(unsafe_code)]`, is small enough to audit and fuzz, and is a crate whose every test can run under Miri (Miri cannot execute the bundled C SQLite, so in the present crate the tests that touch the database cannot run under it; whether the codec's own tests already run today is checked in step 0, and if they do, this gain is smaller). This is not a runtime trust boundary and is never described as one.
3. **A visible home for the permanent commitment.** The wire-format guarantee is a book paragraph and a fixture. In `localcache-format` its golden vectors, fixtures and tests sit beside the code they pin.
4. **Features stop multiplying across unrelated code.** `compression`, `json` and `encryption` live with the code they change; `localcache-storage` has none (below). Expected to shorten the 530 s CI feature matrix; **unmeasured**, and measured in step 0.

## Goals and non-goals

Goals: R1 to R6. Non-goals, stated so they stay out: any change of behaviour, of the public API, of a feature name, of MSRV or of the stored format; splitting `error`, `detection`, the query layer or the engine (review 086 § 6); the runtime adapters (R7: a later stage, not decided here); independent versions for the internal crates.

## Design

### R1 — Three crates, one direction

| Crate | Owns | Depends on | Features |
|---|---|---|---|
| `localcache-format` | the payload codec (`payload_codec`), compression, the JSON codec, the tag table, sealing and opening (RFC 029), `Key` and its erasure, `Codec`, the golden vectors | `serde`, and per feature `zstd`, `serde_json`, `aes-gcm`, `zeroize` | `compression`, `json`, `encryption` |
| `localcache-storage` | schema, migration, classifier, configuration, indexes, repository (all statements), `EntryInfo`, `FullRow`, `ExportRecord`'s row form, `DatabaseError` | `rusqlite` | **none**: the repository functions that are gated today (`json` pushdown, `encryption` rotation pages) are always compiled; unused public functions do not warn |
| `localcache` | engine, query, options, builder, glob, freshness detection, the adapters (`pool`, `read_pool`, `async_engine`, `watcher`), the public error enum, the facade | the two above, `blake3`, `base64`, `thiserror`, the runtimes, `notify`, instrumentation | every feature it has today, same names; `compression`, `json` and `encryption` are forwarded to `localcache-format` |

`localcache-format` and `localcache-storage` do not depend on each other. Neither depends on `localcache`. The CLI depends on `localcache` only, as today.

### R2 — Types go to the layer that owns them; errors are per layer

* The four shared types move **down**: `EntryInfo` to storage, `Codec` to format; `CacheStatus` and `ChangeDetectionMode` stay with freshness detection in the facade (it no longer needs `db` to know them). The facade re-exports every public type under its present path with `#[doc(inline)]`, so rustdoc and `use localcache::EntryInfo` are unchanged.
* `FormatError` (serialization, payload too large, unknown encoding, encryption error, missing key, unsealed entry, I/O from the compressor) and `StorageError` (database, incompatible schema, schema not current, database state, invalid configuration) are the internal crates' error types. The facade's `LocalFileCacheError` is unchanged in variants, messages, `#[non_exhaustive]` and feature gating, and is built from the two by `From`. A test pins the mapping **one to one**: every variant of the public enum, its `Display` text and its feature gate are identical before and after (a golden table, generated on the 0.24.0 tree and compared on the new one). No shared `error` crate exists (review 086 § 6).
* Cross-layer values that carry a path or text (`SkippedEntry`, `QueryReport`) stay in the facade.

### R3 — The internal crates have real APIs, not leaked internals

The rule: the public items of an internal crate are a **designed** API (types and functions named for what they do, documented), not the present `pub(crate)` free functions made `pub`. Concretely:
* no `#[doc(hidden)]` item, no `pub mod internals`;
* a public function takes and returns only types defined in its own crate, `std`, or `serde` (so the two internal crates never name each other's types);
* each crate's public API is sketched in step 0 and must fit one rustdoc page; if it does not, that crate stays a module (the stop rule below).

Storage presents a `Store` (the connection plus the statements) with methods, in place of ~28 free functions over `&Connection`; format presents `seal`/`open` over a `Key`, `encode`/`decode` over a codec and a tag, and the tag table.
The crates are published (Cargo requires it) with a `description` and a first README line that say what they are: *"Implementation crate of `localcache`. Depend on `localcache`; this crate's API is not a separate stability promise."* Their versions are **lockstep** with the facade, pinned `=` between them (`version.workspace = true`, as the CLI has). Stated honestly: lockstep means the format crate's version carries no signal of its own; the commitment is enforced by its golden vectors and fixtures, not by its version number.

### R4 — The facade changes nothing a user can see

The public API, the feature names, the re-export paths, the `docs/src` examples, the CLI and MSRV 1.85 are unchanged; `cargo-semver-checks`-style comparison of the rustdoc item declarations against the v0.24.0 tag shows no difference except crate-of-origin in the "source" links. The workspace gains no external dependency.

### R5 — What is enforced, and how it is tested

* The layering by Cargo itself; `#![forbid(unsafe_code)]` in the two new crates and the facade.
* Each crate's features are tested in that crate: `localcache-format` with `--each-feature` (three rows plus all), `localcache-storage` with one row. The facade's rows (the runtimes, `watching`, `tracing`, `metrics`, `opentelemetry`, and the forwarding of the three format features) remain. The per-crate gate rows are added to `gate-features`, `gate-clippy`, `gate-msrv` and the CI jobs.
* Miri over `localcache-format`'s unit tests and a fuzz target for `from_slice` and `open` are added **only** if step 0 shows they run clean in reasonable time; they are not made gates before that.

### R6 — The release pipeline, with the one thing it cannot rehearse

* `cargo publish --workspace --locked` orders the crates by dependency (cargo's overlay registry already carries a workspace dry run). The release notes list **four** `.crate` digests (the guard in `github-release` is made to require exactly four, valid, in the notes' fixed order); `check_versions.py`, the archive and the advisories gate learn the two crates. A half-published state (the internal crates published, the facade not) is harmless: the facade pins them `=` and is the last to publish.
* **What a dry run cannot rehearse:** the first publication of a crate name, and Trusted Publishing for a name that does not exist yet. The names `localcache-format` and `localcache-storage` are free on crates.io (checked 2026-11-04: not found). Step 0 establishes from crates.io's documentation how a trusted-publisher record is created for a not-yet-published crate; the owner creates the two records before the release (the repository and workflow `release.yaml`, as for the existing two). If it is not possible without a first manual publish, the RFC is amended before acceptance, not improvised at the release.

### R7 — The adapters: a second stage, not decided here

Checked in review 086: `async_engine`, `pool`, `read_pool` and `watcher` (18% of the code) use none of the engine's internals; the async layer mirrors the engine's 36 public methods. A cut would isolate `tokio`, `async-std`, `smol` and `notify`, would remove the runtime-priority rule (`async` over `async-std` over `smol`: a rule with an exception, which exists only because features are additive) and four of the ten feature rows; it costs a core crate, adapter crates and a facade (seven or more crates in all). **This RFC does not do it.** Stage 1 (R1 to R6) must first produce the measurements that decide it: the public-surface sketch of the engine as a library (what an adapter crate would call), the per-row CI time with two crates cut, and the publish rehearsal at the larger crate count. The owner decides stage 2 on those numbers, in a short RFC of its own.

## Test plan

* **The proof of "nothing changes":** rustdoc item declarations, method signatures and `impl` headers of the facade identical to the v0.24.0 tag (the method used for every release since 0.22.1), the error-mapping golden table (R2), and every existing test passing; tests that exercise an internal move with it, and the list of what moved is in the review request.
* The golden vectors, the codec differential and the compat fixtures stay byte-for-byte valid and run in the crate that owns them.
* Per-crate feature rows (R5); the cross-version compatibility harness of releases 0.23.1 and 0.24.0 (databases written by the previous release read by the new one and the reverse, all four tags, compressed and not); the measurements of step 0 repeated on the final tree.
* A `cargo package` and workspace `cargo publish --dry-run` of the four crates from a clean checkout.

## Step 0: what the dev team verifies before any code

1. **The public API sketch of each internal crate** (`Store`; `seal`/`open`/`encode`/`decode`), the count of public items, and whether each satisfies R3 (no `#[doc(hidden)]`, no foreign types, one rustdoc page). The mapping from today's `pub(crate)` functions to it.
2. **The per-layer errors** (`FormatError`, `StorageError`): every variant of `LocalFileCacheError` assigned to a layer or to the facade, and the golden table of `Display` texts and gates.
3. **Miri and a fuzzer on the payload codec and sealing**: first, do the codec's and the sealing's unit tests already run under Miri in the present crate (filtered to the tests that never call SQLite)? Then as a scratch crate with the code copied: do they run clean, and how long do they take.
4. **CI time**: the 530 s feature matrix with `compression`, `json` and `encryption` rows moved to a crate that compiles 1,300 lines, on a prototype in a scratch worktree, per-row times, on a stated host and a stated core count (the host stated in review 086 was a 32-core machine; the CI runners are smaller, so measure on one).
5. **The publish rehearsal**: `cargo publish --workspace --dry-run` and `cargo package` for four crates; how a Trusted Publishing record is created for a name not yet published (from the crates.io documentation, with a reference); what the `Release` workflow's digest steps need.
6. **The hit path**: `get` and `set` through the engine on the prototype against the v0.24.0 tag, alternating arms, at least five rounds, `/proc/loadavg` logged (cross-crate calls are not inlined without `#[inline]` or LTO).
7. **Rendering**: that `#[doc(inline)]` re-exports of types from the internal crates render in the facade's rustdoc and docs.rs page as today.
8. **The relocation list**: which of the 24 integration tests, the unit tests and the fixtures move, and which stay.

**Stop rules.** (a) If an internal crate's API cannot be written as R3 requires (it needs a `#[doc(hidden)]` item, a foreign type, or does not fit one page), **that crate stays a module**; the other may still proceed. (b) If the error mapping cannot be one to one, stop. (c) If step 0 shows that items 3 and 4 give nothing (Miri/fuzz impractical and no CI gain), the case rests on the first two motivations only (enforced layering and least privilege); the architect reports that plainly and the owner decides with that stated, because the cost (R6, the visible internal crates, the lockstep versions) is real.

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

## Compatibility and rollback

No API, feature, MSRV or stored-format change: not breaking, so no announcement release is owed. It is a minor release because every user's dependency graph gains two crates. Rollback: a later release folds the two crates back into `localcache`; the published crates stay on crates.io (they can be yanked) and the facade stops depending on them.

## Decisions

Accepted by the owner on 2026-11-04 with the recommended answers:
1. Stage 1 (R1 to R6) as scoped, with the stop rules; stage 2 (R7, the adapters) is a later, separate decision.
2. Lockstep versions, exact pins, and the plain wording of the internal crates' first README line (R3).
3. Release as v0.25.0, after v0.24.0.
4. The names `localcache-format` and `localcache-storage`.

Handoffs: `../handoffs/030-two-crates-behind-one/` (step 0 first; the slices after the architect's review of step 0).

## Open questions

None for the owner. The eight items of step 0 are verifications with stop rules, not design questions.
