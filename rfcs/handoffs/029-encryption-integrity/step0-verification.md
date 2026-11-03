# RFC 029 Step 0 — Verify the Design Before Any Code

RFC: `rfcs/accepted/029-encryption-integrity.md` (accepted by the owner, 2026-11-03). Scope: `.git-exclude/reviewed/081-architect-v0.24.0-scope-reviewed-against-the-philosophy-2026-11-03.md`.
Start now. This is **verification and measurement only**: no tracked file changes, everything under `.git-exclude/tmp/`. A scratch probe crate that reproduces the defects already exists in `.git-exclude/tmp/probe24/` (it depends on the workspace by path; copy it, do not edit it).
Format of the review request: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6.
Authorship: high-capability model. Authorized: the owner.

## 0. Why, and the rule

RFC 029 binds each sealed payload to its row (R1) and then refuses everything else (R2, R3). That is not proven to stay simple. The owner's rule: **if the verification shows binding cannot stay simple, R1 to R3 leave the RFC and v0.23.1 ships R4 and R5 alone.**
You do not decide that: you report the evidence and the architect decides. Be adversarial; try to break R1 and R3, and look for any reader or writer of a payload that the RFC did not name. Items 4 and 6 also decide whether R5 is worth its dependency.

## 1. What to establish, each with a command, its output and an exit code

1. **Associated data and its cost.** Show that `aes-gcm 0.10.3` seals and opens with `aead::Payload { msg, aad }`, and that a change of one byte of the associated data, of the key, or of the ciphertext makes `open` fail. Measure `set` and `get` through the crate with encryption (a 64-byte and a 1 MiB payload),
   with and without associated data of the RFC's shape (`"localcache/seal/1" 0x00 <tag> 0x00 <path>`, a path of about 100 bytes), alternating arms, at least five rounds, `/proc/loadavg` at the start of each. Report the difference as a number with its spread; no claim from a single round.
2. **The inventory.** Every reader and every writer of `payloads.content` and of the `encoding` tag across the **whole workspace** (`crates/localcache`, `crates/cli`, tests, benches, the docs' code blocks): `serialization.rs`, `db/repository.rs` (`load_payload`, `load_all_full`, `load_encrypted_payloads`, `update_payload_content`, the upserts, the import),
   `db/repository/candidates.rs` (the batch fetch and the JSON pushdown), `cache/engine*.rs`, `pool.rs`, the async layer, `ReadPool`, `inspect`, `list`, `export`, `import`, `copy`, the watcher, every test that pins a tag string. A table: site, read or write, how it learns the entry's **path**, what R2 changes there, and whether anything the RFC did not name appears.
3. **The path that is bound.** The associated data uses "the entry's path, as stored" (`files.path`). Show that at every sealing site (`set`, `batch_set`, rotation, the import) and every opening site (item 2) the *same string* is available and equal, including through `ReadPool`, the async engine and a path given in a non-canonical form.
   Show a case where it would not be (if one exists, that is the finding).
4. **The pushdown.** Can the JSON field-query pushdown ever evaluate an encrypted payload in SQL? It must not. Show the code path and a test or probe that proves an encrypted entry is never matched in SQL.
5. **Old readers.** Build the v0.23.0 tag (`git archive 0.23.0`) and show what it does with an entry whose tag carries the new suffix `-aes256gcm-bound` (written by editing the tag in a scratch database): the RFC says `UnknownEncoding`, fail closed. Show it.
6. **Where the key can appear.** Every `Debug`, `Display` and `tracing` path that could reach the key bytes: the options, the builder, the engine, the pool, `ReadPool`, the async engine, error payloads, the CLI. For each: derives or prints what, and whether the key is reachable. Reproduce the leak (`{:?}` of the options) and show the exhaustive-destructure form of a hand-written `Debug`
   compiling and failing to compile when a field is added.
7. **`zeroize`.** Whether `aes-gcm`'s `zeroize` feature suffices or the crate needs its own `zeroize` dependency; the resolved version at MSRV 1.85 (`cargo +1.85.0`, fresh resolution); `cargo make gate-deny` with it; the `Cargo.lock` diff. What the `Cell<Option<[u8; 32]>>` to `RefCell<Option<Key>>` change touches (every `.get()` site, listed).
   Say what `aes-gcm`'s `zeroize` feature erases of the cipher object, checked in the source, not assumed.
8. **Paged rotation at 1,000,000 encrypted entries.** On a scratch database of 1,000,000 entries sealed under the current code (64-byte and 4 KiB payloads): the peak memory and time of today's `rotate_encryption_key`, and of a prototype paged by `file_id` in one `IMMEDIATE` transaction (the prototype lives under `.git-exclude/tmp/`, outside the tracked tree),
   at three page sizes; the write-ahead log growth; the correctness check that every entry opens afterwards. Propose `PAGE_ROWS` and `PAGE_BYTES` from the numbers.
9. **Import.** Show what `import_entries` and `import_from` do today with a plain record into a keyed engine (the RFC's R2 refuses it), and what a CLI `export` then `import` of a sealed namespace into another namespace does, so the test plan's CLI case is a fact before it is a test.

## 2. The report

One review request: `.git-exclude/review-request/NNN-dev-rfc029-step0-<date>.md`, with each of 1 to 9 answered with evidence, then:
- **Simplicity:** an honest estimate of the lines and files R1 to R3 add, the order of the work, and anything that surprised you;
- **Findings that contradict the RFC**, each stated plainly (the architect corrects the RFC);
- a one-paragraph verdict on whether R1 to R3 can stay simple, and on whether R5 is worth its cost. You report; you do not decide.

## 3. Not yours

Any change to a tracked file; the implementation; the version; the decision to drop R1 to R3 or R5.
