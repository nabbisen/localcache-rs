# Q15 Handoff — Bound Entries, the Strict Reading, Paged Rotation, the Import Check (RFC 029 R1 to R3; v0.24.0)

RFC: `rfcs/accepted/029-encryption-integrity.md` (R1, R2, R3, Compatibility, Test plan). Step 0: `rfcs/handoffs/029-encryption-integrity/step0-verification.md` and review `.git-exclude/reviewed/082-architect-rfc029-step0-review-2026-11-03.md` (read both: the inventory of readers and writers is there).
Format of the review request: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6. **Local commits; do not push.** Q16 (the book, the CHANGELOG, the guarantee scoping) starts after this is committed.
Authorship: high-capability model. Authorized: the owner (RFC 029).

## 0. What this is

The behaviour of v0.24.0, with no schema change. Everything below is in `crates/localcache`; the CLI is untouched (it never decodes, step 0 § 2). Work in five commits, each building and passing all tests, so the review can follow the order:

1. `feat: seal payloads bound to their path (RFC 029 R1)` 2. `feat!: with a key, read only bound entries; UnsealedEntry (RFC 029 R2)` 3. `feat!: rotate_encryption_key seals every entry, in pages by path (RFC 029 R3)` 4. `feat!: a keyed import accepts only bound records (RFC 029 R2)` 5. `test: the v0.23 encrypted fixture and the upgrade (RFC 029)`.

## 1. The change

### 1.1 R1: the tag table, the associated data, the path threaded through

* In `serialization.rs`, replace `strip_suffix(ENC_SUFFIX)` by one parse of the stored tag into (base tag, sealing), sealing being `None`, `Legacy` (`-aes256gcm`) or `Bound` (`-aes256gcm-bound`). Matching is **exact and case-sensitive**; `-aes256gcm-bound` does not end with `-aes256gcm`, so test the longer suffix first and test that no tag parses two ways.
  The bases are `raw`, `zstd`, `json`, `json-zstd` (feature-gated as today). Any other tag is `UnknownEncoding`, as today.
* The associated data is `b"localcache/seal/1" 0x00 <the full stored tag, bound suffix included> 0x00 <the path bytes>`. One function builds it; sealing and opening both call it. Seal and open with `aead::Payload { msg, aad }`.
* `encode_payload` must know the final tag before it seals (step 0: it picks the tag after): reorder, do not duplicate the tag logic. It gains the entry's `path: &str`; `decode_payload` too. Thread the path through the call sites step 0 listed: `engine.rs` `encode` (`set`, `batch_set`), `decode` (`get`, `get_if_fresh`), `decode_with` (`query/execution.rs`, both materialising tiers).
  The path is the canonical key string that `files.path` stores (step 0 § 3 showed it is the same at every site).
* After this commit: writes with a key are `Bound`; reads still accept `Legacy` (opened with empty associated data) and `None`. Nothing is refused yet.

### 1.2 R2: only bound entries are read when a key is set

* New `LocalFileCacheError::UnsealedEntry`, `#[cfg(feature = "encryption")]`, no payload. Its text names the remedy: the entry was not sealed with this engine's key and bound to its path (it is plain, or was sealed before v0.24.0); call `rotate_encryption_key` once with the same key, or purge the entry.
  Document the variant, add it to the error-handling documentation in rustdoc, and update the exhaustiveness tests the enum has.
* With a key: tag `None` or `Legacy` → `UnsealedEntry`; `Bound` → open with the associated data; a failure is `EncryptionError`, whose text becomes "decryption failed — wrong key, corrupted data, or the entry was moved from another path". Without a key: a sealed tag (either) is `MissingEncryptionKey`, a plain tag is read, as today.
* **The Tier 2 guard (step 0 C3):** a keyed engine never takes the SQL pushdown of field queries; it uses the decoding tier. The test: a keyed engine over plain `json` entries runs a field query with the plan of Tier 3 and returns `UnsealedEntry`; an unkeyed engine still uses Tier 2 (assert the tier through the existing explain or test hook).
* Check every consumer of a decode error that matches on variants (`run_report`, the batch reads, `preload`): say in the request what each does with `UnsealedEntry`.

### 1.3 R3: rotation seals everything, in pages, by path

* Rewrite `rotate_encryption_key` as RFC 029 R3 says: one `IMMEDIATE` transaction; rows read in **keyset pages by `(namespace, path)`** (`WHERE namespace = ?1 AND path > ?2 ORDER BY path LIMIT ?3`, served by `sqlite_autoindex_files_1`; **never by `file_id`**, step 0 C1); a page ends at `PAGE_ROWS = 1000` rows or `PAGE_BYTES = 8 MiB`, whichever comes first, always at least one row;
  each page is re-sealed and written before the next is read. The query selects `file_id`, `path`, the tag and the content for **every** row of the namespace and matches tags by the table of 1.1, not by `LIKE`. A `None` row is sealed as it is (bytes unchanged, the tag gains `-aes256gcm-bound`); a `Legacy` row is opened with the current key and empty associated data; a `Bound` row with the current key and its associated data; each is re-sealed under the new key bound to its path.
  `update_payload_content` writes the content **and** the tag. An unknown tag or any failure aborts the whole transaction; the cell is replaced only after the commit, as today. Returns the number of entries re-sealed. `new_key` equal to the current key is allowed.
* Tests: a test-only override makes the pages one row, and failure injection after a page, in the middle of a page and at commit leaves the database byte-identical and the engine on its old key; the existing concurrent-writer test (RFC 022 C2) still passes; plain, legacy and bound rows all end bound and read; the page boundary at exactly `PAGE_ROWS` rows.
* Measure at 1,000,000 entries as in step 0 § 8 (64-byte payloads is enough) and report the peak memory and time against the step-0 numbers (about 8 MiB and 1.6 s).

### 1.4 R2, import

`import_bounded` (it serves `import_entries` and `import_from`): with a key, if any record's tag is not `Bound`, refuse the whole import **before writing**, `InvalidArgument { argument: "records" }`, the text naming the remedy (import into an engine without a key, then rotate). It checks the tag, not the authenticity. Without a key, nothing changes (records are stored verbatim). `export_entries` is untouched.

### 1.5 The fixture and the upgrade test

A committed fixture written by the **published v0.23.1** with a key: `crates/localcache/tests/fixtures/compat-v0_23-encrypted.sqlite3`, following the pattern in `tests/fixtures/README.md` and `generators/` (the generator, its pinned build, and the digest test in `fixture_integrity.rs`). Contents: one namespace per tag variant (`raw`, `zstd`, `json`, `json-zstd`, each sealed, 3 entries), one namespace of plain entries.
The test of the RFC's plan: the v0.24.0 keyed engine refuses each sealed-without-binding and each plain entry with `UnsealedEntry`; `rotate_encryption_key` with the same key reports the right count; every entry then reads to its expected value; a second rotation to a new key works; a keyless engine gets `MissingEncryptionKey`.
Also: the failing-before tests of the RFC plan (swap, tag change, downgrade) first fail on the v0.23.1 tree — show the output — and a property test: for random path, tag, key and plaintext, open(seal(x)) = x, and a change of path, tag, key or one bit of ciphertext fails.

## 2. Measurements

`set` and `get` with encryption on the final tree against the v0.23.1 tag, a 64-byte and a 1 MiB payload, alternating arms, at least five rounds, `/proc/loadavg` at the start of each: the difference as a number with its spread (step 0 found +0.19 µs, +3%, on a 64-byte `get`). Nothing in the book states a number that this does not support.

## 3. Gates and the request

All gates of Q14 § 4, plus `gate-docs` and `gate-archive` on the committed tree, per commit where it is cheap and on the last at least. One review request `.git-exclude/review-request/NNN-dev-q15-<date>.md`: per commit what changed and why, the failing-before output, the consumers of `UnsealedEntry`, the measurements, the gates, and anything you found untrue in the RFC (the architect corrects it).

## 4. Not yours

The book, the CHANGELOG, `migration.md`'s guarantee scoping, the version, the date, `ROADMAP.md` (Q16 and the architect), any change to the CLI, the schema, or the release workflow.
