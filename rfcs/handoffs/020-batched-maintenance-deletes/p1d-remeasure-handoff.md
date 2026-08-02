# P1d Handoff — Re-measure RFC 020

RFC: `rfcs/accepted/020-batched-maintenance-deletes.md`
Implementation: commit `27a6551` (P1c)
Milestone: Phase 23 **P1d**

## 0. What P1d establishes

Whether RFC 020 delivered what it projected — **5.761 s → ~1.09 s at 1M** — and what
`cleanup_expired` costs, which nothing has ever measured.

## 1. Do not compare against P1a's logs

This is the whole methodological point of the milestone, and it is the mistake that cost P1a
three review rounds.

`cleanup_missing_files` is destructive: it cannot be replicated within a run, so every figure is
single-sample and carries **session-level variance of about ±15%** — five otherwise-comparable
btrfs 1M runs spread 1.24× (P1a limitation 5, in the harness doc comment). P1a's numbers were
also taken on a different day, with a different binary.

**So the before and after must both be measured in one session, on the same host, at the same
`TMPDIR` path length.** A "before" taken from a log file is not a control.

## 2. Method — build both versions, run them interleaved

**2.1 — Create a worktree at the pre-RFC-020 library.**

```sh
git worktree add /home/<you>/lc-before 27a6551~1
```

`27a6551~1` is the last commit before P1c; every commit between it and P1a's is documentation
only, so its library code is exactly the pre-RFC-020 state.

**2.2 — Put the *same* harness on both sides.** Copy the current
`crates/localcache/benches/scale_profile.rs` into the worktree, overwriting its copy. RFC 020
changed no public API, so the current harness compiles unchanged against the old library. This
matters because §4 adds a new measurement — both sides must run identical harness code, or the
comparison has two variables again.

**2.3 — Matched `TMPDIR`, not under the repository.** Same rule as P1a's R7: two directories of
equal path length, neither nested in the repo. Confirm with the harness's own
`example stored path length:` line that both sides print **the same number** before reading any
timing. A repo-nested path is what caused P1a revision 1's confound.

**2.4 — Interleave the runs.** Not all the "before" runs and then all the "after" runs — alternate
them, so any drift during the session shows up in both arms rather than loading onto one:

```
before, after, before, after
```

**Two runs of each arm at 1M is enough.** The projected effect is ~5×; the noise is ~1.24×. You do
not need heroic replication to separate those, and the cold section does not need re-running at
all — the change does not touch the scan.

## 3. The built-in control — use it

RFC 020 touches only `cleanup_missing_files` and `cleanup_expired`. **Every other operation in
the profile should be unchanged between the two arms.** That makes them a free, direct measurement
of this session's drift:

- If `field_gt`, the globs, `path_in_dir`, `get`, and populate all land within a few percent
  across arms, the session was stable and the cleanup delta is real.
- If they moved 20%, that is your noise floor, and the cleanup delta must be judged against it.
- **If any of them moved a lot, stop and report it** — RFC 020 should not have touched them, so a
  large move means something unexpected changed.

Report the unchanged operations in the table for exactly this reason. They are not padding.

## 4. Add a `cleanup_expired` measurement

RFC 020 R3 changed `cleanup_expired` identically and it has never been measured. Add it to the
harness, following the existing pattern for `preload` and the bincode section: **its own database
and its own fileset**, so it does not disturb the primary namespace's row counts.

Public API is enough — no test hooks:

1. Open an engine on its own DB with a short `ttl` (a second or two).
2. Populate it.
3. Sleep past the TTL. Report the sleep as setup, not as measured time.
4. Call `cleanup_expired` once and time it.

Keep it out of the primary engine: that engine has no `ttl`, and giving it one would change
`get_if_fresh` and every other measurement.

Measure it in **both** arms — it is a new measurement, but it has a before and an after, because
the old code is what the worktree holds.

## 5. What to report

**5.1 — The headline pair, at 1M:**

| Operation | before (`27a6551~1`) | after (`27a6551`) | ratio |
|---|---|---|---|
| `cleanup_missing_files`, warm | | | |
| `cleanup_expired` | | | |

Both arms, two runs each, medians with min–max.

**5.2 — The control block**, same table shape, for `field_gt`, both globs, `path_in_dir`, `get`,
`get_if_fresh`, populate, DB size per row. Expected: ~1.0× throughout.

**5.3 — All three scales** (10k / 100k / 1M) for `cleanup_missing_files`, so the improvement is
shown to hold across the range rather than only at the top.

**5.4 — The substrate block** each run already prints: filesystem, `TMPDIR` length, example stored
path length, DB size per row. Both arms must match.

## 6. What would falsify the RFC's model

The RFC projected the scan staying at ~0.672 s and the deletes dropping from ~5.089 s to ~0.415 s,
for **~1.09 s total at 1M**.

- **Close to ~1.09 s** — the model holds; record it and P1e proceeds.
- **Materially better** — say so; something else improved too, and it is worth knowing what.
- **Materially worse, or barely changed** — **report it rather than tuning `MAINTENANCE_CHUNK`.**
  The constant was measured against four strategies; if it does not deliver here, the cost model
  is wrong and should be re-derived, not patched. That is a better outcome than a constant quietly
  fitted to one run.

Per P1c's smoke check, ~129 ms at 100k against P1a's ~600 ms suggests the model is roughly right —
but that was a single warm run on a repo-nested `TMPDIR`, so treat it as a hint, not a prior.

## 7. Constraints

- **No library change.** P1d measures. Anything slow that turns up gets reported, not fixed.
- Harness changes limited to §4's addition. Do not restructure existing measurements — the control
  in §3 depends on them being identical to what the old arm runs.
- `LOCALCACHE_SCALE` still defaults to 10 000; the harness stays out of CI.
- Remove the worktree when done: `git worktree remove /home/<you>/lc-before`.
- Scratch directories outside the repo, removed after use.

## 8. Gates

- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` clean
- `cargo fmt --all --check` clean
- `python3 scripts/source_integrity.py --require-tracked` OK
- Full suite green, count reported (currently **416**)
- `git status --porcelain` clean of scratch residue

## 9. What happens next

If the model holds, **P1e is a patch release (v0.21.2)** — RFC 020 is non-breaking. Then RFC 020
moves to `rfcs/done/` with its Status carrying the version, and Phase 23 moves to **P2a**, the
JSON field query design RFC.

The deferred parallel-`stat` question (7.1× on the scan, ~62% of the post-fix cost) becomes
answerable once these numbers exist. **Do not implement it** — P1d's job is to make the decision
possible, not to make it.
