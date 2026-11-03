# Q14 Handoff — The Key Does Not Print, the Crate Erases Its Copies, and v0.24.0 Is Announced (RFC 029 R4, R5; v0.23.1)

RFC: `rfcs/accepted/029-encryption-integrity.md` (R4, R5, Compatibility). Review that raised it: `.git-exclude/reviewed/082-architect-rfc029-step0-review-2026-11-03.md`.
Format of the review request: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6. **Local commits; do not push.** The v0.23.1 release preparation is a separate handoff, after this is approved.
Authorship: high-capability model. Authorized: the owner (RFC 029).

## 0. What this is

v0.23.1 is a patch: a fix and an announcement. No API, schema or stored-byte change; nothing of R1 to R3 is implemented here. The announcement is owed by the project's rule (a breaking change is announced one release ahead) and is what makes v0.24.0 possible.

## 1. The change

1. **R4: the options do not print the key.** A hand-written `Debug for CacheOptions` in `cache/options.rs`: `encryption_key` prints `Some("[redacted]")` or `None`, every other field as the derive printed it (same names, same order). Destructure the struct **without** `..` inside the impl (a comment says a forgotten field is then a compile error, and that `..` is the compiler's suggested fix, so a reviewer must refuse it). The field keeps its type.
   Test (it must fail on the v0.23.0 tree first): a distinctive 32-byte key that is not a repeated byte; assert that no run of four consecutive bytes of it appears in `{:?}` or `{:#?}` of the options, as a decimal list or as hex.
2. **R5: the `Key` type.** A `pub(crate)` type under `#[cfg(feature = "encryption")]`: `Key([u8; 32])`, not `Copy`, not `Clone` unless a use needs it (say which), erases itself on drop (`zeroize::ZeroizeOnDrop`), `Debug` prints `[redacted]`, bytes available by reference only.
   * `Cargo.toml`: `zeroize = "1"` as an optional workspace dependency, `encryption = ["dep:aes-gcm", "dep:zeroize"]`. **Do not** enable `aes-gcm`'s `zeroize` feature and **do not** add the five per-crate features: step 0 measured that the first erases nothing observable and the second is five declarations with no use of their own (RFC 029 R5, "Not done, on purpose"). `Cargo.lock` gains one package; report the diff.
   * The engine's `Cell<Option<[u8; 32]>>` becomes `RefCell<Option<Key>>`; borrow, never copy out. The sites (step 0 § 7): field `engine.rs:139`, init `:260`, rotate read `:873` and write `:927`, `encode` `:1309`, `decode` `:1323`, `EngineCore` field `:1347`/`:1372`, `decode_with` `:1388`, `check_configuration` `:1480` (returns a `Key`, not an array), the builder (`builder.rs:251-252`).
     Rotation keeps the old key until the commit and then replaces it; the replaced `Key` is dropped, which erases it. Do not hold a `RefCell` borrow across a database call.
   * The `Vec<u8>` copies: the builder erases the previous `Vec` when `encryption_key()` is called again and the one it holds when `open` consumes it; `CacheEngine::open` zeroizes the options' `Vec` after moving the key into a `Key`; `ReadPool::open` zeroizes `base` after the per-slot clones (`read_pool.rs:138`), and each clone is erased by its slot's `open`.
     **No `Drop` on `CacheOptions`** (it would forbid `..Default::default()`).
   * Tests: `Key` is `ZeroizeOnDrop` (a compile-time assertion); erasing a `Key` leaves zeros (use the `Zeroize` trait explicitly if the crate forbids `unsafe`, and say which); after `open`, the options' `Vec` is empty; a `ReadPool` of N slots opens and reads with a key; rotation still works, including the failure cases of the existing tests (the engine keeps the old key on error).
   * Do not claim more in the rustdoc or the book than: "the crate does not keep its own copies of the key after they are dropped".
3. **The documentation that changes with R4 and R5.** `docs/src/security.md`: replace the sentence "The library copies it into engine memory and does not zero it on drop; do not assume it is erased" with what is now true and its limits (the crate erases the copies it holds; it cannot erase your own copy of the key, other code's stack frames, the cipher library's per-call key schedule, or swap), and replace the bullet "`CacheOptions` derives `Debug` ... (v0.23.0 and earlier)" with "fixed in v0.23.1; before it, do not log the options of an engine that has a key". Re-read the whole page and say what else you found untrue.
4. **The announcement** (as v0.22.2 did for RFC 026), in `CHANGELOG.md` (`## [0.23.1] — Unreleased` with `*Date set at owner authorization.*`, a `### Security` entry for the fix, and the announcement), and `docs/src/migration.md` (a section "Announced: encryption integrity (v0.24.0)", like the v0.22.2 one was for v0.23.0), and one sentence in `README.md`'s encryption bullet if it states what encryption guarantees. It says, in this order and nothing else, checked against RFC 029 and the tree:
   * what v0.24.0 will do: with a key, an entry is readable only if sealed under it and bound to its path (the namespace is not bound; use one key per trust domain); plain and unbound entries are refused with a new error `UnsealedEntry` that names the remedy; queries on a keyed engine never use the SQL pushdown;
   * **who must act and how**: everyone with an encryption key and an existing database; after upgrading, call `rotate_encryption_key` once with the same key (it re-seals every entry of the namespace, paged and atomic), or purge; unencrypted databases and engines without a key are not affected;
   * it is a one-way door for encrypted entries: v0.23.x and earlier read the new tags as `UnknownEncoding`; **do not call `rotate_encryption_key` with an older version once v0.24.0 has written to the database** (step 0: v0.23.0 returns `Ok(0)` and switches its in-memory key, so a user could believe the key was rotated and discard the old one);
   * what is not protected (freshness; metadata; namespaces under one key; an attacker with the process's memory);
   * the wire-format guarantee: the page `migration.md` will be scoped in place at v0.24.0 (state that it will, and how); do not change it now.
   Every sentence about v0.24.0 is in the future tense and nothing in the book yet says it has happened. Do not state a performance number for R1 to R3 beyond step 0's (a 64-byte `get` +0.19 µs, +3%, on a loaded host).

## 2. Gates and the request

All gates of the previous slices (fmt, clippy `-D warnings`, `gate-features`, test, rustdoc, `gate-versions`, MSRV 1.85, `gate-deny`, `bench-compile`, `mdbook`), plus `gate-docs` and `gate-archive` on the committed tree. Report the `Cargo.lock` diff and the `cargo deny` output.
One review request `.git-exclude/review-request/NNN-dev-q14-<date>.md`: the failing-before output of the R4 test on the v0.23.0 tree, the list of key copies and what happens to each, the gates, and anything you found untrue.
Commit locally after approval, in this order: `fix: the encryption key no longer prints, and the crate erases the copies it holds (RFC 029 R4, R5)`, then `docs: announce encryption integrity for v0.24.0 (RFC 029)`. Do not touch the version or the date.

## 3. Not yours

R1 to R3, the new error variant, the tag table, the strict reading, the rotation rewrite (v0.24.0); the version, the date, `ROADMAP.md`, `docs/src/roadmap.md`.
