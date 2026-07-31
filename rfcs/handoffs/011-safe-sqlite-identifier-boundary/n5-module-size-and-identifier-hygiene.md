# N5 Implementation Handoff — Identifier Hygiene and Module-Size Debt

## 1. Summary

Phase 22 **N5**. Two unrelated bodies of work that share one file:

- **Part 1** — RFC 011's non-blocking findings **N-01** and **N-02**, both in
  `crates/localcache/src/db/indexes.rs`.
- **Part 2** — module-size debt, per `ROADMAP.md` § "Corrected module-size register".
  `indexes.rs` is also a split candidate, which is why these travel together: touch the
  file once, in a controlled order.

Part 1 implements accepted RFC 011 findings. **Part 2 is not RFC-driven** — it is
roadmap-tracked maintenance, and this handoff lives under RFC 011's directory only
because Part 1 does. Do not read Part 2 as having RFC authority; its constraints come
from §4 below.

No public API, schema, payload, or behaviour change in either part.

## 2. Part 1 — RFC 011 N-01 and N-02

**These two are coupled. Do them together, in one commit.**

### N-01 — quote the catalog's spelling, not the caller's

`indexes.rs:94` and `:121` quote `full`; `:180` quotes `name`. All three quote the
**caller's** string rather than the catalog value already validated against the
database.

**This is safe today**, and the change is defence-in-depth, not a bug fix. The current
safety argument runs: `identifier_eq` requires equal length and ASCII-case-equal bytes,
so any `"` in the caller string must exist at the same offset in the catalog name, and
`quote_identifier` doubles every `"` — therefore the emitted SQL is always one
well-formed quoted identifier.

That argument is correct but spans two functions. Quoting `object.name` — a value
already validated against the catalog — makes it **locally** obvious and removes the
dependency on `identifier_eq`'s exact semantics.

### N-02 — comment the ASCII-only folding

`identifier_eq`'s ASCII-only case folding is **deliberate and correct**: it mirrors
SQLite's own ASCII-only identifier folding. It is uncommented, so a future contributor
could "fix" it to Unicode-aware casefolding and silently widen the match set.

**Why these are coupled:** N-01's current safety *depends* on exactly the assumption
N-02 protects. Fixing N-01 removes the dependency; commenting N-02 protects it in the
meantime and afterwards. Doing one without the other leaves the weaker half of the
argument standing alone.

Add a comment stating the folding is ASCII-only **to match SQLite**, and that widening
it to Unicode would change which identifiers are considered equal.

### Constraints

- No behaviour change. `identifier_eq`'s semantics stay exactly as they are — N-02 is a
  comment, not a fix.
- The existing hostile-identifier test suite (`tests/rfc013_input_safety.rs` and the
  `indexes.rs` unit tests) must pass **unchanged**. If any test needs modifying, stop and
  report — that would mean the change is not behaviour-preserving.

## 3. Part 2 — module-size debt

Measured ELOC (non-blank, non-comment) against the project's 500-ELOC guidance:

| File | ELOC | Status |
|---|---|---|
| `crates/localcache/src/cache/engine.rs` | 946 | largest in the crate |
| `crates/localcache/src/db/indexes.rs` | 914 | also Part 1's file |
| `crates/cli/src/main.rs` | 728 | |
| `crates/localcache/src/db/repository.rs` | 618 | |
| `crates/localcache/src/db/schema/classifier.rs` | 586 | |
| `crates/localcache/src/cache/query.rs` | 463 | **complies — remove from the register** |

The previous register omitted `engine.rs` and `classifier.rs` and wrongly listed
`query.rs`. Correcting it is part of this task.

## 4. The governing constraint — read this before splitting anything

**Splits are risk-reducing only. Size alone does not justify restructuring code whose
behaviour is covered and stable.**

A split is justified when it makes a module easier to reason about or review — for
example separating a self-contained concern with a narrow interface. It is *not*
justified merely because a file crosses a line count. A mechanical split that scatters
tightly coupled logic across four files makes the code harder to follow while improving
a metric, and that is a net loss.

Concretely:

- **No public API change.** Module reorganisation must be invisible outside the crate.
  Re-export whatever is needed to keep paths stable.
- **No behaviour change.** Moving a function must not change what it does.
- **Tests move with their code** and must pass unchanged. If a test needs editing to
  survive a move, the move changed behaviour — stop and report.
- **If a file resists a clean split, say so and leave it.** "This module is 946 lines
  and every part of it touches engine state; splitting it would create four files with a
  wide mutual interface" is a **valid and welcome outcome.** Report it rather than
  forcing a split to satisfy the register.

I would rather receive two well-justified splits and four reasoned refusals than six
mechanical splits.

## 5. Suggested order, and why

1. **Part 1 first, as its own commit.** Small and surgical. Doing it before any
   `indexes.rs` split keeps the security-relevant diff readable on its own, rather than
   buried in a file reorganisation.
2. **`engine.rs`** — largest, and most likely to contain separable concerns
   (maintenance operations already live in `engine/maintenance.rs`, so the pattern
   exists).
3. **`indexes.rs`** — after Part 1.
4. **`cli/src/main.rs`** — subcommand handlers are often cleanly separable.
5. **`repository.rs`, `classifier.rs`** — only if a genuine seam exists.

Stop whenever the remaining candidates do not justify the churn.

## 6. Explicit non-change scope

- No change to SQL construction, the `QuotedIdentifier` boundary, or any validation
  logic. Part 1 changes *which already-validated string is quoted*, nothing more.
- No dependency, feature, or manifest change.
- No version bump.
- No changes under `scripts/` — that is N3.
- Do not "improve" code you move. A split diff must be reviewable as a move; mixing in
  refactors makes that impossible.

## 7. Required tests and evidence

- Full suite passes, counts before and after. **The count should not change** for Part 2
  — a split that adds or removes tests is not a pure move.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean.
- `cargo fmt --all --check` clean.
- The full feature matrix (`feature_matrix.py --run-all`) — module moves can break
  feature-gated compilation in ways `--all-features` hides.
- The declared-MSRV matrix on 1.85.
- `source_integrity.py --require-tracked` OK. **Any new module file must be tracked** —
  a `#[cfg(test)]` submodule is not a manifest-declared target, so the gate will not
  catch an untracked one. This bit N1; do not let it bite again.
- For each file in §3: either the split with its justification, or a **reasoned refusal**.
