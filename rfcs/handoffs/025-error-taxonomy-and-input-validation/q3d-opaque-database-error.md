# Q3d Handoff — Opaque `DatabaseError` (RFC 025 R7), first slice of v0.22.0

RFC: `rfcs/accepted/025-error-taxonomy-and-input-validation.md`, Part B, **R7**.
Scope: authorized by the owner on 2026-10-29 (ROADMAP, Phase 24, "v0.22.0 scope"). Order for the dev
team: **Q9a → Q9b → Q9c (RFC 027, which must be pushed to be verified in CI) → this slice (Q3d) → Q1c →
Q3b → …**. Start this slice only when the architect has accepted Q9c, so that no v0.22.0 commit sits
below a push. Within v0.22.0 it goes first so that the `rusqlite` bump (Q1c) is invisible to callers
except through `links`.
Format of the review request: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6.
Authorship: high-capability model. Authorized: the owner.

## 0. What this is

v0.22.0 is a breaking release, and this is its first breaking change. Today
`LocalFileCacheError::Database(#[from] rusqlite::Error)` (`crates/localcache/src/error.rs:46`) makes
`rusqlite::Error` part of the public API, in two ways: the variant's payload, and the public
`impl From<rusqlite::Error> for LocalFileCacheError` that `#[from]` generates. After this slice **no
`rusqlite` type appears in any public signature or public trait impl.**

Two principles govern every judgement call: *"Finally clean, safe and secure, robust and sophisticated
design"*, and *public material must not confuse or mislead users*. If a step seems to need a new public
item that this file does not name, **stop and file a design request**.

## 1. The change

In `crates/localcache/src/error.rs`:

1. A public, opaque `DatabaseError`:
   - holds the underlying error type-erased (`Box<dyn std::error::Error + Send + Sync + 'static>`) and
     the raw SQLite extended result code when there is one (`Option<i32>`);
   - fields private; no public constructor; `Debug`, `Display` and `std::error::Error`;
   - **`Display` prints exactly what `rusqlite::Error`'s `Display` printed**, so
     `database error: …` messages do not change;
   - `Error::source()` returns the underlying error (type-erased);
   - `pub fn sqlite_extended_code(&self) -> Option<i32>`: the extended code of a
     `rusqlite::Error::SqliteFailure`, `None` for every other kind (`QueryReturnedNoRows`, conversion
     errors, …). Document, in its rustdoc, that the primary result code is `code & 0xff`, with the
     `SQLITE_BUSY` (5) example, so a caller can recognize a busy database without `rusqlite`.
2. The variant becomes `Database(DatabaseError)`. Keep the display `database error: {0}` and make the
   field the variant's `#[source]`, as the `#[from]` did.
3. **`#[from]` is removed.** The conversion from `rusqlite::Error` is a crate-private constructor
   (`pub(crate)`), not a trait impl: a trait impl would be public. Everything that relied on `?`
   converting a `rusqlite::Error` now converts explicitly.

   The mechanism is yours to choose, and the compiler lists every site. Recommended: a crate-private
   extension trait on `Result<T, rusqlite::Error>` with one method, or one `pub(crate)` function used as
   `.map_err(...)`. Report which you chose, the site count, and why. **The diff must be mechanical**:
   no behaviour change at any of those sites.
4. Nothing else in `LocalFileCacheError` changes in this slice. Q3b changes the other variants.

`DatabaseError` is exported at the crate root next to `LocalFileCacheError` (`lib.rs` already re-exports
the error type). Add it to `crates/localcache/tests/api_surface.rs`'s expectations if that file lists
exports.

## 2. Tests (failing before)

1. **The extended code.** Provoke `SQLITE_BUSY` through the public API (two engines on one file, one
   holding a write transaction — the existing busy tests in
   `crates/localcache/src/cache/engine/tests.rs` near line 93 and
   `crates/localcache/src/db/schema/tests/atomicity.rs` near line 133 do this). Assert
   `error.sqlite_extended_code().map(|c| c & 0xff) == Some(5)`. The two existing tests that match on
   `rusqlite::Error::SqliteFailure` move to this API; inside the crate they may keep a `rusqlite`
   assertion beside it. On the base the method does not exist, so the test fails to compile: say so.
2. **`source()` and `Display` are preserved.** For a known failure, `to_string()` equals the old text, and
   `source()` is `Some`.
3. **No `rusqlite` in the public surface.** After `RUSTDOCFLAGS="-D warnings" cargo doc -p localcache
   --all-features --no-deps --locked`, `grep -ril rusqlite target/doc/localcache` lists only pages whose
   text mentions the library in prose (list them and quote each match). In particular, the
   `LocalFileCacheError` and `DatabaseError` pages show **no** `From<…rusqlite…>` impl and no
   `rusqlite::Error` in a signature. Paste the grep.
4. **`Send + Sync`.** If no test asserts `LocalFileCacheError: Send + Sync + 'static`, add one. The type
   erasure must keep it so.
5. Every existing test passes. Only assertions that name `rusqlite::Error` through
   `LocalFileCacheError::Database(…)` may change (`read_only_contract.rs:344` matches `Database(_)` and
   should not need to).

## 3. Documentation

- `docs/src/errors.md`: describe `Database(DatabaseError)`, the extended code, and how to recognize a
  busy database without `rusqlite`. Re-read the whole page; fix anything else that is no longer true and
  say so.
- Rustdoc for `LocalFileCacheError::Database`: no `rusqlite` link or type.
- The `compile_fail,E0004` block in `error.rs` keeps working (`Database(_)`).
- `docs/src/dependency_security.md` is **Q1c's**, not this slice's.
- `CHANGELOG.md`, under `## [Unreleased]`, a `### Changed` entry marked **breaking**:
  `LocalFileCacheError::Database` now wraps the opaque `DatabaseError`; `rusqlite::Error` is no longer
  part of the public API; use `sqlite_extended_code()`; how to migrate a `match` on the old payload.
  Create the `[Unreleased]` heading if it is absent (it was renamed `[0.21.6]` at the release).

## 4. Gates and the request

- `cargo fmt --all --check`; `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  (the `unwrap_used` lint is on); `python3 scripts/feature_matrix.py --check-coverage` and `--run-all`;
  `cargo test --workspace --all-features --locked` (report the count: 532 before, so expect more);
  `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked`;
  `python3 scripts/source_integrity.py --require-tracked`; the compat fixtures
  (`crates/localcache/tests/compat.rs`) pass **unmodified**.
- The working tree holds exactly this slice when you file. File the request as
  `.git-exclude/review-request/NNN-dev-q3d-opaque-database-error-<date>.md` (next free number),
  describing the **uncommitted** tree. Commit only after the review approves it. Do not push, tag, bump
  the version or touch `ROADMAP.md`. The v0.22.0 slices accumulate as **local commits** and are pushed
  once, with the release preparation, as for v0.21.5: the book deploys from `main` on every push and must
  not describe v0.22.0 before it is released. (The architect will say if a slice needs an earlier push.)
- List every required step, including the ones that pass; report every judgement call.

## 5. Not yours

No other variant change (Q3b); no `rusqlite` version change (Q1c); no `UnsupportedFeature` edit; no
`DatabaseState`/`SchemaNotCurrent` (Q3b); no MSRV change.
