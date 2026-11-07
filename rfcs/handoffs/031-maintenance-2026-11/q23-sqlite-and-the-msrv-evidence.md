# Q23 Handoff — Evidence for the SQLite / MSRV decision (measurement only)

Review: `.git-exclude/reviewed/100-architect-q20-q21-review-and-plan-2026-11-07.md` § 2. Format of the review request: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6. **No tracked file changes**; everything under `.git-exclude/tmp/`, prototypes in a scratch worktree of `origin/main`. Authorship: high-capability model.

The architect verified CVE-2026-119375 against the NVD (SQLite before 3.54.0; a crafted database plus a WAL file with an undersized page size makes queries return uninitialized page-cache bytes; CVSS 4.0 6.8, 3.1 5.5; local attack vector). Request 075 § 3 shows the extension-table, view and trigger routes are closed by the schema classifier. What is not established is whether this crate is affected in practice, and what the lowest toolchain is that takes the fixed SQLite.

## 1. What to establish, each with command, output and exit code

1. **Reproduce or refute CVE-2026-119375 on our build.** The public analysis is at `https://abraxaslabs.tech/research/sqlite-wal-pagesize`, the fix at `https://github.com/sqlite/sqlite/commit/e5bf119821f25be1d7f10ef0a2edbe175289166e`, the bug report at `https://sqlite.org/bugs/info/2026-08-17T15:54:41Z`. Read them; build a hostile database and WAL pair as they describe, in a scratch directory, and open it with **this tree** (read-write and read-only) and run the engine's own operations (`get`, `list_entries`, a query, `export_entries`). Does anything return bytes that are not stored data? Does the schema classifier, which runs first, refuse the file before any read? Report what happened, with the bytes. If you cannot reproduce it, say exactly what you tried and what is missing; do not claim it is harmless.
2. **The same pair with the fixed SQLite.** Open it with a scratch build of `rusqlite 0.41.1` (bundled 3.54.1) on a toolchain that builds it. The comparison is the evidence.
3. **The lowest toolchain that builds the fixed line.** `rusqlite =0.41.0`, `0.41.1` and `libsqlite3-sys 0.39.x` with this workspace on `rustc 1.86`, `1.87`, `1.88`, `1.89`: which builds the workspace (`cargo check --workspace --all-features --locked` equivalent; the crate declares 1.89, the release notes of 0.41.0 say 1.88). Report the lowest that builds and passes the test suite, and the first error where it does not.
4. **What the raise would let us update** (change nothing): with MSRV 1.88 and with 1.89, which of `sha2 0.11` (1.88), `criterion 0.8` (1.86), `rusqlite 0.41` (1.89), `libsqlite3-sys 0.39` (1.88) become possible; the API changes of `rusqlite 0.41` this tree must absorb (compile a scratch edit, list every error, estimate the size), and whether the bundled-build change in 0.41 (`SQLITE_ENABLE_MEMORY_MANAGEMENT` removed) matters to us.
5. **The compile options our build sets that we do not use** (FTS3/4/5, R-Tree, STAT4, DBSTAT, SOUNDEX, `LOAD_EXTENSION`): can any be turned off for the `bundled` build we use without a fork (a rusqlite or libsqlite3-sys feature, or an environment variable)? A smaller build reduces the exposure to every future CVE in those extensions. Read the build scripts of 0.38.2 and 0.39.1.
6. **`aes-gcm 0.11.1` (request 075 § 5), as design:** the changes the sealing code needs (`OsRng`, `generate_nonce`, `Nonce::from_slice`), the change in the dependency tree (`rand_core`, `universal-hash`, `subtle`), the nonce source (which RNG, from which OS interface), and a plan that proves the ciphertext format unchanged: decrypt every committed encrypted fixture and every sealed vector with the new line, seal with the new line and open with the old, and the reverse. Prototype in a scratch worktree; do not commit.

## 2. The report

One request, each of 1 to 6 answered with evidence, and **your recommendation** on whether the CVE reaches this crate in practice (from item 1) and on the lowest MSRV (item 3). You report; the architect and the owner decide.

## 3. Not yours

Any tracked-file change; the MSRV; the version.
