# Phase 23 P0 Implementation Handoff — v0.21.1

## 0. A deliberate naming deviation, flagged not hidden

RFC 000 defines `rfcs/handoffs/` as *"companion execution docs **for RFCs**"*, named
`NNN-slug/`, with status inherited from the matching RFC number.

**P0 implements no RFC.** It closes recorded findings that span four different RFCs'
territory — glob documentation (RFC 006), a `ConnectionPool` defect (found during RFC 018's
review), release tooling (RFC 009/014), and async test structure (RFC 005). Filing it under
any one of those would misrepresent the other three; splitting it into four directories
would fragment a single patch release of small fixes.

So this directory is **phase-keyed rather than RFC-keyed**, and it has no RFC status to
inherit — it is complete when P0d ships. **P1b's handoff will follow the normal convention**
once that RFC exists.

If the owner prefers a different arrangement, say so and it moves; this is a judgement call,
not a rule I found.

## 1. Summary

Five parts, all small, all targeting **v0.21.1** — a non-breaking patch release. Each part is
**independently reviewable**; file one review request per part or group them, but say which
you are doing.

**Do not bump any version in these tasks.** Coming-version housekeeping is P0d, with its own
gate, and setting versions early breaks the version-reference check.

No schema, payload wire format, SQL, or public API change in any part. If a change you are
making would alter public API, stop and report — that would make the release breaking and
invalidate the v0.21.1 target.

## 2. Part A — Put the glob guidance where users will meet it

**Finding:** N4 measured `path_glob` with a leading literal as **flat** across a 100× range
(2.87 ms → 3.05 ms from 10k to 1M rows), while a leading wildcard grows **18×** over the same
range. The difference is that a leading literal produces an indexable range
(`path>? AND path<?`) and a wildcard does not.

**This is currently documented only in `docs/src/performance.md`** — a page a user reads when
already worried about performance, not when writing their first query.

**Change:** state the rule where the decision is actually made.

1. **`path_glob`'s rustdoc** in `crates/localcache/src/cache/query.rs` — the most important
   one. A user writing `path_glob("*/foo/*")` should see it at the call site, in their editor.
2. **`docs/src/querying.md`** — in the glob section, not a footnote.
3. Link both to `performance.md` for the measured numbers rather than repeating them.

**Keep it short and concrete.** A leading literal keeps the scan bounded; a leading `*` does
not. One sentence and a two-line example beats a paragraph.

**Do not** change `path_glob`'s behaviour, signature, or validation. This part is
documentation only.

## 3. Part B — `ConnectionPool` batch methods break the length invariant

**Finding, from the N1 review §4.1.** Three methods in `crates/localcache/src/pool.rs` return
**one** element on lock failure regardless of how many paths were requested:

| Method | Line (approx) | On lock failure |
|---|---|---|
| `batch_get` | 124 | `vec![Err(e)]` |
| `batch_get_fresh` | 135 | `vec![Err(e)]` |
| `check_status_batch` | 184 | `vec![Err(e)]` |

A caller doing `paths.iter().zip(results)` silently processes one path and drops the rest; a
caller indexing by position reads the wrong entry. It only manifests on a poisoned lock — an
exceptional path — which is exactly why it has gone unnoticed.

**`ReadPool` already does this correctly** since RFC 018, and is the model to follow
(`crates/localcache/src/read_pool.rs`, `batch_get`):

```rust
match self.checkout() {
    Ok(guard) => guard.batch_get(paths),
    Err(_) => paths.iter().map(|_| Err(LocalFileCacheError::Poisoned {
        resource: "ReadPool",
    })).collect(),
}
```

**Change:** all three `ConnectionPool` methods return **one error per requested path**, using
`Poisoned { resource: "ConnectionPool" }`, matching what `lock()` already returns. Document
the guarantee in each method's rustdoc, as `ReadPool`'s now does.

**Is this breaking?** We judge not: it changes a return-vector length on an exceptional path
that no documented contract described, and it makes behaviour match the reasonable
expectation. But **if you find any test, doctest, or example depending on the single-element
shape, stop and report** — that would be evidence someone relied on it, and it becomes a
release-target question rather than an implementation one.

### Also in Part B — a duplication to collapse

`crates/localcache/src/cache/engine/portable.rs`: `namespace_copy`'s body is **byte-identical**
to `import_from`'s (184 characters each; verified during the N5 review). N5 correctly reported
rather than fixed it, because that was a move-only task.

Collapse the duplication — one delegating to the other, or both to a shared private helper.
**Both public methods must keep their names, signatures, and documented behaviour.** If they
are genuinely meant to differ and the identity is the bug, that is a finding: report it
instead of unifying them.

## 4. Part C — Release tooling hygiene

Three recorded findings under `scripts/`. All independent of Parts A, B, and E.

**C1 — `fetch_with_retry` catches too broad an error.** In
`scripts/check_advisories.py`, the retry wrapper catches `AdvisoryGateError`. That is correct
*today* only because `live_fetch`'s sole failure mode is `OSError`/`URLError`. But `Fetch` is
an injection point (`Fetch = Callable[[str, int], tuple[...]]`), so a substitute raising
`AdvisoryGateError` for a non-transient reason would be retried three times — turning a fast,
clear failure into a slow, confusing one.

Introduce a dedicated transient error type that `live_fetch` raises only for network
conditions, and have `fetch_with_retry` catch **only** that. The retry decision belongs to the
layer that knows whether a failure is transient, not to the wrapper inferring it.

Keep every existing boundary: a non-5xx status, a validation failure, and a size-limit breach
must still never be retried.

**C2 — `follow-up` is a sentence fragment.** In `security/advisory-policy.json`, both entries
read like `"a maintained fork of async-std gains adoption, …"` — a fragment that only forms a
sentence once the reporter prepends "reassess if". Read directly, or by any future consumer
that is not this one report format, it is incomplete.

Make the stored value self-describing and have the reporter emit it verbatim. Data that parses
only inside one template is fragile the moment a second consumer appears, and an evidence
bundle is exactly the kind of artefact that acquires them.

**C3 — Pin the exhaustiveness doctest.** `crates/localcache/src/error.rs`'s ```` ```compile_fail ````
doctest passes if the code fails to compile for **any** reason — a typo would satisfy it
vacuously. Rustdoc accepts an expected error code: ```` ```compile_fail,E0004 ````.

The guarantee currently rests on a mutation test the N1 reviewer ran by hand (adding a `_` arm
made it fail). Pinning the code makes that permanent.

> **Correction, 2026-08-01 — the sentence above is wrong, and was wrong when written.**
> Rustdoc does **not** verify a `compile_fail` block's error code against the actual
> diagnostic. The implementer found this by mutating the doctest twice (an unresolved-path
> error, and a clean E0308 with the match made exhaustive) — both still reported `ok` — and
> the reviewer reproduced it in an isolated crate: a block annotated `compile_fail,E0004`
> that actually fails with E0425 still passes.
>
> The annotation is worth keeping as documentation of intent, and `error.rs` now carries a
> comment saying so. But **the real guarantee still rests on mutation testing at review
> time**, exactly as before the edit. Recorded in `ROADMAP.md`'s deferred register rather
> than pretended closed. Left here rather than deleted so the wrong claim does not reappear.

**Note:** C1 and C2 change `scripts/check_advisories.py` and are the same-file hazard N3's §2
warned about. They are in the same part deliberately — do them together, in one commit, and
re-pin `scripts/release-tools.toml`'s `[implementations]` hash for that file.

## 5. Part E — Deduplicate the async runtime tests

`crates/localcache/tests/pool_observe.rs` carries three runtime modules — `rfc005_async_std`,
`rfc005_smol`, `rfc015_tokio_async_engine` — totalling **295 lines**, of which two test
functions and a `block_on` helper are genuinely triplicated:

- `poisoned_mutex_recovers_on_subsequent_calls`
- `panic_inside_blocking_closure_yields_async_task_panicked`

**Change:** a `macro_rules!` helper that generates the per-runtime modules from one body.

**Explicitly not a proc-macro.** Owner decision, 2026-08-01, recorded in `ROADMAP.md`
§ "Why P0e is a `macro_rules!` helper". Do **not** create a new crate, and do **not** add
`syn`, `quote`, or `proc-macro2`.

**Constraints:**

- **Test count must not change**, and every test must still run under its own runtime feature.
  A macro that accidentally collapses three tests into one is a coverage regression that a
  green suite would not reveal — assert the count before and after.
- The generated modules keep their current names, so failures stay attributable to a runtime.
- Tests that are genuinely single-runtime stay as they are. This is not an invitation to
  unify the other 29 async tests.
- If a test looks triplicated but differs subtly between runtimes, **leave it and say so** —
  a difference you cannot see is a difference that matters.

## 6. Explicit non-change scope

- No public API change in any part, except Part B's return-length correction on an
  exceptional path.
- No schema, migration, payload encoding, or SQL change.
- No version bump; no `CHANGELOG.md` version heading edit. Add changelog **entries** under
  the existing Unreleased structure if you wish, but do not create the `0.21.1` heading.
- No new crate, and no new runtime dependency anywhere.
- Do not touch `scripts/release.py`'s gate composition or `CI_REQUIRED_JOBS`.

## 7. Required evidence

Per part, plus overall:

- Full test suite with counts **before and after** — Part E in particular must show the count
  unchanged.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` clean.
- `cargo fmt --all --check` clean.
- The full feature matrix (`feature_matrix.py --run-all`) — Part E changes feature-gated test
  structure, which `--all-features` alone can hide.
- The declared-MSRV matrix on 1.85.0.
- For Part C: the `scripts/tests` suite, the suite under RC-3's **restricted `PATH`**, a live
  `release.py security` run at exit 0, and confirmation the `check-advisories` hash pin
  matches after C1/C2.
- `python3 scripts/source_integrity.py --require-tracked` OK — **any new module file must be
  tracked**; the gate will not catch an untracked `#[cfg(test)]` submodule, which bit N1.

## 8. Recommended order

A (docs, no risk) → C (self-contained, one file plus a pin) → E (tests only) → B (library
change, the only one touching shipped behaviour). B last so it lands against an otherwise
settled tree.

P0d — coming-version housekeeping and the v0.21.1 release — follows once A, B, C, and E are
accepted. Its procedure is
`rfcs/handoffs/009-reproducible-source-archives-and-release-gates/n6-coming-version-housekeeping.md`,
which is version-agnostic apart from the target number.
