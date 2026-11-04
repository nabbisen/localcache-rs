# Q18 Handoff — The Format Crate: `localcache-format` (RFC 030 slices 4 and 6)

RFC: `rfcs/accepted/030-two-crates-behind-one.md` (R1, R2, R3, R5, R6, R8; decision point 1 answered yes by the owner on 2026-11-04: format yes, storage deferred to decision point 2). Background: `.git-exclude/reviewed/089-architect-rfc030-step0-review-2026-11-04.md`, `.git-exclude/reviewed/091-architect-rfc030-slices-1-3-review-and-decision-point-1-2026-11-04.md`, and your request 066 (the sketch under `.git-exclude/tmp/rfc030/sketch/format/`, the prototype, the publish rehearsal).
Format of the review request: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6. **Local commits; do not push** (the architect pushes after review). Two review requests: after the cut (§ 1 to § 3), after the gates, pipeline and book (§ 4 to § 6).
Authorship: high-capability model. Authorized: the owner.

## 0. What this is, and the rules

One new workspace crate, `crates/localcache-format`, that owns the payload format: the codec, compression, JSON, the tag table, sealing and opening, `Key`, `Codec`, `FormatError`. The facade `localcache` depends on it. Storage is **not** cut (decision point 2 comes later): `db` stays in `localcache`.
The rules of slices 1 to 3 hold: **no change of public API, behaviour, feature name, stored byte, MSRV or error text**; after each commit every test passes and the public rustdoc compared with the v0.24.0 tag shows no difference except what § 2.4 names. The crate's public API is **designed** (RFC 030 R3): no `#[doc(hidden)]` item, no function that takes a type of the facade, nothing that names `rusqlite`.
Start from `origin/main` (`9a49701`, slices 1 to 3).

## 1. The cut (slice 4), in commits that each build and pass

1. **`chore: the localcache-format crate, empty`**: `crates/localcache-format/Cargo.toml` (`name = "localcache-format"`, `version.workspace = true`, `edition.workspace`, `rust-version.workspace`, `license.workspace`, `authors.workspace`, `repository.workspace`, a `description` and a `readme = "README.md"` of its own), `src/lib.rs` with `#![forbid(unsafe_code)]` and `#![deny(missing_docs)]`, `[package.metadata.docs.rs] all-features = true`, the three features `compression`, `json`, `encryption` (dependencies `serde`, optional `zstd`, `serde_json`, `aes-gcm`, `zeroize`; no others), the workspace member and a `[workspace.dependencies]` entry `localcache-format = { version = "=0.24.0", path = "crates/localcache-format" }` (**exact pin**; the release preparation moves it with the workspace version, § 5).
   **The wording of the first README line is fixed by the RFC (R3) and the owner accepted it:** *"Implementation crate of `localcache`. Depend on `localcache`; this crate's API is not a separate stability promise."* The `description` says the same in one sentence. Say in the README what the crate holds (the payload format and its commitment: the golden vectors), the one command each for Miri and the fuzz targets (§ 3), and nothing else.
2. **`refactor: move the payload format into localcache-format`** (the mechanical move, as your prototype did): `serialization/**` (payload codec, compression, JSON, seal, tags), `key.rs`, `Codec`, `DEFAULT_MAX_DECOMPRESSED_BYTES`, `FormatError`. The designed API follows your sketch (`PayloadFormat::encode/decode`, `seal_state`, `reseal`, `Key`, `ErasedVec`, `erase`, `Encoded`, `Resealed`); where the built code differs from the sketch, say why.
   The facade drops `zstd`, `aes-gcm`, `zeroize` and forwards: `compression = ["localcache-format/compression"]`, `json = ["dep:serde_json", "localcache-format/json"]` (the facade still uses `serde_json` for field queries), `encryption = ["localcache-format/encryption"]`. `FormatError` becomes `pub` with `Display` texts **byte for byte** the public ones (the golden table already pins the layer text equal to the public text; keep that test in the facade).
   The lift stays a **crate-private trait in the facade** (no `From`, RFC 030 C2); `LocalFileCacheError::from_format` names the now-external `FormatError`; the golden test builds every `FormatError` variant from the external type.
3. **`test: the payload format's tests live with it`**: the 38 unit tests (24 `payload_codec`, 8 seal, 6 key), the golden vectors, the codec fixtures (`tests/fixtures/payload_codec/` with its generator) and their digest test move to the format crate. `compat-v0_18`, `compat-v0_23-encrypted` and their tests stay in the facade (they test the whole read path). Tests that reached into a moved internal are rewritten against the public API of the format crate or stay as facade tests; list them.
4. **Public rustdoc against the v0.24.0 tag** (the HTML method): `Codec` and `DEFAULT_MAX_DECOMPRESSED_BYTES` are re-exported from the facade with `#[doc(inline)]`; docs of moved items that linked to facade items become code spans (list them; say in the request what each link was). The expected differences are exactly those, plus the cosmetic `Debug::fmt` rendering you found; anything else is a finding.
5. **The layering test** (`tests/layering.rs`) loses the `serialization`/`key` rows (the compiler enforces them now) and keeps the rest; its header says so.

## 2. The crate's own checks (R5)

* `cargo hack --each-feature` on `localcache-format` (three rows plus none and all) for clippy and tests; `cargo tree -p localcache -e normal --no-default-features` shows none of `zstd`, `aes-gcm`, `zeroize`, `serde_json`; `--features compression` only `zstd`; and so on (your item 9, rerun on the real tree).
* **Miri** (not a gate): `cfg(miri)` variants for the four slow tests and the one wall-clock assertion you found, so `cargo +nightly miri test -p localcache-format --all-features` finishes; report the command, the time and the result. Put the command in the crate README.
* **Fuzz targets** (not a gate, not run in CI): `crates/localcache-format/fuzz/` with the two targets of step 0 (`from_slice` through the public API; `open` over arbitrary bytes, tags, path, with and without a key), a `Cargo.toml` that is its own workspace (so `cargo build --workspace` never needs nightly), a README line on how to run it. Run each ten minutes on the final tree and report the executions and any crash.
* The hit path (`get`, `set`; 64 B and 1 MiB; with and without a key; alternating arms, at least seven rounds, `/proc/loadavg` logged) against the v0.24.0 tag; the 1 MiB rows are layout-sensitive (review 091 § 2): report them, do not explain them away, do not act on them.

## 3. The gates, the pipeline, the book (slice 6)

1. **`Makefile.toml`:** rows for `-p localcache-format` in `gate-clippy`, `gate-test` (each feature and all), `gate-msrv`; `gate-features-assert` also asserts the format crate's three features against its own `Cargo.toml`; the facade's rows are unchanged. `gate-docs` builds the format crate's docs with and without default features (`-D warnings`). Time the matrix before and after on the host you used in step 0 and say what moved (RFC 030 C6: expect the gain to be small).
2. **`scripts/check_versions.py` and its tests:** one new check: every `localcache-format` pin (`=x.y.z`) in `[workspace.dependencies]` equals the workspace version; a test that fails when it does not.
3. **`.github/workflows/release.yaml`:** the digest steps count **three** `.crate` files in the fixed order `localcache-format`, `localcache`, `localcache-cli` (the lines you listed in step 0: `:200-201`, `:218`, `:247-249`, `:283-291`); `actionlint` exit 0; demonstrate the guard with the fake-tool harness of release 0.22.1 (a good set of three passes; two, four, a malformed digest, a wrong order fail). `.github/workflows/ci.yaml`: nothing to change if the gates run through `cargo make`; say which jobs run the new rows. `cargo publish --workspace --locked` needs no change. Do not run the `Release` workflow.
4. **The book:** `docs/src/architecture.md` (the crate layout and the layering paragraph: what the compiler now enforces and what the test still checks), `docs/src/source_archives.md` and `docs/src/security.md` (three crates are published; **the Publishing section keeps saying a release is made by the workflow and says nothing yet about the one-time manual publication** — that is written at the release, by the architect, as a record), the CLI-facing pages unchanged (users depend on `localcache`). Re-read each page you touch in full.
5. **`CHANGELOG.md`:** an `## [Unreleased]` section above `## [0.24.0]`: *Internal* entries for slices 1 to 3 (no public change; the engine no longer names `rusqlite`; a layering test) and for the new crate (a third published crate, `localcache-format`, an implementation crate; `localcache` re-exports the same API; features and MSRV unchanged), and the four documentation links lost to the move, if any. Not breaking; no announcement is owed.

## 4. For the release (slice 7): what you prepare and what only the owner can do

You prepare, under `.git-exclude/tmp/` (not tracked): **the placeholder crate** for the first manual publication, `localcache-format 0.0.1` (a `Cargo.toml` with name, version, license, description "Reserved for localcache; the implementation crate arrives in 0.25.0", repository; an empty `src/lib.rs`; a one-line README), with `cargo package` exit 0 and the file list. You do **not** publish it.
The owner's steps (RFC 030 R6), which the architect will ask for at the release: (1) create a crates.io API token with the `publish-new` scope; (2) publish the placeholder by hand; (3) on the crate's settings add the trusted publisher (repository `nabbisen/localcache-rs`, workflow `release.yaml`, environment `release`); (4) revoke the token; (5) the release runs as usual. Write these five steps, with the exact commands for (2), in the request so the owner can follow them without opening the RFC.

## 5. The version commit belongs to the release preparation

The pin `=0.24.0` and the workspace version move together to `0.25.0` in that commit (the new check of § 3.2 fails otherwise). Not here: the version, the date, `ROADMAP.md`, `docs/src/roadmap.md`.

## 6. Reports

First request (after § 1 and § 2): the commits, the API as built against the sketch, the tests that moved and that did not, the rustdoc comparison, the feature-forwarding table, the Miri and fuzz results, the hit path, the gates run so far (all twelve at the end of § 1). Second request (after § 3): the gate rows and timing, the pins check, the workflow diff and the guard demonstration, the book pages and what else you found untrue, the CHANGELOG entry, the placeholder crate, the owner's five steps.

## 7. Not yours

Storage, the `Store`'s cut (decision point 2), the adapters (R7), the version and date, publication, the `Release` workflow run, the roadmap pages.
