# RFC 019 Implementation Handoff — Standing Dispositions

## 1. Summary

Implement [RFC 019](../../accepted/019-standing-dispositions-for-unmaintained-dependencies.md),
Phase 22 **N2**.

Make `expires` optional for `unmaintained` and `notice` dispositions, keep it
mandatory for `vulnerability` and `unsound`, and migrate the two live entries to
standing dispositions.

RFC 019 is Accepted and authoritative. Where this handoff and the RFC appear to
disagree, **the RFC wins — report the discrepancy rather than choosing**.

This touches release tooling only. No library code, no schema, no Cargo manifest.

**There is no deadline pressure.** The current entries run to 2026-10-21 and this
change lands well inside that window. Do not rush it, and **do not pick a renewal
date** — after this RFC none is needed.

## 2. Change scope

### 2.1 — `scripts/check_advisories.py`

**`PolicyEntry.expires` becomes `date | None`.**

Three places need care:

1. **Schema validation.** `require_exact_keys` currently makes `expires` mandatory for
   every entry. It must become kind-aware: required for `vulnerability` and `unsound`,
   optional for `unmaintained` and `notice`.

   **The "no unknown keys" guarantee must not weaken.** An entry containing a
   misspelled or unexpected key must still be an error. If the simplest way to make
   `expires` optional also lets unknown keys through, that is the wrong
   implementation — a widening here becomes a hole.

2. **Parsing.** `parse_iso_date(entry["expires"], …)` runs unconditionally today.
   Guard it. The existing rule that `expires` must post-date `approved` still applies
   whenever `expires` is present.

3. **Classification.** In `classify_findings`, `if today >= entry.expires` must not run
   when `expires is None`. A standing disposition never expires.

Accept either an omitted key or an explicit `null`, per R1.

### 2.2 — Reporting (R4)

Current output:

```text
WARN RUSTSEC-2025-0052/async-std/1.13.2/unmaintained: warn until 2026-10-21 (localcache maintainers)
```

A standing disposition must **not** print "until <date>" — there is no date. It must
read as a standing disposition and name the condition that would re-raise it, so a
reader of the evidence bundle can tell the two forms apart without opening the policy
file.

Exact wording is yours; it must not imply an expiry that does not exist.

### 2.3 — `security/advisory-policy.json` (R6)

Migrate both live entries to standing dispositions: remove `expires`, keep `approved`
(RFC 019 open question 1 recommends retaining it), and rewrite `follow-up` per R3 to
state the **condition** that would change the decision rather than a date.

The existing follow-ups are date-shaped and must be replaced. For `bincode`, the
current note about assessing a format-compatible implementation against every
historical fixture is good substance — keep the substance, drop the "before expiry"
framing.

**Do not remove either dependency, and do not change `async-std`'s or `bincode`'s
version.** That is a separate decision the RFC deliberately leaves open.

### 2.4 — Hash pin

`scripts/release-tools.toml`'s `[implementations]` pin for `check_advisories.py` must
be updated to match the modified file. A stale pin fails the gate.

### 2.5 — RFC 014 amendment (R5)

RFC 014 documents the disposition schema. Mark the amendment **inline, at the clause
that states `expires` is mandatory** — not only in a decisions section, and not only
at the top of the file.

This is not pedantry. RFC 009 R16 was amended by RFC 017 without an inline marker, and
a later reviewer reading R16 concluded a retired requirement was still in force. Same
mistake, one line to avoid.

RFC 014 is in `rfcs/done/`. Amending a done RFC's text to record a later supersession
is correct and expected; do not move the file.

### 2.6 — User documentation

`docs/src/dependency_security.md` describes the policy schema. Update it to document
standing dispositions, when they apply, and why `vulnerability` and `unsound` keep
mandatory expiry — the standing-condition versus deferred-fix distinction is the
reasoning worth conveying.

## 3. Explicit non-change scope

- Do **not** change which kinds may use `warn` versus `exception`.
- Do **not** add new disposition actions.
- Do **not** change the advisory source, the RustSec fetch, or `--require-tracked`.
- Do **not** remove `async-std` or `bincode`, or alter their versions.
- Do **not** add an age-based warning for old standing dispositions. RFC 019 open
  question 2 considered and **rejected** it — it would reintroduce the calendar the
  RFC removes.
- Do **not** touch `scripts/release.py`'s gate composition.

## 4. Required tests

All of RFC 019's test plan is required. The one that carries the argument:

- [ ] **A standing `unmaintained` disposition does not cover a `vulnerability` finding
      for the same package and version.** The entire case for removing expiry rests on
      the key including `kind`, so this must be tested explicitly rather than assumed.

Plus:

- An `unmaintained` entry with no `expires` is accepted and reported as standing.
- A `vulnerability` entry with no `expires` is a **schema error**.
- A `vulnerability` entry with a past `expires` still denies — unchanged behaviour.
- An unknown key in any entry is still an error (guards §2.1's widening).
- A standing entry whose package version changes produces **both**
  `DENY: no exact policy disposition` for the new finding and
  `DENY: stale policy entry` for the old.
- A one-byte change to `check_advisories.py` still fails producer-tool verification.

## 5. Required evidence

- The advisory gate run against the migrated policy: exit status and full output,
  showing both entries reported as standing.
- Full `scripts/tests` suite result with counts before and after.
- Confirmation that the `[implementations]` pin matches.
- The RFC 014 amendment quoted in the review request, so the inline placement can be
  checked without opening the file.

## 6. A note on what this changes

RFC 019 **weakens a control** — it removes a periodic forced re-decision — and argues
that the pressure was illusory for these kinds because the version+kind key already
re-raises on every substantive change.

You do not need to agree with that to implement it. But if the implementation reveals
the argument is wrong — for example if the key does **not** behave as
§2.4's required test expects — that is a finding that invalidates the RFC's premise.
**Stop and report it.** Do not adjust the test to match the behaviour.
