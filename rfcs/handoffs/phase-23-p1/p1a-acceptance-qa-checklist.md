# P1a Acceptance & QA Checklist — Real-Storage Measurement

Companion to `rfcs/handoffs/phase-23-p1/p1a-real-storage-measurement.md`. This is what the
review will check. **A measurement milestone is accepted on the honesty and completeness of its
reporting, not on whether the numbers are good** — a result showing nothing changed is a pass, if
it is established rather than assumed.

## A. Storage substrate

- [ ] `TMPDIR` pointed at a **btrfs** path under `/home`, not the tmpfs `/tmp`
- [ ] Filesystem type and mount point recorded **in the profile's own output**, not only in prose
- [ ] The btrfs-on-LUKS caveat stated, so the numbers are not read as universal
- [ ] Free space confirmed sufficient before the 1M run

## B. The cache trap — the one that decides this milestone

- [ ] Warm figures reported for `cleanup_missing_files` and `get_if_fresh`
- [ ] Cold figures reported, **or** an explicit statement that the cache could not be dropped
- [ ] Where only warm figures exist, they are **labelled as still a floor** — not presented as
      real-storage results
- [ ] If per-row `cleanup_missing_files` cost landed near N4's 1.40 µs, that was investigated and
      explained rather than reported as a finding
- [ ] No attempt was made to acquire root privilege

**This section is not gradeable by inspection of the numbers alone.** State plainly what was
measured under which cache state. A run that cannot say which one it was is not acceptable.

## C. Comparability with N4

- [ ] All three scales — 10k, 100k, 1M
- [ ] Same host as N4
- [ ] Existing operations measure **the same thing** as before; no restructuring
- [ ] Before/after table with ratios, line for line against N4's §2 table
- [ ] Row-count qualifiers preserved; equal-row-count assertions still in place where a ratio is
      reported
- [ ] Setup cost reported separately from measured operations

## D. Hypothesis handling

The handoff predicts SQL-bound operations barely move and `stat`-bound ones move a lot.

- [ ] The prediction is scored explicitly — right, wrong, or mixed
- [ ] If the split did **not** appear, it is reported prominently, not as a footnote
- [ ] No result was quietly reconciled to the expectation

N4 scored its own four hypotheses and found two wrong; that record is why Phase 23 was scoped
from numbers. Keep the practice.

## E. The four additions

- [ ] `preload` — measured at each scale, or scoped down with a stated reason
- [ ] Cold-open cost — measured, or the privilege limitation stated
- [ ] Bincode codec at scale — measured for the operations it supports
- [ ] Concurrent access — measured, or explicitly scoped down with what was left out
- [ ] Watcher on large trees **not** added (deliberately out of scope)
- [ ] Additions are additive; N4's table remains comparable

## F. Scope discipline

- [ ] **No library change.** Nothing in `crates/localcache/src/` modified
- [ ] Anything slow that was found is **reported, not fixed**
- [ ] No test count change
- [ ] Harness remains `harness = false`, not wired into CI
- [ ] `LOCALCACHE_SCALE` still defaults to 10_000

## G. Standard gates

- [ ] `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` clean
- [ ] `cargo fmt --all --check` clean
- [ ] `python3 scripts/source_integrity.py --require-tracked` OK — any new file tracked
- [ ] Full suite green, count unchanged
- [ ] Scratch `TMPDIR` directory removed after the run; `git status --porcelain` clean of residue

## H. Reporting

- [ ] Limitations section, in the style of N4 §5 — what would make these numbers misleading
- [ ] A "not measured" section, in the style of N4 §6
- [ ] Any consequence for the Phase 23 ranking stated plainly (handoff §7)
- [ ] Judgement calls reported rather than absorbed

## What will not count against you

- Numbers that show no meaningful change from tmpfs. That is a real finding.
- An inability to drop the page cache, stated clearly.
- Scoping down §4.4 with an explicit account of what was omitted.
- Reporting that the ranking should change. The ranking was built on a figure we knew was a
  floor; re-ranking is a success of this milestone, not a disruption to it.
