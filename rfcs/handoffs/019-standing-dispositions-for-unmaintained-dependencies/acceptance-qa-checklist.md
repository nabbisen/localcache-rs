# RFC 019 Acceptance and QA Checklist — Standing Dispositions

Operationalizes [RFC 019](../../done/019-standing-dispositions-for-unmaintained-dependencies.md)
(Phase 22 N2). The RFC is authoritative; this list adds and relaxes nothing.

**Every box must be backed by an observed result. An unrun check is a failure, not a
pass.**

## Preconditions

- [ ] The implementation commit under test is identified exactly.
- [ ] The tracked worktree is clean.
- [ ] No library source, Cargo manifest, schema, or version changed.
- [ ] `scripts/release.py`'s gate composition is untouched.

## R1/R2 — Kind-aware expiry

- [ ] An `unmaintained` entry with `expires` **omitted** is accepted.
- [ ] An `unmaintained` entry with `expires: null` is accepted.
- [ ] A `notice` entry without `expires` is accepted.
- [ ] A `vulnerability` entry without `expires` is a **schema error**.
- [ ] An `unsound` entry without `expires` is a **schema error**.
- [ ] A `vulnerability` entry with `expires` in the past still **denies**.
- [ ] `expires` must still post-date `approved` wherever it is present.

## The guarantee the RFC rests on

- [ ] **A standing `unmaintained` disposition does not cover a `vulnerability` finding
      for the same package and version.** Tested explicitly. *(If this fails, the RFC's
      premise is wrong — report it; do not adjust the test.)*
- [ ] A standing entry whose package version changes yields
      `DENY: no exact policy disposition` for the new finding **and**
      `DENY: stale policy entry` for the old one.

## The widening must not become a hole

- [ ] An entry containing an unknown or misspelled key is **still an error**.
- [ ] An entry missing a still-required key (`id`, `package`, `version`, `kind`,
      `action`, `owner`, `approved`, `reason`, `follow-up`) is still an error.

## R3/R4 — Justification and reporting

- [ ] Both migrated entries state a **condition** in `follow-up`, not a date.
- [ ] Neither `follow-up` says only "review later" or equivalent.
- [ ] Gate output for a standing disposition does **not** print "until \<date\>".
- [ ] Gate output names the re-raise condition, so the two forms are distinguishable in
      the evidence bundle without opening the policy file.

## R5/R6 — Amendment and migration

- [ ] RFC 014's mandatory-`expires` clause carries an **inline** amendment marker
      naming RFC 019, at the clause itself — not only in a decisions section or file
      header.
- [ ] `security/advisory-policy.json`: both entries have no `expires`, retain
      `approved`, and keep `action: "warn"`.
- [ ] **No renewal date was chosen anywhere.**
- [ ] `async-std` and `bincode` remain in the graph at unchanged versions.
- [ ] `docs/src/dependency_security.md` documents standing dispositions and why
      `vulnerability`/`unsound` keep mandatory expiry.

## Non-goals respected

- [ ] No new disposition action was added.
- [ ] The `warn`/`exception` rules per kind are unchanged.
- [ ] **No age-based warning** for old standing dispositions was added — RFC 019
      considered and rejected it.
- [ ] The advisory source and `--require-tracked` checks are unchanged.

## Gates

- [ ] The advisory gate runs green against the migrated policy; exit status and full
      output are quoted.
- [ ] Full `scripts/tests` suite passes; counts before and after reported.
- [ ] `scripts/release-tools.toml`'s pin for `check_advisories.py` matches the file.
- [ ] A one-byte change to `check_advisories.py` fails producer-tool verification.
- [ ] The suite passes under RC-3's restricted `PATH` (no `cargo`/`rustc`/`mdbook`/
      `rustup`/`cargo-audit`) — this suite runs in a toolchain-free CI job.
- [ ] `python3 scripts/source_integrity.py --require-tracked` OK.
