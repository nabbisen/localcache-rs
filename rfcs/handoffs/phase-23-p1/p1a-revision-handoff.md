# P1a Revision Handoff — Replicated Figures

Companion to `rfcs/handoffs/phase-23-p1/p1a-real-storage-measurement.md`.
Review: `.git-exclude/reviewed/architect-p1a-real-storage-measurement-review-2026-08-02.md`.

**Read this first: the engineering is not the problem.** The harness is close to right, the
cache-drop protocol is a good design, and no library code was touched. What needs fixing is the
sampling policy and the tables derived from it. Two of the six items are code; four are
re-deriving the report from figures that replicate.

**And the headline gets stronger, not weaker.** Once the outlier run is dropped,
`cleanup_missing_files` is the more expensive operation in the *warm* case too — the ranking does
not narrow, it reverses. The report was understating its own result.

## R1 — Discard the first sample in the point-operation loops

**Defect:** the reported `get` per-call figure is one first-access amortised over 200 samples,
not a per-call cost. At 1M the first call costs ~1.38 ms against a ~8.2 µs steady state — it
inflates the reported mean ~1.85×. That is why the profile shows `get` (21.57 µs) as *more*
expensive than `get_if_fresh` (8.85 µs), which is impossible: `get_if_fresh` runs the identical
lookup path plus an `exists()` check and a `detect_change` stat.

On tmpfs the same first call costs ~72 µs and inflates the mean 1.03% — invisible. This is a
second instance of exactly what P1a exists to catch.

**Change:** in each point-operation loop, time the first call separately and exclude it from the
mean. Report it as its own line — it is genuinely interesting (a first-access-after-population
cost, adjacent to §4.2's cold-open), just not a per-call figure.

Suggested output shape:

```
  get (warm hit)
    ↳ first call (excluded from mean)          1.381ms
    ↳ per call, samples 2..200                 8.161µs
```

Apply to `get`, `get_if_fresh` (warm and cold), and bincode `get`.

**Expected result:** steady-state `get` ≈ 8.2–8.9 µs, and `get`/`get_if_fresh` returns to ~0.96×
— the correct ordering. `get` vs N4 becomes ~1.1×, not 2.87×.

**While you are there:** the loops never assert what they returned. Add an assertion that the
probe lookup yields `Some`, so a silent early return can never be reported as a "metadata hit."
It currently does return `Some` — I verified — but nothing in the harness would notice if that
changed.

## R2 — Replicate every 1M figure; report median and spread

**Defect, and the most important one:** two runs of identical code on one host at one scale
disagree by up to 3.56×.

| Operation | `p1a-1m.log` | `p1a-1m-retry.log` | Ratio |
|---|---|---|---|
| `field_gt` | 4.024 s | **8.144 s** | 2.02× |
| `path_in_dir` | 61.4 ms | **218.6 ms** | 3.56× |
| `path_glob` wildcard | 68.2 ms | 69.4 ms | 1.02× |
| `cleanup_missing_files` warm | 6.622 s | 6.928 s | 1.05× |
| populate, per row | 12.93 µs | 12.99 µs | 1.00× |

Every table in the review request draws from the retry run — the one that disagrees. I
independently ran the harness's payload and setup twice more at 1M on the same filesystem
(database byte-identical at 1054539776, so the replication is faithful): `field_gt` came out
4.309 / 4.202 / 4.301 / 4.081 s across four passes. **Five measurements cluster at 4.0–4.3 s;
the reported 8.144 s stands alone.**

`path_in_dir` is worse — my run A gave 279 ms and 970 ms on two passes of the *same* run.

**Change:** at 1M, no figure derived from a single run. Minimum **three runs**; report median and
min–max. Where spread exceeds ~1.5×, say so explicitly next to the number rather than reporting
a median that looks precise.

**Do not chase the mechanism.** I could not establish why those two operations diverged and not
the others, and neither of us should write a guess into the record — the same discipline §4 of
your review request already applied, correctly. Replication makes the question moot for P1b's
purposes.

**Cost control:** you do not need three *full* runs. The queries are the unstable part; a mode
that populates once and repeats only the whole-namespace query block is enough, and my
replication shows within-run repeat passes are cheap. Do not re-run the cold section three times
— it needs the owner each time. Keep cold at one run and label it as such.

## R3 — Re-derive §3's comparison table

Rebuild from replicated medians, with the corrected `get` from R1. Expected shape:

| Operation | N4 (tmpfs) @1M | P1a (btrfs) @1M | Ratio |
|---|---|---|---|
| `get` (steady state, first call excluded) | 7.51 µs | ~8.2–8.9 µs | ~1.1× |
| `get_if_fresh` warm | 7.55 µs | 8.85 µs | 1.17× |
| `cleanup_missing_files` warm | 1.40 s | 6.6–6.9 s | ~4.8× |
| `field_gt` + `order_by` + `limit 25` | 4.381 s | ~4.2 s | ~0.96× |
| `path_in_dir` | 46.2 ms | replicate — erratic | — |

Withdraw `field_gt = 8.144 s` and `get = 2.87×`. Keep the 10k/100k tables — those figures are
consistent across runs and are not affected.

## R4 — Re-score the hypothesis

With the outlier removed, the SQL-bound / stat-bound split is **clean, not mixed**: `field_gt`
0.96×, literal glob 1.03×, wildcard glob 1.23×, `get_if_fresh` 1.17×, and
`cleanup_missing_files` 4.7–4.9× warm / 6.3× cold. The single operation that does a `stat` per
row moved 5–6×; nothing else moved much.

Two corrections to fold in:

- **`get` is not stat-bound** and should not be cited as evidence for the stat-bound side. It
  performs no `stat` — that is the entire difference between it and `get_if_fresh`.
- **The "unexplained `path_in_dir` 4.73×" dissolves.** Flagging it rather than rationalising it
  was the right call and is what made it catchable; now retire it, pending R2's replication.

## R5 — Restate the headline

The current headline ("the gap has nearly closed... under the cache-drop condition it inverts")
is too cautious. On replicated figures:

- `field_gt` ~4.2 s — **did not move** from tmpfs
- `cleanup_missing_files` 6.6–6.9 s warm, 8.868 s cold — **moved 4.7–6.3×**
- So `cleanup_missing_files` is **1.6× more expensive warm** and **2.1× cold**

On tmpfs `field_gt` was 3.1× the more expensive of the two. On real storage the relationship
**reverses outright**, in both cache states — not only the cold one.

State it that way. And keep the point that N4's §7 ranking was built on a figure known to be a
floor: the re-ranking is this milestone working, not a disruption to it.

**This does not retire P1b.** `field_gt` at ~4.2 s per million rows is still a real user-facing
ceiling. It is no longer the *largest* one.

## R6 — Record the variance as a harness limitation

Add to `scale_profile.rs`'s "Reading the results honestly" as limitation 5, and carry it into the
findings record.

N4's §5.3 called the harness "adequate for spotting a 115× effect; inadequate for detecting a 10%
regression" — a judgement made on tmpfs, where run-to-run variance is negligible. On real storage
that no longer holds: single-run 1M figures cannot distinguish a real 2× effect from noise. Both
of the report's headline surprises fell inside that band.

**This is the most valuable thing P1a found** — it outlasts every individual number in the table,
and it is what stops the same class of error from recurring in P1d's re-measurement. Give it the
prominence it deserves rather than a footnote.

## What is not changing

No library change. The cache-drop marker protocol, the trap-detector, `filesystem_info()`, §8's
`entry_count()` fix (I verified it — correct in both warm-only and cold paths), and §4.4's
scoping-down all stand as they are. Cold-open (796.7 ms) and the bincode figures have no tmpfs
baseline to be distorted by; keep them, labelled as single-run.

Keep both existing 1M logs. The timed-out first attempt was not required by anything I wrote, and
keeping it is the only reason the outlier was detectable at all.

## Acceptance

Re-review against this document plus the original QA checklist. The bar:

- [ ] Point-op means exclude the first call; first call reported separately (R1)
- [ ] Probe lookups assert `Some` (R1)
- [ ] 1M figures replicated ≥3×, median and spread reported (R2)
- [ ] §3 table re-derived; `8.144 s` and `2.87×` withdrawn (R3)
- [ ] Hypothesis re-scored; `get` no longer cited as stat-bound (R4)
- [ ] Headline restated as a reversal in both cache states (R5)
- [ ] Variance recorded as a harness limitation (R6)
- [ ] Standard gates re-run: clippy, fmt, `source_integrity.py --require-tracked`
