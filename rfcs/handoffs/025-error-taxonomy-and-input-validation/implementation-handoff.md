# RFC 025 Implementation Handoff — Part A: One Expiry Rule (Q3a, v0.21.5)

RFC: `rfcs/accepted/025-error-taxonomy-and-input-validation.md` (accepted 2026-09-24; all four owner
decisions as recommended)
Milestone: Phase 24 **Q3a**, shipping in v0.21.5. Part B (Q3b–Q3e, v0.22.0) gets its own handoff
after v0.21.5 ships.
QA companion: `rfcs/handoffs/025-error-taxonomy-and-input-validation/acceptance-qa-checklist.md`

## 0. What this is

**A correctness fix. It is not breaking.** Two defects come from two sites doing TTL arithmetic
differently, each with a signed/unsigned `as` cast (RFC 025, "Two TTL defects (reproduced)"):

1. **A backward clock step expires fresh entries.** In `is_expired`
   (`crates/localcache/src/cache/engine.rs`, around line 1299),
   `now.saturating_sub(updated_at) as u64` wraps a negative age to about 1.8×10¹⁹. With TTL 3600 s
   and a stored `updated_at` one second in the future, `get_if_fresh` returns `None` and
   `check_status` returns `Stale`.
2. **`explain` contradicts itself for a huge TTL.** In `crates/localcache/src/cache/engine/diagnose.rs`
   (around line 70), `ttl.as_secs() as i64` wraps `Duration::MAX` to −1. The result is
   `ttl_remaining_secs: Some(0)` while `status` is `Fresh`.

The architect reproduced both end to end, against the crate:
`.git-exclude/tmp/rfc025-ttl/e2e/src/main.rs`, with its output in `run.log` and `run2.log`. Reuse that setup for
your failing-before.

**No public signature, no error variant, no rejection.** Nothing that expires correctly today may
change.

The two standing principles from the owner:
- **"Finally clean, safe and secure, robust and sophisticated design."**
- **Public APIs must not confuse or mislead users.**

## 1. The change

In `crates/localcache/src/cache/engine.rs`, next to `is_expired`:

```rust
/// Seconds an entry has left before its TTL expires, or `None` when there is no TTL.
/// The one place TTL arithmetic happens (RFC 025 R1).
///
/// Age is clamped at zero: an `updated_at` in the future (the clock stepped back)
/// counts as age 0, never as a wrapped huge value. Everything saturates.
pub(crate) fn ttl_remaining(now: i64, updated_at: i64, ttl: Option<Duration>) -> Option<u64>
```

1. Compute the age as `now.saturating_sub(updated_at).max(0)`, converted to `u64` without an `as`
   cast between signedness. The value is non-negative by then, so use `u64::try_from(..)`, and
   state why it cannot fail, or use `unsigned_abs` on the clamped value. The remaining time is
   `ttl.as_secs().saturating_sub(age)`.
2. `is_expired(updated_at, ttl)` becomes
   `ttl_remaining(repository::now_secs(), updated_at, ttl) == Some(0)`. Keep its signature and all
   three callers (`get_if_fresh`, `check_status`, `cleanup_expired`) unchanged.
3. In `diagnose.rs`, `ttl_remaining_secs` comes from `ttl_remaining(…)`, converted to the public
   `Option<i64>` with **saturation at `i64::MAX`** (`i64::try_from(r).unwrap_or(i64::MAX)`). The
   expired flag is `ttl_remaining(…) == Some(0)`, the same expression `is_expired` uses. Read `now`
   **once** per `explain` call, and pass it to both.
4. **No other `as` cast between signed and unsigned** may remain on the TTL path. Show
   `git grep -n " as u64\| as i64" -- crates/localcache/src/cache/engine.rs crates/localcache/src/cache/engine/diagnose.rs`
   before and after in the request, and explain each remaining hit (none should be on the TTL
   path).

A future `updated_at` is treated as age 0, so the entry is fresh. That is the RFC's decision. Do
not make it `Stale` "to be safe": that would reintroduce mass expiry on every clock step.

## 2. Tests (reproduce first)

Write these and run them against the **unfixed** code (the Q2b commit), capturing the failures.
Then fix. Put the engine-level cases in `crates/localcache/tests/storage.rs` or
`crates/localcache/tests/core.rs`, wherever the existing TTL tests live. `ttl_remaining` gets unit
tests in the engine's own test module.

| Test | Before (must fail) | After |
|---|---|---|
| TTL 3600 s; stored `updated_at` set to now + 1 through `rusqlite`, as the probe does | `get_if_fresh` is `None`, `check_status` is `Stale` | `Some` / `Fresh` |
| the same entry, with `cleanup_expired()` | deletes it | returns 0; the entry remains |
| the same entry, with `explain()` | `status` is `Stale` while `ttl_remaining_secs` is `Some(3601)`: it contradicts itself (architect, `run2.log`) | `Fresh`, with `ttl_remaining_secs` = 3600 |
| TTL `Duration::MAX`, a fresh entry, `explain()` | `ttl_remaining_secs == Some(0)` | a large positive value, never 0 |
| `ttl_remaining` unit tests: at, one second before, and one second after the TTL, for 1 s and 3600 s; `updated_at = i64::MIN`, `i64::MAX`; `now = i64::MIN`; `Duration::MAX`; no TTL | — | exact expected values, and no panic, in debug and release |

The boundary rows are regression guards and may pass before. Say which rows failed before, as the
project always does.

Run the unit tests in **debug** mode too. Overflow checks are on there, so a remaining `as` or an
unguarded subtraction panics instead of wrapping.

## 3. Documentation and CHANGELOG

- The rustdoc on `CacheOptions::ttl` and the builder's `ttl`: one sentence saying age is measured
  from the entry's last write, and that a clock stepped back does not expire entries. **Do not**
  document R8's one-second resolution or R6's rejection here. Those are v0.22.0's (Part B).
- `CHANGELOG.md`, `## [Unreleased]` → `### Fixed`: the two defects, each in one sentence, as user
  symptoms ("a system clock stepped back expired entries written in the last seconds";
  "`explain()` reported `ttl_remaining_secs: 0` for a very large TTL").

## 4. Not yours — do not do these

- No error-variant change, no rejection (`max_entries(0)`, a TTL under one second), and no
  `DatabaseError`. Those are Part B, in v0.22.0.
- No change to what TTL means for correctly timed entries, and no change to `is_expired`'s callers.
- **Commit locally only; do not push** (review 019 § 4: the v0.21.5 batch goes out at release
  preparation).

## 5. Gates

- `cargo fmt --all --check`.
- `python3 scripts/feature_matrix.py --run-all`, every row, clippy `-D warnings`.
- `cargo make msrv-check` under 1.85, fresh `--output-dir`.
- `cargo test --workspace --all-features --locked`, **reporting the count you observe** (505 at
  the Q2b commits).
- `RUSTDOCFLAGS="-D warnings" cargo doc -p localcache --no-deps --all-features --locked`.
- `python3 scripts/source_integrity.py --require-tracked`.
- `git status --porcelain` shows only this slice's files.

## 6. Review request

File it as `.git-exclude/review-request/NNN-dev-q3a-one-expiry-rule-<date>.md`, following the
organization workflow § 9.2. Include:
- the failing-before and passing-after output;
- the `as`-cast grep before and after;
- the CHANGELOG diff;
- every gate, with its command;
- every required step, including the ones that pass.
