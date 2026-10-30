# Q10 Handoff — Bounded Payload Decoding (RFC 025 Amendment 2)

RFC: `rfcs/accepted/025-error-taxonomy-and-input-validation.md`, **Amendment 2**. **Accepted by the owner on 2026-10-31
as written (the 64 MiB default and the name `PayloadTooLarge` included); this file is in force.** Start when Q3e
is committed locally.
Order: … Q3e → **Q10** → release preparation. **Local commit; do not push.**
Format of the review request: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6.
Authorship: high-capability model. Authorized: the owner (after acceptance).

## 0. What this is

A stored payload must never make a read abort the process. Today `decompress_bytes` (`serialization.rs`) calls
`zstd::decode_all` with no limit: a 32 KB frame can expand to 1 GiB. After this slice every payload is bounded
by `CacheOptions::max_payload_bytes` (default 64 MiB) on **read and write**, with a typed error. Decisions D1 to
D7 of the amendment are the specification; this file says how to build and prove them. If something is not
covered, **stop and file a design request**.

## 1. The change (D1–D6)

1. **The variant** (`error.rs`), every feature combination:
   `#[error("payload exceeds the {limit}-byte limit (CacheOptions::max_payload_bytes)")] PayloadTooLarge { limit: usize }`,
   with rustdoc saying what it means and what to do (raise the option, or treat the entry as unusable).
   Add it to the `compile_fail,E0004` block's neighbours only if it exists in every combination (it does);
   keep that block passing for the right reason.
2. **The option.** `CacheOptions::max_payload_bytes: usize`, default `DEFAULT_MAX_PAYLOAD_BYTES = 64 * 1024 *
   1024` (a documented `pub const` re-exported at the crate root); the builder's `max_payload_bytes(n)`;
   `CacheOptions::default()`. `0` is rejected in `check_configuration` as `InvalidConfiguration { option:
   "max_payload_bytes" }` (add `ConfigOption::MaxPayloadBytes`, nothing else), before any side effect, like the
   other configuration checks (Q3c). Thread the engine's limit to every `encode_payload` / `decode_payload` call
   (the call sites are `cache/engine.rs` lines near 1264, 1276 and 1335; `git grep` for others, including the
   read pool, the watcher helper and the async wrappers, and report the list).
3. **Read (`decode_payload`)**, after decryption, for every codec:
   - a `zstd` frame: first `zstd::zstd_safe::get_frame_content_size`; if it is `Some` and above the limit →
     `PayloadTooLarge` **without allocating**; otherwise decode through `zstd::stream::read::Decoder` behind
     `Read::take(limit as u64 + 1)` into a `Vec` with capacity `min(declared, limit)` when the size is known;
     more than `limit` bytes → `PayloadTooLarge`. A malformed frame stays `Io` as today. A frame that declares
     an unknown size (`None`) goes straight to the bounded stream;
   - an uncompressed payload: `data.len() > limit` → `PayloadTooLarge`.
   Keep `decompress_bytes` the only place that touches `zstd::decode`; do not leave an unbounded call anywhere
   (`git grep -n "decode_all\|stream::read\|bulk::" -- crates` shows exactly the new one).
4. **Write (`encode_payload`)**: the serialized length **before** compression and encryption `> limit` →
   `PayloadTooLarge`, before any compression or encryption work. `batch_set` reports it per item like other
   encode failures.
5. **No decode at import or export** (D5). Do not add one. `rotate_encryption_key` is unchanged.
6. **`zstd` with `default-features = false`** (D6) in the workspace `Cargo.toml`
   (`zstd = { version = "0.14", default-features = false }`); `cargo update` must change nothing else in
   `Cargo.lock` except dropped optional packages (list the lockfile diff, as in Q1c). If `zstd::stream::read`
   or `zstd_safe::get_frame_content_size` is unavailable without a default feature, **stop and report**; do not
   re-enable a feature to make it compile. The compat fixtures must decode unmodified.

## 2. The D7 probe (required, reported either way)

A crafted `raw` (bincode, legacy configuration) payload whose length prefix claims `u64::MAX / 8` elements of a
`Vec<f32>` but holds a few bytes. Run `get` on it in a scratch binary or test **with a ulimit/`RLIMIT_AS` of
512 MiB** (or measure peak RSS) and report: error returned, or abort/alloc failure. If it allocates beyond a
small multiple of the payload, bound it: bincode 2's `Configuration::with_limit::<N>()` takes a const, so
choose the smallest mechanism that keeps the legacy wire format (a size check before the deserializer is
**not** enough, because the claim is inside the data). If you need a design decision, stop and ask. Do the
same probe for `json` (a document claiming nothing: skip if serde_json cannot be tricked) and say so.

## 3. Tests (failing before)

1. **The bomb.** A real zstd frame built in the test (a few KB, decoding to `limit + 1` bytes and, separately,
   to 1 GiB) stored through the engine API with the test hook that writes a stored payload (`common` has
   `set_payload_encoding`; add a helper that writes arbitrary stored bytes). `get`, `get_if_fresh`,
   `batch_get`, every query tier (index tier, JSON tier, decode-everything tier) and `preload` return
   `PayloadTooLarge { limit }` and **the process survives**. On the base the 1 GiB case aborts or exhausts
   memory: run the failing-before check in a child process under `RLIMIT_AS` (a test that spawns itself, or a
   scratch run) and report the exit status; do not let the failing-before run take the CI machine down.
2. **Memory is bounded.** Under an `RLIMIT_AS` of `3 × limit + 256 MiB` (use a small custom limit such as 8 MiB
   in the test, and scale the rlimit accordingly), the bomb is refused and the process does not exceed it.
3. **A forged declared size.** A frame whose header declares a small content size but whose stream is larger
   is refused (by zstd as malformed `Io`, or by the bound) and never aborts: assert the outcome you observe
   and say which.
4. **Boundaries.** A payload whose serialized size is exactly `limit` round-trips; `limit + 1` is refused by
   `set` (and for `batch_set` reported in `failed`) and a stored `limit + 1` payload is refused by `get`. For
   `raw`, `zstd`, `json`, `json-zstd`, and (with `encryption`) the encrypted variants.
5. **The option.** `max_payload_bytes(0)` rejected before any side effect (extend the existing side-effect
   helper); per-engine limits are independent; the default constant is 64 MiB.
6. **Compat.** The fixtures decode unmodified with `default-features = false`.
7. Every other test passes; queries that skip an oversized entry return it in `skipped` and `run()` returns the
   error (B1).

## 4. Documentation and CHANGELOG

- `docs/src/errors.md`: the `PayloadTooLarge` row and its migration note; `docs/src/builder.md` and `api.md`
  (`max_payload_bytes`, the default, the symmetry of read and write); `docs/src/security` page if one exists
  (the architect is writing a security-model page; do not create one); `docs/src/performance.md` only if a
  sentence about payload size becomes untrue. Re-read the pages; say what else you fixed.
- Rustdoc: the option, the constant, the variant, `set`/`batch_set`/`get` error lists.
- `CHANGELOG.md`: a **Breaking** entry under `### Changed` (the option, the default, the error on read and
  write, what an upgrader with payloads over 64 MiB does) and a `### Security` entry: the unbounded decode is
  fixed (announced in 0.21.6), `zstd` built without legacy decoders.
- `docs/src/dependency_security.md`: the `zstd` feature change in one sentence.

## 5. Gates and the request

As in Q3b's handoff § 6, plus `cargo make gate-deny` (the lockfile changed) and the 1.85 MSRV rows. File
`.git-exclude/review-request/NNN-dev-q10-…-<date>.md` for the **uncommitted** tree; commit locally after
approval; no push, tag, version bump or `ROADMAP.md` edit. List every required step and every judgement call,
including the probe's result and the lockfile diff.

## 6. Not yours

Import-time decoding; any change to the stored format; encryption design (AAD is v0.23.0); other limits (path
lengths, namespace sizes); version and release preparation.
