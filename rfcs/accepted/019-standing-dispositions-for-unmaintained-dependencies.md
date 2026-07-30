# RFC 019 — Standing Dispositions for Unmaintained Dependencies

| Field | Value |
|---|---|
| Status | Accepted |
| Feature | *(release engineering; no Cargo feature)* |
| Touches | `scripts/check_advisories.py`, `security/advisory-policy.json`, `scripts/release-tools.toml`, `docs/src/dependency_security.md` |
| Amends | **RFC 014** — advisory policy schema |
| Finding | Owner challenge, 2026-07-30: renewing an expiry date is calendar management, not project management |
| Milestone | Phase 22 N2 |
| Breaking | No — no library, schema, or wire-format change |

## Summary

Let an `unmaintained` or `notice` disposition be recorded **without an expiry date**,
because the advisory gate already re-raises those findings whenever anything
substantive changes. Keep mandatory time-boxing for `vulnerability` and `unsound`,
where it does real work.

## Motivation

`security/advisory-policy.json` currently requires every disposition to carry an
`expires` date, enforced by `require_exact_keys` in `check_advisories.py`. The two
live entries — `async-std` and `bincode`, both `unmaintained`, neither a
vulnerability — expire on 2026-10-21, at which point the gate denies and CI turns
red.

The owner's objection is that renewing that date is calendar administration, not a
decision about the project. That is correct, and the mechanism is worse than merely
tedious: **the renewal carries no information.** Nothing about the dependency has
changed; the only thing that expired is a number in a file. A process that
periodically demands a decision where nothing has changed trains its participants to
approve without thinking — the opposite of what a security gate is for.

### The re-review trigger already exists, and it is a better one

A disposition is matched to a finding by an exact four-part key:

```python
key = (advisory_id, package, version, kind)
```

Both directions fail closed:

- a finding with no exactly-matching entry → `DENY: no exact policy disposition`;
- an entry matching no current finding → `DENY: stale policy entry`.

Three consequences follow, and together they make the calendar redundant for
`unmaintained`:

1. **Any version change re-raises the finding.** When `async-std 1.13.2` becomes
   `1.14.0`, the key stops matching and the gate denies until a human decides again.
2. **Any change of kind re-raises the finding.** This is the decisive one. The
   obvious worry about accepting an unmaintained crate is *"what if a vulnerability
   is found later and nobody fixes it?"* If that happens, RustSec publishes a
   finding with `kind: "vulnerability"` — a **different key** — which no
   `unmaintained` disposition covers. The gate denies immediately, on the day the
   advisory lands, rather than waiting for a date someone picked months earlier.
3. **Dead entries cannot accumulate.** A disposition that no longer matches anything
   is itself a denial, so the policy file cannot silently rot.

An expiry date adds a fourth trigger that fires when *nothing has changed*. It is
the only one of the four that carries no signal.

### Where expiry still earns its place

For `vulnerability` and `unsound` findings — which the schema already requires to
use `action: "exception"` rather than `"warn"` — a known defect exists and a fix
path is usually knowable. Accepting one is a deliberate deferral, and a deferral
should have a deadline that forces you back to it. **This RFC does not change those.**

The distinction is between a *standing condition* and a *deferred fix*.
Unmaintained-ness is a condition: it is either acceptable or it is not, and the
answer does not change on a Tuesday in October. An unpatched vulnerability is a
deferral, and deferrals should expire.

## Requirements

### R1 — `expires` becomes optional for `unmaintained` and `notice`

Those kinds may omit `expires`, or set it to `null`. The gate treats such an entry as
a **standing disposition**: valid until the version, the kind, or the dependency
graph changes.

### R2 — `expires` remains mandatory for `vulnerability` and `unsound`

Unchanged, including the existing rule that `expires` must post-date `approved`. An
entry of either kind omitting `expires` is a schema error, not a standing
disposition. This keeps the harder case strict.

### R3 — A standing disposition must justify itself

`reason` and `follow-up` remain mandatory for every entry. For a standing
disposition, `follow-up` must state the condition that would change the decision —
for example *"reassess if a maintained fork gains adoption, or if any vulnerability
advisory is published against this package"* — rather than a date. A standing
disposition with a `follow-up` that only says "review later" defeats the purpose and
should be rejected in review.

### R4 — Reporting distinguishes the two

Gate output must not imply an expiry that does not exist. Current form:

```text
WARN RUSTSEC-2025-0052/async-std/1.13.2/unmaintained: warn until 2026-10-21 (localcache maintainers)
```

Standing form must instead read `standing disposition` and name the re-raise
condition, so a reader of the evidence bundle can tell the two apart without
consulting the policy file.

### R5 — RFC 014's schema description is amended

RFC 014 documents the disposition schema. It must record that `expires` is
conditional on `kind`, with this RFC named as the amendment — marked **at the point
of use**, not only in a decisions section. (RFC 009 R16 was amended by RFC 017
without an inline marker, and a later reviewer concluded a retired requirement was
still in force. Same mistake, cheap to avoid.)

### R6 — Migrate the two live entries

`async-std` and `bincode` become standing dispositions with their expiry removed and
`follow-up` rewritten per R3. **No renewal date is chosen**, because after this RFC
none is needed.

### R7 — Non-goals

Not in scope: changing which kinds may use `warn` versus `exception`; adding new
disposition actions; changing the advisory source or the `--require-tracked`
integrity checks; removing `async-std` or `bincode` from the graph. The last is a
separate decision this RFC deliberately leaves open — it makes accepting them
honest, not permanent.

## Design

`PolicyEntry.expires` becomes `date | None`. Parsing enforces R1/R2 by kind. The
`today >= entry.expires` check is skipped when `expires is None`. `require_exact_keys`
becomes kind-aware, or `expires` moves to an optional-key set — whichever keeps the
"no unknown keys" guarantee intact, which must not be relaxed.

`schema: 1` is retained. This is a widening change: every currently valid policy file
remains valid, so no migration is forced on anyone.

## Test plan

- An `unmaintained` entry with no `expires` is accepted and reported as standing.
- A `vulnerability` entry with no `expires` is a **schema error**.
- A `vulnerability` entry with `expires` in the past still denies, unchanged.
- An unknown key anywhere in an entry is still an error — the widening must not
  become a hole.
- A standing entry whose package version changes yields
  `DENY: no exact policy disposition` for the new finding **and**
  `DENY: stale policy entry` for the old one.
- A standing `unmaintained` entry does **not** cover a `vulnerability` finding for
  the same package and version. This is the requirement the whole argument rests on
  and it must be tested explicitly, not assumed.
- `scripts/release-tools.toml`'s hash pin for `check_advisories.py` is updated, and
  a one-byte change to the script still fails verification.

## Security considerations

**This weakens a control, and the case for it must stand on that basis.** Removing a
periodic forced re-decision is a real reduction in process pressure.

The claim is that the pressure was illusory for these kinds: the version+kind key
already re-raises on every substantive change, and a newly published vulnerability
against an accepted-as-unmaintained package is denied on the day it appears rather
than at the next renewal. Under the current scheme, an unmaintained crate that gains
a vulnerability advisory the day after a renewal is *also* denied immediately — for
the same reason. The expiry never provided that protection; the key did.

What is genuinely lost: a scheduled prompt to reconsider whether the dependency is
still worth keeping. R3 substitutes a written re-assessment condition. That is
weaker as a forcing function and stronger as a record, and the trade is deliberate.

`vulnerability` and `unsound` keep mandatory expiry precisely because for those the
scheduled prompt is the point.

## Compatibility

No library, schema, payload, or CLI change. Existing policy files stay valid.
Affects the release gate only.

## Open questions

1. **Should a standing disposition still record `approved`?** Recommend yes — it
   dates the decision even without an expiry, and costs nothing.
2. **Should the gate warn if a standing disposition is older than some age?** That
   would smuggle the calendar back in, so: no. Noted only to record that it was
   considered and rejected.
