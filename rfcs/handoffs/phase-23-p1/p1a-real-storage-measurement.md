# P1a Implementation Handoff — Real-Storage Measurement

## 0. Naming note

Same deviation as `phase-23-p0/`: this is **phase-keyed, not RFC-keyed**, because P1a
implements no RFC — it is measurement that will *inform* P1b's RFC. RFC 000 reserves
`handoffs/NNN-slug/` for RFC companions. P1b's handoff will follow the convention once that
RFC exists.

## 1. Why this milestone exists

N4's scale profile (Phase 22) is the evidence base for all of Phase 23, and **one of its
numbers is knowingly wrong in a specific direction**. The measurement host put `TMPDIR` on
tmpfs — RAM — so every source-file `stat` ran at memory speed. `cleanup_missing_files` calls
`stat` per entry, and metadata-mode freshness checks do too.

The published figure is **1.40 s at 1M entries**, recorded as *"a floor, not an estimate"*.
P1a establishes what it actually costs on storage.

The stakes are concrete: `cleanup_missing_files` is currently **third** in the Phase 23
priority ranking. If real storage moves it materially, the ranking changes and P1b's design
work should be aimed differently.

## 2. Establish real storage — and record what it was

`/tmp` on this host is **tmpfs**. `/home` is **btrfs on a LUKS-encrypted volume**, with ~1.1 TB
free — so space is not a constraint even at 1M. Create the scratch directory under `/home` and
point `TMPDIR` at it:

```sh
LOCALCACHE_SCALE=1000000 TMPDIR=.git-exclude/tmp \
  cargo bench -p localcache --features json --bench scale_profile
```

> **Correction, 2026-08-03.** This example originally pointed `TMPDIR` at
> `/home/<you>/localcache-scale-tmp`, outside the project. **Scratch belongs in
> `.git-exclude/tmp/`.** The one place a repo-external path was genuinely required was P1a's
> revision-2 tmpfs-vs-btrfs pair — see the note in `p1a-revision-2-handoff.md` § R7 — and that
> reason does not generalize. Absolute figures taken under the longer repo path run ~1.2× higher
> on scan-bound operations; that cancels in any comparison where both sides share the location.

`scale_profile.rs`'s module docs **already instruct exactly this** — limitation #1 says "Set
`TMPDIR` to a directory on the target filesystem before drawing conclusions about I/O-bound
operations." N4 recorded the instruction and then did not follow it. That is the whole of this
milestone.

**Two things to be careful about:**

1. **Record the filesystem in the harness output.** The entire reason this milestone exists is
   that the substrate changed the meaning of the numbers and nothing in the output said so. Add
   the filesystem type and mount point of `TMPDIR` to the profile header — `stat -f -c %T` or
   equivalent. A profile that cannot tell you what it ran on is the defect we are fixing.
2. **btrfs-on-LUKS is not neutral.** Copy-on-write and encryption both add cost, and results
   will differ from ext4 or a bare SSD. That is fine — it is *a* real storage number rather than
   *the* real storage number — but say so in the profile, so a later reader does not treat it as
   universal.

Expect setup to be much slower than the tmpfs run: creating 1M small files on btrfs is not a
RAM write. Budget for it and do not mistake it for a regression in the measured operations —
setup is reported separately for exactly this reason.

## 2.5. The trap: moving off tmpfs may not be enough

**Read this before running anything.** It is the way this milestone most plausibly fails while
appearing to succeed.

The harness creates 1M files and then immediately `stat`s them. Every one of those inodes and
dentries is in the kernel's cache from having just been written. This host has **59 GB of RAM,
~25 GB available** — comfortably enough to hold the metadata for 1M small files and the ~950 MB
database. So `cleanup_missing_files` can run at close to memory speed **on btrfs**, and you will
have moved off tmpfs and still measured RAM.

If that happens and it is not noticed, P1a produces a number that looks like a real-storage
figure, is not one, and closes a milestone that exists precisely to stop that. **The failure
mode here is a plausible-looking result, not an error message.**

So report **two** figures for the `stat`-bound operations:

- **Warm** — as the harness runs today, caches hot from population.
- **Cold** — caches dropped between population and measurement.

Dropping the cache needs root:

```sh
sync && echo 3 | sudo tee /proc/sys/vm/drop_caches
```

**This is an owner action, not yours** — do not attempt to acquire privilege. If you cannot get
a cold run, say so explicitly and report the warm figure **labelled as still a floor**. An
honest "we could not drop the cache" is a good outcome for this milestone; an unlabelled warm
number is the failure it was created to prevent.

A useful sanity check either way: if `cleanup_missing_files` per-row cost lands near N4's
1.40 µs, you are almost certainly still measuring cache rather than storage. Treat that as a
signal to investigate, not as a result.

## 3. Re-run the existing profile unchanged

**Do not modify what the existing operations measure.** The value of this run is a clean A/B
against N4's numbers, and that only holds if the only variable is the storage.

Same three scales: **10k, 100k, 1M**. Same host. Report as a before/after table against N4's
published figures:

| Operation | N4 (tmpfs) @1M | P1a (btrfs) @1M | Ratio |
|---|---|---|---|
| `get` (warm hit) | 7.51 µs | | |
| `get_if_fresh` (metadata) | 7.55 µs | | |
| `cleanup_missing_files` (total) | 1.40 s | | |
| LRU eviction (total, ~450k evicted) | 932 ms | | |
| `path_in_dir` non-recursive (1000 rows) | 46.2 ms | | |
| `path_glob` leading literal (1000 rows) | 3.05 ms | | |
| `path_glob` leading wildcard (1000 rows) | 56.3 ms | | |
| `field_gt` + `order_by` + `limit 25` | 4.381 s | | |
| populate, per row | 10.71 µs | | |
| DB size per row | 950 B | | |

The row-count qualifiers matter — N4's §5.5 records that comparing a 1000-row query against a
1-row one once produced an exactly backwards conclusion. The harness asserts equal row counts
where it reports a ratio; keep that.

**A hypothesis worth stating so it can be wrong:** the SQL-bound operations
(`field_gt`, both globs, `path_in_dir`) should barely move, because SQLite's page cache absorbs
the difference once the database is warm. The `stat`-bound ones (`cleanup_missing_files`,
`get_if_fresh`) should move a lot. **If that split does not appear, the result is more
interesting than the one we expected** — report it prominently rather than as a footnote.

## 4. Add the four unmeasured operations

From N4 §6, in rough order of value:

**4.1 — `preload`.** Takes a callback, so it needs a different harness shape from the existing
one-shot timings. Measure the whole-namespace preload at each scale.

**4.2 — Cold-open cost.** How long does `CacheEngine::open` take against an existing 1M-entry
database, on a cold page cache? This is the number a user feels on process start, and nothing
measures it today. Dropping the page cache may need privileges — if you cannot do it cleanly,
**say so and measure what you can** rather than reporting a warm number as cold.

**4.3 — Bincode codec at scale.** The existing profile is JSON-only. `field_gt` cannot work on
bincode payloads at all, but `get`, `set`, `cleanup`, and eviction can — and bincode is the
default codec, so the profile currently measures the non-default path.

**4.4 — Concurrent access.** `ReadPool` with several slots, and the async backends. This is the
loosest of the four; if a meaningful measurement needs harness work disproportionate to the
value, **scope it down and say what you left out**.

**Keep additions additive.** Do not restructure the existing measurements to accommodate new
ones — a reader must be able to compare N4's table to yours line for line.

**Deliberately out of scope: watcher behaviour on large trees.** N4 §6 lists five unmeasured
items; the ROADMAP's P1a row scopes four. The watcher is excluded because it needs a
fundamentally different harness — sustained observation with induced filesystem events, not
one-shot timing — and RFC 015 already governs its failure behaviour. Do not add it. If the
real-storage run surfaces something that makes it look urgent, **report that** and it can be
scoped as its own milestone.

## 5. Constraints

- **No library change.** This milestone measures; it does not optimise. If you find something
  obviously slow, **report it — do not fix it.** A fix inside a measurement run destroys the
  baseline the measurement exists to establish.
- The harness stays a bench target with `harness = false`, not a Criterion suite, and is **not**
  wired into CI. Whole-namespace operations are measured once, not sampled.
- `LOCALCACHE_SCALE` keeps its 10_000 default so an accidental invocation stays cheap.
- Any new file must be tracked — `source_integrity.py` will not catch an untracked bench module.

## 6. Required evidence

- The before/after table from §3, all three scales.
- **Warm and cold figures for the `stat`-bound operations** (§2.5), or an explicit statement that
  the cold run was not possible and why — with the warm figure labelled as still a floor.
- The filesystem type and mount point recorded **in the profile output**, not only in the review
  request.
- Results for each of §4's four additions, or an explicit statement of what was not measured and
  why.
- Setup cost reported separately from measured operations, as the existing harness already does.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` clean;
  `cargo fmt --all --check` clean; `source_integrity.py --require-tracked` OK.
- Full test suite unchanged — this milestone adds no tests and should not alter any count.

## 7. What this feeds

P1b's RFC on the JSON field query. If P1a moves `cleanup_missing_files` above the query cost,
**say so plainly in the review request** — the Phase 23 ranking was built on tmpfs numbers, and
the reviewer would rather re-rank than have the design work aimed at the wrong target.
