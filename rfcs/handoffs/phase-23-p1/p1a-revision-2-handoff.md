# P1a Revision-2 Handoff — Control the Path-Length Variable

Companion to `rfcs/handoffs/phase-23-p1/p1a-real-storage-measurement.md` and
`p1a-revision-handoff.md`.
Review: `.git-exclude/reviewed/architect-p1a-revision-review-2026-08-02.md`.

**Start here: R1–R6 were all done correctly, and the harness work is keepable as-is.** The
problem is a variable that changed underneath the measurements — and it changed because of a
relocation I approved as harmless. This is not rework of your engineering.

**What happened.** Between the original run and the revision, `TMPDIR` moved from
`/home/nabbisen/localcache-scale-tmp/…` (45 chars) into the repo at `…/.git-exclude/tmp/…`
(105 chars). Every entry stores its absolute path, in the `files` table **and** again in the
covering index `idx_files_namespace_path`. Longer paths → bigger table, bigger index, slower
scans.

You detected the symptom yourself and reported it — §4's note that DB size moved 1054.5 →
1250.9 B/row and populate 12.99 → 15.25 µs/row, flagged as unexplained. That was the right call.
The cause was your own §9 relocation, and **I reviewed that relocation and signed off on "no data
or result is affected."** That was my error to catch.

**Direct confirmation** — same host, same filesystem, same code, same scale (100k), only path
length differing (16 vs 95 chars):

| | short path | long path | ratio |
|---|---|---|---|
| DB size per row | 979.1 B | 1250.0 B | **1.28×** |
| `path_in_dir` | 8.45 ms | 14.46 ms | **1.71×** |
| `path_glob` wildcard | 9.60 ms | 13.22 ms | **1.38×** |
| `path_glob` literal | 3.02 ms | 3.08 ms | 1.02× |
| `field_gt` | 401 ms | 415 ms | 1.03× |

**Your headline is unaffected and still stands** — `cleanup_missing_files` is only ~1.04×
path-sensitive (bracketed by your own two runs), so the reversal is real in both cache states.
Only the multipliers move.

## R7 — Re-baseline tmpfs with the current harness at a matched path length

**The core fix.** Stop comparing against N4's stored numbers. Re-run the *current* harness on
tmpfs, so both halves of every ratio come from the same code and the same path length and differ
only in the filesystem.

This closes two confounds at once. The second has not been named before: **the revision compares
a corrected steady-state `get` (10.63 µs, first call excluded) against N4's uncorrected
200-sample mean (7.51 µs).** On tmpfs the dilution was only ~1.03×, so the distortion is small —
but it is the same class of error R1 just fixed, and re-baselining removes it for free.

Method:

1. Pick two directories whose **absolute path lengths match as closely as you can**, one on tmpfs
   and one on btrfs — e.g. `/tmp/lc-scale` and `/home/nabbisen/lc-scale`. Do **not** use a path
   under the repository; that is what caused this.
2. Run the current harness at all three scales on both.
3. Report both halves of every ratio from these runs. N4's numbers become historical context, not
   the denominator.
4. **Confirm the control worked**: DB size per row should now be near-identical between the two
   runs. If it is not, path lengths are still mismatched — fix that before reading any timing.

That last check is the whole point. DB size is deterministic, so it is a free, unambiguous
detector for exactly this class of error.

## R8 — Make the harness print what it depends on

Add to the substrate header, next to the filesystem line:

- the resolved `TMPDIR` **character length**
- one example stored path and its length

And keep printing DB size per row where it already is. The reasoning is the same as for
`filesystem_info()`: a profile that cannot say what it ran on is the defect P1a exists to fix, and
path length is now a known input to the results — so it belongs in the output, not in a reviewer's
reconstruction.

Consider adding it to limitation 5 as a named example: run-to-run variance is one cause of
irreproducible figures; **an uncontrolled input that looks like part of the environment is
another**, and this one changed a "deterministic" quantity by 18.6% without touching a line of
code.

## R9 — Re-derive §5 and §6 from the re-baselined runs

- **§5's table** — rebuild from R7. Expect the SQL-bound rows to come in near 1.0×.
- **§6's scoring** — with the confound gone the split should be **clean, not "cleaner than
  mixed"**: every SQL-bound operation flat, and only `cleanup_missing_files` moving 5–7×. Your
  hesitation in §6 — declining to call `path_in_dir`/wildcard "barely moved" while saying you had
  no confident mechanism — was the right instinct: there was no mechanism, because it was not a
  real-storage effect. Retire both as path-length artifacts.
- **§0's headline** — direction and framing are right; restate the multipliers from R7. Expect
  `cleanup_missing_files` ≈ 1.6× `field_gt` warm and ≈ 2.2× cold, with `field_gt` flat versus
  tmpfs.

## Note on R2, for next time

Your three repeats run on a **single population**, which measures within-run stability — genuinely
useful, and it is what I asked for. But the variance that produced the original outlier was
*across* populations, and path length is an across-population variable. Three tight repeats can
read as "replicated" while an uncontrolled input sits underneath all three.

Where a figure is compared against a different run, at least one full re-population is worth it.
No change needed now — R7's re-baseline is a fresh population by construction.

## What is not changing

Everything else stands: `timed_point_op` and first-call exclusion (R1), the `Some` assertions,
`LOCALCACHE_SCALE_QUERY_REPEATS` and the spread reporting (R2), limitation 5 (R6), the cache-drop
marker protocol, the trap-detector, `filesystem_info()`, `entry_count()`, §4.4's scoping-down,
cold-open, and the bincode figures. No library change.

Keep every existing log. The original two 1M runs are what made the first outlier detectable, and
the revision run is now the evidence for the path-length effect.

## Acceptance

- [ ] tmpfs and btrfs runs at matched path length, current harness, all three scales (R7)
- [ ] DB size per row near-identical across the two — the control confirmed before any timing is read (R7)
- [ ] `TMPDIR` length and an example stored path length printed in the harness output (R8)
- [ ] §5 table re-derived from the re-baselined pair (R9)
- [ ] §6 re-scored; `path_in_dir` and wildcard glob retired as path-length artifacts (R9)
- [ ] §0 multipliers restated; direction unchanged (R9)
- [ ] Standard gates re-run
