# Q35 — Make `main` green again (rustix), hold `rusqlite 0.42` with a reason, and study it

Review 115 (request 097). CI on `main` has been red since RUSTSEC-2026-0486 was added to the database (`rustix` 1.0.0 to 1.1.5, `unsound`: six `xattr` functions mark unwritten bytes as initialized for a zero-length buffer; patched `>= 1.1.6`; I read the advisory file myself). The locked `rustix` is 1.1.4. **Part 1 is authorized now; part 2 is authorized; part 3 is read-only.** Format of the review request: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6.

## 1. `Cargo.lock`: `rustix` 1.1.4 → 1.1.6 (one commit, now)

1. `cargo update -p rustix` and nothing else. Report the lockfile diff in full (expected: `rustix` only; if anything else moves, **stop and report**). `rustix 1.1.6` declares Rust 1.65.
2. Evidence that it is safe, in proportion: `cargo make gate-deny` exit 0 with `ignore = []` (no exemption is added: the owner's rule is that an advisory is fixed, not excused); `gate-test`, `gate-clippy` and `gate-msrv` (1.89.0); the callers: say whether anything in our tree calls an `xattr` function (the dev's request 097 did not grep): `cargo tree -i rustix -e all` and a `git grep -n "xattr"` over `crates/` and, if cheap, over the vendored sources of `rustix`'s dependents that you can list from the lock (`notify`, `tempfile`, `async-io`/`polling`/`async-process`): report what you find or say you did not look.
3. A plain `git push`; CI and Docs green. Message: `deps: rustix 1.1.4 to 1.1.6 (RUSTSEC-2026-0486, unsound xattr)`. No version, no CHANGELOG release section; add a line under a new `## [Unreleased]` → *Dependencies* only if the architect later cuts a release (do not create the section now).

## 2. The held row for `rusqlite` (one commit, after part 1 is green)

`rusqlite 0.42.0` (2026-11-11, Rust 1.89, requires `libsqlite3-sys 0.40.0`) is out and `gate-requirements` correctly reports `rusqlite ^0.41.1` (request 097 § 3). Add this row to `requirements-held.toml`, `latest = "0.42.0"`, and regenerate the book table (`python3 scripts/check_requirements.py --write-book`):

* `crate = "rusqlite"`, `requirement = "^0.41.1"`.
* `why`: "rusqlite 0.42 requires libsqlite3-sys 0.40. Because libsqlite3-sys declares `links = "sqlite3"`, a change of rusqlite line forces every crate that depends on rusqlite directly to move with localcache, so it is made in a minor release with an upgrade list. Nothing in 0.41.1 is a security problem: it bundles SQLite 3.54.1, as 0.42 does."
* `ends`: "The release that adopts rusqlite 0.42 (a minor release), once the study of request Q35 part 3 is accepted."

`cargo make gate-requirements` live must print `OK (… 1 held …)`. A plain `git push`; CI and Docs green.

## 3. Study `rusqlite 0.42.0` (read-only; nothing committed)

Report in the review request: the upstream changelog / release notes for 0.42.0 and `libsqlite3-sys` 0.40.0 (what changed, which APIs `localcache` uses are affected: `git grep -n "rusqlite::" crates/localcache/src | wc -l` and the specific types); the bundled SQLite version of `libsqlite3-sys 0.40.0` (read `SQLITE_VERSION` and the `sqlite3WalReadFrame` form from the published source, as in request 095); whether the compile options the book lists (FTS, R-Tree, STAT4, …) changed (read the build script); and, **in a scratch copy of the tree**, `rusqlite = "0.42"`: does it build on 1.89 and 1.89 only, how many source edits, does the whole suite pass, and do the re-prepare counts of request 092 still hold (the `CAST` workaround). Then your recommendation: patch or minor (the architect expects minor, because of `links`), and whether anything in 0.42 is worth having for localcache itself, or whether adopting it is only following upstream.

## 4. Not yours

A release, a version, a tag, posting anywhere, `ROADMAP.md`, the adoption of `rusqlite 0.42` in the committed tree.

## 5. Update after the owner's answer (2026-11-13)

The owner said the move is acceptable: the one known direct user of `rusqlite` (orbok, six crates) can be asked to update together with us, so the "announce one release ahead" step is replaced by telling orbok directly before the release. Consequences for this task: the row above is worded without the announcement (done); part 3 now decides **whether the study is clean**, not whether to wait: if it is, the architect plans a minor release (v0.27.0) that adopts `rusqlite 0.42`, includes the `rustix` lock, deletes the row, and carries the upgrade list ("a crate that depends on `rusqlite` directly must use 0.42 or later; `libsqlite3-sys` 0.40"). The reason to follow upstream, recorded: v0.26.0 had to be a rushed breaking security release because we were one `rusqlite` line behind when the CVE fix appeared; staying current makes the next such fix an ordinary release. Part 3 therefore adds: the tests for the `links` text in `dependency_security.md` that would need updating, and the list of book pages that name `rusqlite 0.41` or `libsqlite3-sys 0.39` (`git grep`), as a to-do for the release preparation. Still nothing is adopted in the committed tree by this task.
