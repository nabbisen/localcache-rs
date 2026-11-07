# Q24 Handoff — Say what is true about the bundled SQLite (documentation only, for v0.25.1)

Review: `.git-exclude/reviewed/101-architect-q22-q23-review-2026-11-07.md`. Format of the review request: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6. **Local commits; do not push.** Authorship: high-capability model.

## 1. `docs/src/security.md`: a known issue, stated as your request 078 § 1 found it

Add to "What a hostile database file can do" (or where the page keeps limits) a paragraph "**Known issue: the bundled SQLite (v0.25.x)**". It says, in plain words and no more than is established:
* the SQLite that `rusqlite 0.40` bundles in this release is 3.53.2; **CVE-2026-119375** (published 2026-10-30, fixed in SQLite 3.54.0) lets a crafted database together with a sibling `-wal` file make ordinary queries return uninitialized page-cache memory; its CVSS is 6.8 (v4) / 5.5 (v3.1) with a local attack vector;
* it concerns a process that opens a database file, with its `-wal`, that a less trusted party could have written: the CLI's `copy --from-db` and any import of a foreign file, a cache in a shared directory, a cache restored from an untrusted backup. A cache that only your own user can create is outside the model;
* **what to do now:** keep the cache directory writable only by the user of the cache (the page's recommendation 1), and do not open a database file, or its `-wal`, that someone less trusted could have written;
* **the fix** is SQLite 3.54.x, which needs `rusqlite 0.41`, which declares Rust 1.89, above this project's MSRV (1.85); the MSRV policy (RFC 023 R3.1) provides for a raise for a security fix; the page is updated in the release that makes it. Do **not** name a version or a date for that release.
* what the classifier does and does not do: it refuses a database file with any application object we did not create (virtual tables, triggers, views), which closes the extension-table and view CVEs of 2026; it reads through SQLite's pager, so it does not protect against a flaw at the page level.
Do not describe how the file is built. Re-read the whole page and say what else is now untrue.

## 2. `docs/src/dependency_security.md`: the unused extensions

A short subsection "SQLite compile options we do not use". The bundled build compiles in FTS3/4/5, R-Tree, STAT4, DBSTAT, SOUNDEX and load-extension support; this crate uses none of them (only core SQL and the JSON functions, which are in the core since SQLite 3.38). Say that **a program that builds its own binary** can remove them with the environment variable `LIBSQLITE3_FLAGS` (the seven `-U…` words of your request 078 § 5, copied exactly), that the workspace's own suite (742 tests) passes with them removed, that the probe binary is 22% smaller, and that this reaches only the build where the variable is set: not a user who depends on `localcache` from crates.io unless they set it in their own build. Say plainly that it does nothing for CVE-2026-119375, which is in the core. Add: "your own program may use FTS or R-Tree through `rusqlite` in the same binary; check before you remove them".

## 3. Gates and the request

All gates on the committed tree (`gate-msrv` as `rustup run 1.85.0 cargo make gate-msrv`). One short request: the commit hashes and what else you found untrue. Housekeeping approved with exact paths: `q22/target-*`, `q22/fuzz-target`, `q23/target-*`, and `git worktree remove` for `q22/base`, `q23/wt`, `q23/wt2`, `q23/wt3`.

## 4. Not yours

The version, the CHANGELOG, the MSRV, `rusqlite`, any announcement of removals.
