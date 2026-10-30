# Q10 Handoff — Bounded Decompression (RFC 025 Amendment 2, revised)

RFC: `rfcs/accepted/025-error-taxonomy-and-input-validation.md`, **Amendment 2 (revised 2026-10-31)**.
**Revision accepted by the owner on 2026-10-31; this file is in force.** (The first text was superseded the same
day after the architect's second review.) Start when Q3e is committed locally.
Order: … Q3e → **Q10** → release preparation. **Local commit; do not push.**
Format of the review request: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6.
Authorship: high-capability model. Authorized: the owner (after acceptance).

## 0. What this is

A stored compressed payload must never make a read abort the process. Today `decompress_bytes`
(`serialization.rs`) calls `zstd::decode_all` with no limit: a 32 KB frame can expand to 1 GiB. After this slice
the **decompressed size** is bounded by `CacheOptions::max_decompressed_bytes` (default 64 MiB), with a typed
error, on read and on write. **Only decompression is limited**: uncompressed payloads are exactly as large as
their stored blob and are not touched. Everything is `#[cfg(feature = "compression")]`: without that feature
the API does not change. Decisions D1 to D7 of the amendment are the specification; this file says how to
build and prove them. If something is not covered, **stop and file a design request**.

## 1. The change (D1–D6)

1. **The variant** (`error.rs`), only with `compression`:
   `#[cfg(feature = "compression")] #[error("payload exceeds the {limit}-byte limit for compressed payloads
   (CacheOptions::max_decompressed_bytes)")] PayloadTooLarge { limit: usize }`, with rustdoc saying what it means
   and what to do (raise the option, or treat the entry as unusable). The `compile_fail,E0004` block names only
   variants that exist in every feature combination: do not add this one to it.
2. **The option.** `#[cfg(feature = "compression")] CacheOptions::max_decompressed_bytes: usize`, default
   `DEFAULT_MAX_DECOMPRESSED_BYTES = 64 * 1024 * 1024` (a documented `pub const`, re-exported at the crate root
   under the same cfg); the builder's `max_decompressed_bytes(n)` (cfg-gated like `compress_payloads`);
   `CacheOptions::default()`. `0` is rejected in `check_configuration` as `InvalidConfiguration { option:
   "max_decompressed_bytes" }` (add `ConfigOption::MaxDecompressedBytes`, cfg-gated, nothing else), before any
   side effect, like the other configuration checks (Q3c). Thread the engine's limit to `decode_payload` and
   `encode_payload` (call sites in `cache/engine.rs` near lines 1264, 1276 and 1335; `git grep` for others,
   including the read pool, the watcher helper and the async wrappers; report the list).
3. **Read (`decode_payload`)**, for `zstd` and `json-zstd` payloads only, after decryption:
   - first `zstd::zstd_safe::get_frame_content_size`; if it is `Some` and above the limit → `PayloadTooLarge`
     **without allocating**;
   - otherwise decode through `zstd::stream::read::Decoder` behind `Read::take(limit as u64 + 1)` into a `Vec`
     with capacity `min(declared, limit)` when the size is known; more than `limit` bytes → `PayloadTooLarge`.
   A malformed frame stays `Io` as today; a frame declaring an unknown size goes straight to the bounded
   stream. `raw` and `json` payloads are **not** checked. `decompress_bytes` stays the only place that touches
   zstd decoding; `git grep -n "decode_all\|stream::read\|bulk::" -- crates` shows exactly the new one.
4. **Write (`encode_payload`)**: when the payload is about to be compressed, the serialized length before
   compression `> limit` → `PayloadTooLarge`, before any compression or encryption work. `batch_set` reports it
   per item like other encode failures. A payload that is not compressed is not checked.
5. **No decode at import or export** (D5). Do not add one. `rotate_encryption_key` is unchanged.
6. **`zstd` with `default-features = false`** (D6) in the workspace `Cargo.toml`
   (`zstd = { version = "0.14", default-features = false }`); `cargo update` must change nothing else in
   `Cargo.lock` except dropped optional packages (list the lockfile diff, as in Q1c). If `zstd::stream::read` or
   `zstd_safe::get_frame_content_size` is unavailable without a default feature, **stop and report**; do not
   re-enable a feature to make it compile. The compat fixtures must decode unmodified.

## 2. The D7 probe (required, reported either way)

A crafted `raw` (bincode, legacy configuration) payload whose length prefix claims `u64::MAX / 8` elements of a
`Vec<f32>` but holds a few bytes. Run `get` on it in a scratch binary or test under an `RLIMIT_AS` of 512 MiB
and report: an error returned, or an abort/allocation failure, and the peak resident size. **If it aborts or
allocates far beyond the payload, stop and file a design request**: that is a different threat on uncompressed
payloads, outside this amendment, and it needs its own decision. Say what you tried for `json` too.

## 3. Tests (failing before)

1. **The bomb.** A real zstd frame built in the test (a few KB, decoding to `limit + 1` bytes and, separately, to
   1 GiB) stored through the engine API with a test helper that writes arbitrary stored bytes (extend
   `tests/common`; `set_payload_encoding` retags). `get`, `get_if_fresh`, `batch_get`, every query tier (index,
   JSON, decode-everything) and `preload` return `PayloadTooLarge { limit }` and **the process survives**. On the
   base the 1 GiB case aborts or exhausts memory: run the failing-before check in a child process under
   `RLIMIT_AS` and report its exit status; do not let that run take the CI machine down.
2. **Memory is bounded, measured.** With a small limit in the test (for example 8 MiB), run the bomb under an
   `RLIMIT_AS` of `limit + 128 MiB`-class headroom (state your number) and report the peak resident size of the
   refusal. The claim in the amendment is "near the limit, not a multiple of the expansion"; show it.
3. **A forged declared size.** A frame whose header declares a small content size but whose stream is larger is
   refused (as malformed `Io` by zstd, or by the bound) and never aborts: assert the outcome you observe and
   say which.
4. **Boundaries.** With `compress_payloads`, a payload whose serialized size is exactly `limit` round-trips;
   `limit + 1` is refused by `set` (for `batch_set` reported in `failed`) and a stored `limit + 1`
   compressed payload is refused by `get`. For `zstd` and `json-zstd` and (with `encryption`) their encrypted
   variants. **Without compression, a payload larger than the limit still sets and gets** (the guard that the
   option does not limit what it should not).
5. **The option.** `max_decompressed_bytes(0)` rejected before any side effect (extend the side-effect helper);
   per-engine limits independent; the default constant is 64 MiB.
6. **The no-`compression` build** compiles and its tests pass; neither the option nor the variant exists there
   (the `gate-features` rows cover it).
7. **Compat.** The fixtures decode unmodified with `default-features = false`.
8. Every other test passes; queries that skip an oversized entry list it in `skipped` and `run()` returns the
   error (B1).

## 4. Documentation and CHANGELOG

- `docs/src/errors.md`: the `PayloadTooLarge` row and its migration note; `docs/src/builder.md` and `api.md`
  (`max_decompressed_bytes`, the default, "applies to compressed payloads only", the read/write symmetry);
  `docs/src/performance.md` only if a sentence about payload size becomes untrue. Do not create a security page
  (the architect writes it). Re-read the pages; say what else you fixed.
- Rustdoc: the option, the constant, the variant, and the `# Errors` lists of `set`, `batch_set`, `get`.
- `CHANGELOG.md`: a **Breaking** entry under `### Changed` (the option and variant with `compression`, the
  default, the error on read and write, what an upgrader with compressed payloads over 64 MiB does) and a
  `### Security` entry: the unbounded decompression is fixed (announced in 0.21.6), and `zstd` is built without
  its legacy decoders.
- `docs/src/dependency_security.md`: the `zstd` feature change in one sentence.

## 5. Gates and the request

As in Q3b's handoff § 6, plus `cargo make gate-deny` (the lockfile changed) and the 1.85 MSRV rows; run
`gate-features` (the matrix, not only `--all-features`: the option is cfg-gated). File
`.git-exclude/review-request/NNN-dev-q10-…-<date>.md` for the **uncommitted** tree; commit locally after
approval; no push, tag, version bump or `ROADMAP.md` edit. List every required step and every judgement call,
including the probe's result, the measured memory, and the lockfile diff.

## 6. Not yours

Limits on uncompressed payloads; import-time decoding; any change to the stored format; encryption design (AAD
is v0.23.0); other limits (path lengths, namespace sizes); version and release preparation.
