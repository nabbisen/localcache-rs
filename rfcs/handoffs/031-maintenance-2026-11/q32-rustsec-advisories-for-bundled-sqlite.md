# Q32 — Make the RustSec advisories ready to submit: `libsqlite3-sys`, and the other crates the `rusqlite` maintainer named

The `rusqlite` maintainer answered on `rusqlite/rusqlite#1945` (2026-11-08 14:31 UTC, saved verbatim at `.git-exclude/upstream/rusqlite/receive/issue-1945-gwenn-reply-2026-11-08.md`): "I am fine if you send the RustSec", and: "you can do the same for" `stainless-steel/sqlite3-src`, `udoprog/sqll` (`sqll-sys/source`), "...". That is the upstream confirmation RustSec's `CONTRIBUTING.md` asks for (request 086 § 3), so the two-week wait is not needed for `libsqlite3-sys`. **You still send nothing.** The owner decides who submits (the architect posts what the owner authorizes).
Format of the review request: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6.
Authorship: high-capability model.

## 1. `libsqlite3-sys`: bring the draft up to the day of submission

1. Re-run, read-only, the three feed checks of request 088 § 1 and the existence check in `rustsec/advisory-db` (open and closed pull requests and issues by CVE id and by `libsqlite3-sys`): has anything changed since 2026-11-08? Has `libsqlite3-sys` published a release after `0.39.1` (crates.io)? If so, read its bundled version from the header and update the table and the `patched` range only if the header says so.
2. Update `.git-exclude/tmp/q29/RUSTSEC-draft-libsqlite3-sys.md`: `date` = the day the owner submits (leave a marker the architect replaces with the system date); keep everything else that request 086 § 4 established. Do not add claims.
3. Write the **pull request description** (a scratch file next to it): what the advisory is, the link to the maintainer's confirmation (the comment URL above), the evidence in two lines (the header and `sqlite3WalReadFrame` check of request 086 § 1, reproducible with one `grep -c`), and that the maintainer of `rusqlite` agreed. No exploit detail, nothing about `localcache`.
4. Write, as a list of exact commands for whoever submits, the steps to submit: fork `rustsec/advisory-db`, a branch, the file path RustSec expects (`crates/libsqlite3-sys/RUSTSEC-0000-0000.md`), `gh pr create` against `main`. Do not run any of them.

## 2. The crates the maintainer named: evidence first, then drafts only if the evidence supports one

For `sqlite3-src` (stainless-steel), `sqll-sys` (udoprog), and **any other crate you find** (crates.io search for crates that bundle SQLite source: for example `sqlite3-src`, `sqll-sys`, `libsql-ffi`, `sqlite-wasm-rs`, `rusqlite-sys`-like names; list what you searched and what you found, with names only for the ones you do not examine):

* which of its releases contain a SQLite amalgamation (the crate source on crates.io, read as text: nothing built or run), which SQLite version each bundles (`SQLITE_VERSION`), and whether `sqlite3WalReadFrame` in it has the unfixed pattern (`nOut>sz ? sz : nOut` without the `memset(pOut+sz, 0, nOut-sz)`), exactly as in request 086 § 1, for the releases from the line that bundles SQLite before 3.54.0 up to the first that bundles 3.54.0 or later (or to the latest if none does yet);
* whether the crate is maintained (last release date, repository activity) and whether it already has a RustSec advisory or an open issue about this CVE (search);
* the **patched range** if there is one, or "none yet" if every release is affected.

Draft one advisory file per crate in the same format, **only where releases are affected and the evidence is complete**; where a crate has no affected release, or the evidence is incomplete, say so and do not draft. For a crate with no patched release, say what RustSec's format does for "no patched version" (read the template and two examples) and flag it for the owner: submitting an advisory with no fix is a judgement call.

## 3. The request

One review request: § 1 (what changed, the PR description, the commands), § 2 (the table per crate, the drafts' paths and full text, what you searched), and anything that makes any of this unsuitable for RustSec. Nothing is committed or sent.

## 4. Not yours

Submitting, forking, commenting anywhere, contacting the other crates' maintainers, any advisory for `localcache`, `ROADMAP.md`.
