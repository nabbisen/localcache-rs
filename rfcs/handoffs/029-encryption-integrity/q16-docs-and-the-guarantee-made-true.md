# Q16 Handoff — The Book, the CHANGELOG and the Guarantee, Made True (RFC 029 R6, Compatibility; v0.24.0)

RFC: `rfcs/accepted/029-encryption-integrity.md`. Starts after Q15 is **committed locally** (it rewrites text about code that must exist). Order: Q15 → **Q16** → v0.24.0 release preparation.
Format of the review request: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6. **Local commit; do not push.**
Authorship: high-capability model. Authorized: the owner (RFC 029, Decision 1).

## 1. The change

1. **`docs/src/migration.md`.** The section "Announced: encryption integrity (v0.24.0)" becomes the real section "Encryption integrity (v0.24.0)", in present tense: what changed, who must act and how (`rotate_encryption_key` once with the same key; or purge), the one-way door for encrypted entries, **do not rotate with an older version once v0.24.0 has written**, what is not protected. And **the wire-format guarantee is scoped in place** (Decision 1): it is about the payload codec bytes and holds for every entry except a sealed entry read by an engine with a key, which needs the one rotation; state it in the guarantee's own paragraph, not as a footnote, and update "What could break this guarantee" and "How it is enforced" (name the new fixture).
2. **`docs/src/security.md`.** The table of an attacker who can write the file but has no key (cannot read, forge, move an entry to another path, downgrade to plain, change how a sealed entry is decoded; **can** delete entries, restore an older valid ciphertext of the same row, move a payload between namespaces under one key, read all metadata); the sentence "does not yet insist" and "Authentication does not bind a payload to its path" become true statements of v0.24.0; the nonce limit (NIST SP 800-38D, 2^32 encryptions per key, about 50 days at 1,000 writes per second; rotation starts a new key); "one key per trust domain". Re-read the whole page.
3. **Other pages.** `architecture.md` (the encoding tags: the five places step 0 found), `errors.md` (`UnsealedEntry`: when, why, the remedy), `builder.md` and `cookbook.md` and `api.md` wherever encryption or `rotate_encryption_key` is described (the upgrade, the paged rotation, that rotation seals plain entries too and can switch encryption on), `querying.md` (a keyed engine never uses the SQL pushdown), `portability.md` (a keyed import accepts only bound records; export is verbatim), `performance.md` (the measured cost from Q15 § 2, with its limits, no promise).
   Search first: `git grep -n -i "aes256gcm\|encrypt\|rotate_encryption_key\|does not bind\|not bound\|plain" -- docs/src README.md crates` and report the final output; each remaining old statement must be justified or fixed.
4. **`CHANGELOG.md`.** A `## [Unreleased]` section above `## [0.23.1]` with **Breaking** entries, each written so it can be listed in a migration summary: a keyed engine reads only bound entries (`UnsealedEntry`); `rotate_encryption_key` seals every entry and is paged; a keyed import accepts only bound records; a keyed engine never uses the SQL pushdown; the new tags and the one-way door; the new error variant; the changed `EncryptionError` text. The release preparation turns it into `[0.24.0]` with an upgrade list.
5. **`README.md`** only if it states what encryption guarantees. **Nothing else:** not the version, not the date, not `ROADMAP.md`, not `docs/src/roadmap.md`.

## 2. Gates and the request

As in Q15 § 3 (all gates, `gate-docs` and `gate-archive` on the committed tree), plus the search above. One review request. Re-read each page you touch in full and say what else you found untrue.

## 3. Not yours

The library code (Q15; if you find a behaviour wrong, stop and file a design request), the version, the date, the roadmap pages.
