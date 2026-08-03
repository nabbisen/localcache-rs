# P2c Handoff — Re-measure RFC 021

RFC: `rfcs/accepted/021-query-execution-one-pass-late-materialization.md`
Implementation: commit `cdc2a01` (P2b)
Review: `.git-exclude/reviewed/architect-rfc021-p2b-review-2026-08-03.md`
Milestone: Phase 23 **P2c**

## 0. What P2c establishes

What RFC 021 actually delivered, on the scale profile rather than on ad-hoc probes — **reported as
two separate numbers, not one.**

The review measured **tier 1 at 14.7× and the headline field query at 2.0×** on a heavy payload.
Those differ by more than 7×. A single blended figure would hide both the win and the shortfall,
and the shortfall is the more useful of the two to have on record.

## 1. Method — the same one P1d proved

Do not compare against stored figures from another session. Build both sides and interleave them.

1. **Worktree at the pre-RFC-021 library:**
   `git worktree add .git-exclude/tmp/p2c-before cdc2a01~1`
2. **Copy the current `scale_profile.rs` into the worktree**, overwriting its copy, and confirm
   byte-identical with `diff` before building. RFC 021 changed no public API, so the same harness
   compiles against both. This matters because §2 adds a measurement — both arms must run identical
   harness code.
3. **One shared `TMPDIR` under `.git-exclude/tmp/`** for every run. Both arms are on the same
   filesystem, so a shared parent matches path length by construction. Confirm the harness's own
   `TMPDIR path length` and `example stored path length` lines print **the same numbers on every
   run** before reading any timing.
4. **Interleave**: before, after, before, after at 1M. Two runs per arm is enough — the effects
   being separated are 2×–15× against a few percent of session noise.
5. Remove the worktree when done: `git worktree remove .git-exclude/tmp/p2c-before`.

## 2. Add a tier-1 query measurement to the harness

The profile currently measures only `field_gt + order_by_field + limit 25` — a **tier 2** query.
The largest effect RFC 021 produced is in **tier 1**, and nothing in the harness measures it.

Add, next to the existing whole-namespace queries:

```rust
engine.query().limit(25).run()
```

No predicate, no field sort — so no payload is touched until the winners are known. Report it under
the existing replication mechanism (`LOCALCACHE_SCALE_QUERY_REPEATS`), like the other queries.

Label it clearly as tier 1 in the output. A reader should not have to know the tier taxonomy to see
that two different things are being measured.

## 3. The control block still applies

RFC 021 touches query execution only. `get`, `get_if_fresh`, `cleanup_missing_files`,
`cleanup_expired`, LRU eviction, populate, and DB size should be **unchanged between arms** — they
are your free measurement of session drift, exactly as in P1d.

`path_in_dir` and both globs *do* go through `execute_query`, so they are **expected to improve** —
they are not controls. Report them as results.

**If a genuine control moves a lot, stop and report it** — RFC 021 should not have touched it.

## 4. What to report

**4.1 — Two headline rows, separately:**

| Query | before (`cdc2a01~1`) | after (`cdc2a01`) | ratio |
|---|---|---|---|
| `limit(25)`, no field predicate (**tier 1**) | | | |
| `field_gt` + `order_by_field` + `limit 25` (**tier 2**) | | | |

**4.2 — The other query operations**, which also route through the rewritten path: `path_in_dir`,
`path_glob` literal, `path_glob` wildcard.

**4.3 — The control block**, expected ~1.0×.

**4.4 — All three scales** (10k / 100k / 1M) for both headline rows.

**4.5 — The substrate block** each run prints. Both arms must match.

## 5. Expected, and what would falsify it

From the review's probe measurements at 1M, heavy payload:

| | expected |
|---|---|
| Tier 1 | **~15×** (3.39 s → ~0.23 s) |
| Tier 2 headline | **~2×** (4.24 s → ~2.13 s) |

**The tier-2 figure being ~2× rather than ~4× is expected and already explained** — SQLite's
`json1` is not streaming, so `json_extract` parses the whole stored document per call, costing
O(payload bytes) per candidate. That was the RFC's named-but-unpriced unknown, and it resolved
unfavourably. **Do not treat a ~2× tier-2 result as a failure to investigate.**

Treat as findings worth reporting:

- Tier 1 materially below ~10× — the core mechanism is not doing what the probe showed.
- Tier 2 **below 1.5×** — worse than the review measured; something differs from the probe.
- Any genuine control moving more than a few percent.

## 6. Constraints

- **No library change.** P2c measures. Report anything slow; do not fix it.
- Harness change limited to §2's addition. Do not restructure existing measurements — the control
  block depends on them being identical to what the old arm runs.
- `LOCALCACHE_SCALE` still defaults to 10 000; the harness stays out of CI.
- Scratch under `.git-exclude/tmp/`, removed after use.

## 7. Gates

- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` clean
- `cargo fmt --all --check` clean
- `python3 scripts/source_integrity.py --require-tracked` OK
- Full suite green — report the number you observe (425 as of `cdc2a01`; report yours)

## 8. What happens next

P2d is the release decision. RFC 021 changed no public API, so it is a **patch** — v0.21.3 — unless
P2c surfaces something that changes that.

Two items already recorded, for P2d rather than P2c:

- `docs/src/performance.md`'s query figures will need the new numbers, the same way P1e rebuilt the
  cleanup figures.
- `docs/src/querying.md` says `dry_run()` runs `EXPLAIN QUERY PLAN` on the path-listing SQL only.
  Still true of the first line, but it now also reports the execution tier (R4).

Neither is yours in this milestone; both are listed so they are not lost.
