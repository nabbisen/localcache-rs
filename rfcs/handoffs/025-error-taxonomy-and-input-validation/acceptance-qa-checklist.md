# RFC 025 Acceptance & QA Checklist — Part A (Q3a)

Companion to `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md`.

## A. Q3a — One expiry rule (R1–R2)

- [ ] `ttl_remaining(now, updated_at, ttl) -> Option<u64>` exists, is `pub(crate)`, and is the only place TTL arithmetic happens
- [ ] Age is clamped at zero, and everything saturates. No signed/unsigned `as` cast remains on the TTL path; the grep is shown before and after, with each remaining hit explained
- [ ] `is_expired` is `ttl_remaining(…) == Some(0)`, with its signature and three callers unchanged
- [ ] `explain` derives both `ttl_remaining_secs` (saturating at `i64::MAX`) and its expired flag from `ttl_remaining`, reading `now` once
- [ ] Failing-before shown for: the clock-back `get_if_fresh`/`check_status`; `cleanup_expired` deleting the entry; `explain` on the clock-back entry; `Duration::MAX` → `Some(0)`
- [ ] Passing-after for all of them, plus the `ttl_remaining` unit tests (boundaries, extremes, no TTL), run in debug as well as release
- [ ] Correctly timed entries expire exactly as before: the existing TTL tests pass unmodified
- [ ] The rustdoc on `ttl` notes that a backward clock step does not expire entries. Nothing about v0.22.0's rejection or resolution appears
- [ ] CHANGELOG `### Fixed` has two user-symptom sentences
- [ ] No error variant, rejection, public signature, or push

## B. Gates and reporting

- [ ] `cargo fmt --all --check` clean
- [ ] `feature_matrix.py --run-all` green (clippy `-D warnings`)
- [ ] `cargo make msrv-check` green under 1.85, with every attempt reported
- [ ] The full suite count is reported as observed
- [ ] Rustdoc with `-D warnings` is clean
- [ ] `source_integrity.py --require-tracked` OK
- [ ] The review request is `.git-exclude/review-request/NNN-dev-q3a-one-expiry-rule-<date>.md`, and names this handoff
- [ ] Every required step is listed, including the ones that pass
- [ ] Judgement calls are **reported, not absorbed**
