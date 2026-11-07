# Q27 — A draft RustSec advisory that maps CVE-2026-119375 to `libsqlite3-sys`

Authorized by the owner on 2026-11-07 (reviews 101 to 106). **You draft; the owner reviews and submits from their own account. You do not open the pull request, and you send nothing to RustSec, GitHub or anyone.** The reason: step 0 of Q25 (request 081) found that neither RustSec, OSV nor GitHub's reviewed database lists the CVE against any crate, so `cargo audit` and `cargo deny` users never learn that a `libsqlite3-sys` that bundles SQLite before 3.54.0 has the flaw. An advisory against `libsqlite3-sys` reaches every dependent, including this project's users.
Format of the review request: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6.
Authorship: high-capability model.

## 1. Evidence first (read-only; send nothing but a name, a version or a CVE id)

1. **Which `libsqlite3-sys` versions bundle which SQLite.** From the crate sources (the `SQLITE_VERSION` of `sqlite3/sqlite3.h`, as `scripts/check_requirements.py` reads it), for every release from the line that bundles SQLite below 3.54.0 up to the first that bundles 3.54.0 or later: the version, the bundled SQLite, the publication date. The patched range is the first `libsqlite3-sys` version whose header says 3.54.0 or later; do not infer it from release notes. Report the table.
2. **Is the flaw in the bundled build at all?** The CVE concerns a WAL page-size mismatch. Request 078 § 3 (the code evidence from both SQLite versions) says the pattern is in the bundled 3.53.2 and zero-filled in 3.54.1. State the evidence you rely on in two sentences and how a reader can check it. Do not build or run any crafted file (the earlier withdrawal of that request stands).
3. **What RustSec expects.** Read RustSec's advisory-db `CONTRIBUTING`/template (public): the TOML front matter fields (`id` left as `RUSTSEC-0000-0000`, `package`, `date`, `url`, `categories`, `keywords`, `aliases`, `cvss`, `[versions] patched = [...]`, `[affected]` if needed), the markdown body, and how an advisory for a crate that **bundles** C source is conventionally written. Find two existing examples for `-sys` crates and say which you followed. Report anything that makes this report unsuitable for RustSec (for example a policy that vulnerabilities in the bundled C library are reported upstream first), so the owner can decide before submitting.

## 2. The draft

One file, `.git-exclude/tmp/q29/RUSTSEC-draft-libsqlite3-sys.md` (scratch; not committed, not in the repository), in RustSec's format:

* `package = "libsqlite3-sys"`, `aliases = ["CVE-2026-119375", "GHSA-72w2-rjgg-q729"]` (the GitHub advisory exists, unreviewed, and carries that id; confirm it), `categories = ["memory-exposure"]` or the closest category the template lists, `cvss` as NVD gives it (v4 `AV:L/AC:L/AT:N/PR:N/UI:P/VC:H…`: copy the vector exactly from the CVE record), `url` of the upstream record, `[versions] patched` from item 1.
* The body, plain: what is affected (a process that opens a database file, with its `-wal`, that a less trusted party could have written; the bundled build, not a system SQLite); what to do (upgrade `libsqlite3-sys`/`rusqlite`; `rusqlite 0.41` needs Rust 1.89); that a system-library build (`libsqlite3-sys` without `bundled`) is not affected by this crate's bundled copy. No exploit detail, nothing about how to build the file.
* **Not a claim about `localcache`** beyond one neutral line if RustSec's format has a place for dependents; do not name the project as the reporter's product.

## 3. The request

One review request: the table of item 1, the evidence of item 2, the findings of item 3 (with links), the path of the draft and its full text. Nothing is committed.

## 4. Not yours

Submitting, contacting RustSec, the `libsqlite3-sys` or `rusqlite` maintainers, GitHub, or NVD; any advisory for `localcache` itself (not decided; the owner has asked only for the mapping to `libsqlite3-sys`).
