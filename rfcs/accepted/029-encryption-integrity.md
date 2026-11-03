# RFC 029 — Encryption Integrity: Entries Bound to Their Row, a Key That Does Not Leak

| Field | Value |
|-------|-------|
| Status | Accepted (owner, 2026-11-03), all three decisions as recommended |
| Feature | `encryption` |
| Touches | `serialization.rs` (seal and open, the tags), `cache/engine.rs` (`rotate_encryption_key`, the key cell), `cache/engine/portable.rs` (import), `db/repository.rs` and `db/repository/candidates.rs` (every reader of `payloads.content`), `cache/options.rs` and `cache/builder.rs` (the key's `Debug` and ownership), `error.rs` (one variant), `Cargo.toml` (`zeroize`), `docs/src/` (security, migration, architecture, cookbook), the compat fixtures |
| Release | **v0.23.1** (a patch: R4, R5 and the announcement) then **v0.24.0** (a minor release: R1 to R3, breaking). No schema change |
| Scope document | `.git-exclude/reviewed/081-architect-v0.24.0-scope-reviewed-against-the-philosophy-2026-11-03.md` (authorized by the owner, 2026-11-03) |

## Summary

With a key configured, an entry is readable **only if it is sealed under that key and bound to its row**. Anything else is an error that names the remedy,
never a silent read. The key never prints, and the crate erases the copies of it that it holds.

## Motivation (observed on the released v0.23.0 tree, 2026-11-03; `.git-exclude/tmp/probe24/`)

1. **Swap.** Two entries stored under one key; one `UPDATE` copies the first ciphertext onto the second row. `get` of the second returns the first's secret,
   as `Ok`. The ciphertext is authenticated, but not tied to the row that holds it.
2. **Downgrade.** An entry written by an engine with no key is returned, as `Ok`, by an engine configured with a key.
3. **The stored `encoding` tag is not authenticated.** It tells the reader how to decode the bytes after decryption, and anyone who can write the file can change it.
4. **The key prints.** `format!("{:?}", options)` of a `CacheOptions` with a key contains `encryption_key: Some([7, 7, …])`.
5. **The crate copies the key and keeps the copies.** The builder's `Vec<u8>`, the options, and the engine's `Cell<Option<[u8; 32]>>`, whose `.get()` copies the array out on every call.
6. **`rotate_encryption_key` holds the whole namespace in memory** (`load_encrypted_payloads`), and under the rule above it becomes the one way an existing database is brought forward.

A feature whose purpose is to protect payloads returns another entry's secret, accepts plaintext as if it were protected, and writes its key into logs. For a user who
configured a key, a returned `Ok` means "this is what I stored, protected". It must mean that.

## Goals and non-goals

Goals: R1 to R5 below. Non-goals, stated so the book can state them: the **freshness** of an entry (someone who can write the file can restore an older valid
ciphertext of the same row, or delete entries); the confidentiality of **metadata** (paths, namespaces, sizes, times and the tag stay in the clear, as today); **isolation between namespaces**
under one key (use one key per trust domain; see R1); protection against an attacker who can read the process's memory.

## Design

### R1 — Bind each sealed payload to its row (associated data)

A payload is sealed with AES-256-GCM as today (random 96-bit nonce, `nonce ‖ ciphertext ‖ tag`) **with associated data**:

    "localcache/seal/1" 0x00 <the full encoding tag> 0x00 <the entry's path, as stored>

so the ciphertext authenticates which entry it belongs to and how it is to be decoded. A ciphertext moved to another path, or opened under another tag, fails to open.
**The namespace is not bound.** Bound or not, a namespace is a partition of one keyspace, and binding it would make every raw copy (`import_from`, `copy`, `import`) re-seal an entry,
which the CLI cannot do (it takes no key) and which would turn `localcache copy` of an encrypted namespace into a silent success that produces unreadable entries. Someone who can
write the file can therefore move a payload between namespaces **under one key**; the book says so and says what to do: one key per trust domain.

A bound entry carries a new tag suffix, `-aes256gcm-bound` (`raw-aes256gcm-bound`, `zstd-aes256gcm-bound`, `json-aes256gcm-bound`, `json-zstd-aes256gcm-bound`). The old suffix `-aes256gcm` continues
to mean "sealed without binding". The tag, not a column, tells the two formats apart: no schema change. A v0.23.x or earlier reader meets the new tags as `UnknownEncoding` and fails closed.

### R2 — With a key set, only bound entries are read

With a key configured, `get`, `get_if_fresh`, the batch reads, queries (`run` and `run_report`) and every other path that decodes a payload open an entry only if its tag is bound; a plain entry or an entry sealed without binding
returns the new `LocalFileCacheError::UnsealedEntry` (`#[cfg(feature = "encryption")]`; the enum is `#[non_exhaustive]`), whose text names the remedy (`rotate_encryption_key`, or purge the entry). A bound entry
that fails to open is `EncryptionError`, whose text now reads "wrong key, corrupted data, or the entry was moved". Without a key the behaviour is today's: a sealed tag is `MissingEncryptionKey`, a plain tag is read.

**Writes.** With a key, `set` and `batch_set` write bound entries; without one, plain entries (a user's choice). A keyed `import_entries` or `import_from` accepts only records whose tag is bound and
refuses the whole import before writing otherwise (`InvalidArgument { argument: "records" }`, text naming the remedy), the same shape as the recency rule of RFC 026. It checks the tag, not the
authenticity: a record that is forged or sealed under another key is imported and fails closed on read, as any corrupt entry does.

This is a rule, not an option. A `require_encryption` option would leave the defect on for everyone who did not read about it.

### R3 — `rotate_encryption_key` is the upgrade path, and it is bounded

`rotate_encryption_key(new_key)` re-seals **every entry of the namespace** under `new_key`, bound to its path, whatever its current state: plain (it seals the bytes as they are, adding the suffix),
sealed without binding (opened with the engine's current key, no associated data), or bound. `new_key` may equal the current key: that is the upgrade of a v0.23.0 database, and
how encryption is switched on for a plain one (open with the key, rotate to it). Everything happens in **one `IMMEDIATE` transaction** (the all-or-nothing property of today's method, and its guard
against a concurrent writer, are kept), but rows are read and written in **keyset pages** by `file_id` (the pattern of RFC 020): at most `PAGE_ROWS` rows or `PAGE_BYTES` bytes per page, so memory is
bounded by the page, not by the namespace. The engine switches to `new_key` only after the commit, as today. It returns the number of entries re-sealed. An unknown tag or any failure aborts and changes nothing.

The write-ahead log grows by about the size of the namespace's payloads (the transaction is as large as today's); the book says so.

### R4 — The key does not print (v0.23.1)

`CacheOptions` has a hand-written `Debug` that prints `encryption_key: Some("[redacted]")` and destructures the struct **without** `..`, so a field added later does not compile until `Debug` is updated.
The field keeps its type (`Option<Vec<u8>>`): a public field's type is API. A test asserts that no byte of a configured key appears in the `Debug` output of the options, and (step 0) in any other `Debug`,
`Display` or `tracing` output the crate produces.

### R5 — The crate erases the copies it holds (v0.23.1)

The key is held in one type of the crate's own: `Key([u8; 32])` is not `Copy`, erases itself on drop (`zeroize`), prints `[redacted]`, and is passed by reference, never copied out (the cell becomes
a `RefCell<Option<Key>>`, and the call sites borrow). The `Vec<u8>` the options hold is zeroized when `open` consumes the options. `CacheOptions` does **not** get a `Drop`: a type with `Drop` cannot be
moved out of, which would forbid `CacheOptions { …, ..Default::default() }`, the form the book teaches. The cipher object built for each call holds the expanded key too; step 0 checks what `aes-gcm`'s `zeroize` feature erases of it, and the book says only what was checked.
What this is not, and the book says it: it cannot erase the caller's own copy of the key, a register, the stack frames of other code, or the swap file. It is "the crate does not leave the key behind", never "the key cannot leak".

### R6 — The book states what is and is not protected

`security.md` gets one table of an attacker who can write the file but has no key: cannot read, cannot forge, cannot move an entry to another path, cannot downgrade an entry to plain, cannot change how a sealed entry
is decoded; **can** delete entries, restore an older valid ciphertext of the same row, move a payload between namespaces under one key, and read all metadata. It also states the nonce limit: with random 96-bit nonces
NIST SP 800-38D allows one key at most 2^32 encryptions (about 50 days at 1,000 writes per second); rotation starts a new key.

## Compatibility

* **v0.23.1** is a patch: a redacted `Debug` and an erased key change no API and no stored byte. It also carries the **announcement** the project's rule requires one release ahead (Phase 24): what R1 to R3 will do,
  who must act (everyone with an encryption key and an existing database), and how (`rotate_encryption_key(same key)` once, or purge).
* **v0.24.0** is a minor release and breaking: a database written by v0.23.x or earlier with a key refuses every read of a sealed-without-binding or plain entry until one rotation; bound entries are refused by v0.23.x and earlier as `UnknownEncoding`,
  so for encrypted entries v0.24.0 is a **one-way door** (the rollback is a purge, or an export from before). Unencrypted databases and keyless engines are not affected.
* **The wire-format guarantee** (`docs/src/migration.md`: "Payloads written by any 0.x release are readable by every other 0.x release") is scoped by this RFC and the owner is asked to accept the scoping (Decision 1): it is
  about the payload codec bytes, and it holds for every entry **except a sealed entry that is read with the strict rule**. The guarantee's own text allows "a deliberate, headline CHANGELOG item ... documented with migration tooling, and backed by updated
  compatibility tests"; this is that item, the tooling is `rotate_encryption_key`, and the compatibility test is the fixture of the test plan. The page is amended in place, not annotated.
* The CLI is unchanged: it takes no key, lists and exports sealed entries verbatim, and imports and copies them verbatim, which stays valid because the namespace is not bound.

## Test plan

Each behaviour change has a test that **fails on the v0.23.0 tree** first.

* Swap (Motivation 1): after the `UPDATE`, `get` of the second entry is `EncryptionError`. Tag change (3): a sealed entry whose tag is edited to another tag fails to open. Downgrade (2): a keyed engine reading a plain entry is `UnsealedEntry`; so is one sealed without binding.
* Debug (4): the options' `Debug` output contains no byte of the key, in decimal list form or hex; the exhaustive-destructure compiles only with every field; builder and async engine likewise (step 0 lists every type that can hold the key).
* A committed fixture written by **v0.23.0** with a key (all four tag variants, one plain entry, a payload that is compressed): the v0.24.0 keyed engine refuses each with `UnsealedEntry`; `rotate_encryption_key` with the same key
  reports the count; every entry then reads to its expected value; a second rotation to a new key works; a keyless engine gets `MissingEncryptionKey`. The fixture's README says how it was made.
* Rotation: failure injection after a page, in the middle, and at commit leave the database byte-identical and the engine on its old key; a concurrent writer is not overwritten (the existing C2 test); the page limits are tested with a
  hook that shrinks them to one row; plain and legacy rows seal and read.
* Import: a keyed import of a plain record, and of a legacy-sealed record, fails whole before writing; of bound records succeeds; a keyless import of sealed records succeeds (verbatim).
* The CLI: `export` then `import` of a sealed namespace into another namespace of the same database, read by a keyed engine: succeeds (the namespace is not bound).
* Property test: for random path, tag, key and plaintext, `open(seal(x, path, tag), path, tag) = x`, and any change of path, tag, key or one bit of the ciphertext fails.

## Step 0: what the dev team verifies before any code (the stop rule is below)

1. `aes-gcm 0.10.3` takes associated data (`aead::Payload { msg, aad }`) and what the extra authenticated bytes cost on `set` and `get` with encryption (measured, alternating arms, a loaded host stated).
2. **An inventory of every reader and writer of `payloads.content` and of the `encoding` tag across the whole workspace, the CLI included**: `serialization.rs`, `repository.rs` (`load_payload`, `load_all_full`, `load_encrypted_payloads`, `update_payload_content`, the upserts, the import),
   `candidates.rs` (the batch fetch and the JSON pushdown), `portable.rs`, `pool.rs`, the async layer, `ReadPool`, `inspect`, `list`, every test that pins the tags. For each: how it learns the path, and what R2 changes.
3. Whether the field-query pushdown can ever evaluate an encrypted payload in SQL (it must not).
4. `zeroize`: whether `aes-gcm`'s `zeroize` feature suffices, the resolved version at MSRV 1.85, `cargo deny`, the lockfile diff; the `Cell` to `RefCell` change at every `.get()`.
5. Paged rotation at 1,000,000 encrypted entries: peak memory against today's, write-ahead log growth, time; the page limits chosen from the measurement, not guessed.
6. Every `Debug`, `Display` and `tracing` path that could reach the key (the options, the builder, the engine, the pool, error payloads).

**Stop rule (from the scope review).** If step 0 shows that binding (R1) cannot stay simple, R1 to R3 leave this RFC and v0.23.1 ships R4 and R5 alone; the swap and the downgrade then stay documented limits. If R5 costs more than it is worth, it leaves alone.

## Alternatives considered

* **Bind the namespace as well.** Rejected: it breaks raw copy for the CLI and makes `copy` a silent producer of unreadable entries (R1). Per-trust-domain keys give the same isolation without it.
* **A `require_encryption` option, off by default.** Rejected: a rule plus an exception, and the defect stays on by default.
* **Read sealed-without-binding entries under a key during a transition.** Rejected: nobody can tell whether a database is fully bound, and the swap stays possible for exactly the entries a user has not yet upgraded. One rule, one explicit step.
* **Upgrade on open, as the schema does.** Rejected: it needs the key at open, runs crypto the user did not ask for, and hides the one security-relevant event of the upgrade.
* **A new method for the upgrade.** Rejected: `rotate_encryption_key` already re-seals every entry; making it seal every entry whatever its state is the smaller API.
* **`#[non_exhaustive]` and a `Key` field type in `CacheOptions`.** Rejected: the public field's type is API, and `#[non_exhaustive]` forbids the documented `..Default::default()` form (scope review 081).
* **`aes-gcm 0.11`.** Not needed: associated data works in 0.10.3. A major upgrade adds churn and fixes nothing here.
* **Authenticating freshness** (a per-row counter or a version in the associated data). Rejected as over-engineering for a cache: the attacker can rewrite the row's metadata too. The limit is stated instead.

## Attack on this design (what the drafting found, before the owner is asked)

1. *Namespace binding looked like the safe reading and was wrong:* the CLI has no key, so `copy` would silently produce unreadable entries. Caught by reading the CLI before choosing.
2. *The wire-format guarantee is a documented permanent commitment, and the strict rule breaks its plain reading.* It has an escape clause that fits exactly, but the owner decides whether to use it (Decision 1).
3. *The project promises to announce a breaking change one release ahead.* Shipping R1 to R3 in the next release would break that promise; hence v0.23.1, which also gets the leak fixed earlier.
4. *A `Drop` on `CacheOptions` would have forbidden `..Default::default()`.* Rejected before it reached the design; the options' `Vec` is zeroized when `open` consumes it.
5. *The engine's `Cell<[u8; 32]>` copies the key on every `.get()`; zeroizing the cell would have erased one of many copies.* R5 passes the key by reference.
6. *Strictness without an upgrade path locks users out;* hence R3, and R3's memory bound became a requirement.
7. *Import into a keyed engine could silently create entries the same engine refuses to read.* R2 refuses them at the import.
8. *Binding does not defend freshness, namespaces under one key, or metadata.* Stated as non-goals and in the book, not hidden.

## Rollback

v0.23.1: none needed. v0.24.0: for encrypted entries, a purge or an export from before; a revert of the code leaves bound entries unreadable to older versions (`UnknownEncoding`). The CHANGELOG says so.

## Decisions

Accepted by the owner on 2026-11-03 with the recommended answers:
1. The wire-format guarantee is scoped as in Compatibility, and `docs/src/migration.md` is amended in place (at v0.24.0, when the behaviour ships; v0.23.1 announces it).
2. Delivery in two releases: v0.23.1 (R4, R5 and the announcement), then v0.24.0 (R1 to R3).
3. The namespace is not bound (R1), with "one key per trust domain" in the book.

Handoffs: `../handoffs/029-encryption-integrity/` (step 0 first; the slices after the architect's review of step 0).

## Open questions

None for the owner. The six items of step 0 are verifications with a stop rule, not design questions.
