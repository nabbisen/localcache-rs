# Q30 — Erase the cipher's key schedule when it is dropped (`aes-gcm`'s `zeroize` feature)

Authorized by the owner on 2026-11-08 ("Almost yes, but review them carefully by yourself"); the architect's review of the decision is the section "Why, and the limits" below. Context: v0.26.1 (`aes-gcm 0.11`, released 2026-11-08 if the architect has tagged it by the time you read this: check `git tag`). `docs/src/security.md` says that the library "cannot erase … the cipher library's per-call key schedule". With `aes-gcm 0.11` it can, under a feature we do not enable. This task enables it and makes the book say exactly what then is and is not true.
Format of the review request: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6.
Authorship: high-capability model.

## 0. Why, and the limits (read by the architect in the crate sources)

* `aes-gcm 0.11.1`: `zeroize = ["dep:zeroize", "aes?/zeroize", "ghash/zeroize"]`. With it, `aes` implements `Drop` for each cipher type with `zeroize::zeroize_flat_type(self)` (round keys), `ghash`/`polyval` zeroize the GHASH key and its powers, and `AesGcm` gets `ZeroizeOnDrop` (it has no `Drop` of its own: its members do it). `AesGcm::from` zeroizes the temporary GHASH key explicitly. The counter-mode state (`ctr`) holds `&Aes` (a borrow) and a counter and nonce, which are not key material, so no `ctr` feature is needed.
* `zeroize` is already a dependency of `localcache-format`. No new crate enters the graph; no API, feature name, stored byte or MSRV changes.
* **Limits that must stay in the book.** Erasure on drop is best effort: Rust gives no guarantee about copies the compiler or the CPU made (registers, stack temporaries, the 32-byte key array that `new_from_slice` copies from `&[u8; 32]`). It cannot be proven by scanning process memory, and the book must not claim it can. The claim to make is only: the cipher object's memory (round keys, GHASH key and powers) is overwritten when the object is dropped, and which copies we do not control.
* Philosophy check (owner's): less is more (one feature flag, no code, no new dependency); safe and secure (closes a limitation the book itself states, with a library that is built for it); nothing that confuses (the sentence on the page stays true or is made more exact, never broader than the evidence).

## 1. The changes

1. `aes-gcm = { version = "0.11.1", features = ["zeroize"] }` in the workspace manifest (check how the manifest spells default features; keep them), `Cargo.lock` in full (expected: `zeroize` gains the feature use; list any package that appears or leaves).
2. **A test that proves the wiring and would fail without the feature.** In `crates/localcache-format`, under the `encryption` feature: build an `Aes256Gcm` in memory you control (for example in a `MaybeUninit` or a `Box` of the type), drop it in place, and read the bytes of that memory region afterwards; they must be zero for the whole object. This must not be undefined behavior (read the region as bytes through a pointer you obtained before the drop; the memory remains allocated; say how you made it sound and that **Miri passes** it, or why it cannot run under Miri and what you did instead). It must **fail** when the `zeroize` feature is removed (show the failing run as the negative control). If you cannot make such a test sound and meaningful, **stop and report**: do not ship a test that cannot fail.
3. **The book.** `docs/src/security.md` (the sentence about the per-call key schedule; read the whole page for others) and any other page that says the schedule cannot be erased: grep `key schedule`, `erase`, `zeroize`. State what is now erased on drop and what is still out of our control, in the project's plain style, with no word of "guarantee". CHANGELOG `[Unreleased]` → a patch section: *Security* (hardening; one sentence), nothing to do when you upgrade.
4. **Measure the cost, paired, in the style of request 080**: keyed `set`/`get` of 64 bytes and 1 MiB against the current `main` (host, load, ranges, no promise). Expectation: not distinguishable; report whatever you find.
5. Reading, not measuring: re-read `aes` 0.9.4's `Drop` (`src/lib.rs`, the `common_impls` macro) for the hardware (AES-NI/ARMv8) types as well as the software one, and say whether `zeroize_flat_type` is applied to all of them on this host's backend and on the portable one.

## 2. Verify (as for v0.26.1 in request 087, proportionally)

The format crate's tests, fuzz `open` and `from_slice` (ten minutes each) and Miri on `localcache-format`; the stored-bytes and cross-open checks against the published 0.26.1 (this changes nothing stored: show 0 failures on a smaller set than request 087, say its size); public API against the `0.26.1` tag (no difference); `cargo make gate-deny` and `gate-requirements`; all twelve gates; MSRV on 1.89.0. Do not push, version or tag: the version commit and the release candidate follow when the architect has decided what v0.26.2 contains (see Q29).

## 3. The request

One review request: the diff, the lockfile diff, the test and its negative control, the book diff, the measurements, the reading of item 5, the verification.

## 4. Not yours

The date, tags, publication, `ROADMAP.md`, `docs/src/roadmap.md`, a version bump, any claim stronger than the evidence.
