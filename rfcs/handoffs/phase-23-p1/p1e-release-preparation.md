# P1e Handoff — Release Preparation for v0.21.2

Milestone: Phase 23 **P1e**
Ships: RFC 020 (`rfcs/accepted/020-batched-maintenance-deletes.md`)
Two parts: **A** — coming-version housekeeping. **B** — correct the published performance figures.

**Neither part is optional, and B is the one that matters.** `docs/src/performance.md` currently
tells users to treat 1.40 s per million as a floor for the exact operation this release makes
5.9× faster. Shipping that unchanged would publish guidance we have measured and disproved.

**No release action here.** Tagging and publishing remain the owner's under RFC 009 R15.

---

# Part A — Coming-version housekeeping

Every line reference below was re-verified against the working tree today. **Do not carry numbers
over from `p0d-coming-version-housekeeping.md`** — that document is v0.21.1's and is historical.

## A.1 — The workspace version

`Cargo.toml:16` — `version = "0.21.1"` → `"0.21.2"`.

Both members inherit via `version.workspace = true`. Do **not** add a version to either member
manifest.

## A.2 — The three gated install examples

Each must read `localcache = "0.21.2"`:

| File | Line |
|---|---|
| `README.md` | 52 |
| `docs/src/getting_started.md` | 9 |
| `docs/src/introduction.md` | 7 |

These are exactly `VERSION_REFERENCE_TARGETS` in `scripts/release.py`, matched by
`^localcache = "([^"]+)"$`. Re-verified today with `git grep`: that form appears in **no other
tracked file**.

## A.3 — The CHANGELOG heading

Add a **new** `## [0.21.2]` section above the existing `## [0.21.1] — 2026-08-01`. Do not edit
the 0.21.1 section.

Date is set at **owner authorization, not before** — use the RC-placeholder form.
`verify_changelog_has_coming_version_section` requires only a `0.21.2` heading with a non-empty
body.

The body covers RFC 020. Points worth stating plainly:

- **A performance release, not a behaviour release.** `cleanup_missing_files` is ~5.9× faster and
  `cleanup_expired` roughly 8–12× faster at 1M entries; both now scan in pages and delete each
  page in one transaction instead of committing per row.
- **No public API, schema, or wire-format change.** Existing databases open unchanged.
- **One behaviour refinement on the error path:** partial progress on failure is now bounded to
  whole completed pages, where it was previously an arbitrary number of individually-committed
  deletes. Neither is all-or-nothing; the new behaviour is better defined.
- **A returned-count refinement observable only under concurrency:** the count is now rows
  actually deleted rather than rows attempted. If another writer removed a row between the scan
  and the delete, the old code counted a deletion that did not happen.

## A.4 — The ungated live version string

`scripts/check_advisories.py:555`:

```python
"User-Agent": "localcache-rfc014-security-gate/0.21.1",
```

Live, sent to crates.io on every advisory-gate run, and **outside** `VERSION_REFERENCE_TARGETS` —
no gate catches it. Update to `0.21.2`.

**This changes the file's SHA-256**, so `scripts/release-tools.toml:25`'s
`[implementations.check-advisories]` pin must be updated in the same commit or the gate fails
closed. Current value, verified today:
`1dd51aa240929f0aec323c80ec444cd8c5d11f639a89b0582ee86976e58bf94e`.

## A.5 — `Cargo.lock`

Regenerates from the workspace bump. Confirm the diff contains **only** the two
`localcache`/`localcache-cli` version lines. A stray dependency change riding into a release
unreviewed is exactly what RFC 014's gate exists to prevent.

## A.6 — Must NOT update

Rebuilt for v0.21.2. Getting this wrong is worse than missing an update, because it falsifies the
record. Every `0.21.1` occurrence in these files is **historical and stays**:

- `CHANGELOG.md` — the `## [0.21.1] — 2026-08-01` section and its body
- `ROADMAP.md` — all `0.21.1` references record what shipped
- `rfcs/handoffs/009-…/n6-coming-version-housekeeping.md` — historical
- `rfcs/handoffs/phase-23-p0/*.md` — P0 shipped in 0.21.1; historical

---

# Part B — Correct `docs/src/performance.md`

The published figures are wrong in **two** independent ways: they were taken on tmpfs, and they
predate RFC 020. Both are now fixed, and the document still carries the old story.

## B.1 — Replace the measured-scaling table

All figures below are from P1d's post-RFC-020 runs on **btrfs (LUKS-encrypted)**, entries stored
at **71-character absolute paths**, queries reported as the median of three repeats:

| Operation | 10k | 100k | 1M | Growth |
|---|---|---|---|---|
| `get` (warm hit) | 8.45 µs | 8.92 µs | 8.40 µs | **flat** |
| `get_if_fresh` (metadata mode) | 8.56 µs | 9.07 µs | 8.75 µs | **flat** |
| `path_glob`, leading literal | 3.07 ms | 2.97 ms | 3.17 ms | **flat** |
| `batch_set`, per entry | 12.89 µs | 12.71 µs | 13.36 µs | flat per entry |
| `cleanup_missing_files` (10% absent) | 9.63 ms | 108.8 ms | 1.08 s | linear |
| `cleanup_expired` (whole namespace) | 43.2 ms | 448 ms | 6.31 s | linear |
| LRU eviction, per evicted entry | 4.68 µs | 3.50 µs | 3.29 µs | flat per entry |
| `path_in_dir`, non-recursive | 3.66 ms | 8.94 ms | 60.8 ms | 17× |
| `path_glob`, **leading wildcard** | 3.62 ms | 9.49 ms | 68.9 ms | 19× |
| `field_gt` + `order_by_field` + `limit 25` | 37.0 ms | 386 ms | **4.00 s** | **108×** |

Storage: **980 / 1018 / 1031 bytes per entry** at the three scales.

**State the path length next to the storage figure.** Every entry stores its absolute path in the
table *and* again in the covering index, so per-entry storage and every index-scan timing depend
on how deep the cached files live. P1a measured 979 B/row at a 27-character path against 1251 B/row
at 106 — a 1.28× difference from path length alone. A reader planning capacity needs to know the
number is not universal.

## B.2 — Add `cleanup_expired` to the guidance

It is now measured and it is the most expensive maintenance operation, because it deletes the
whole namespace rather than a fraction. The existing guidance section mentions
`cleanup_missing_files` but not `cleanup_expired`.

## B.3 — Rewrite "What was not measured"

It currently lists `preload`, concurrent access, bincode at scale, watcher, and cold-open.
**P1a measured all but two.** The honest remaining list is:

- **watcher behaviour on large trees** — needs sustained observation with induced filesystem
  events, not one-shot timing
- **async-runtime backends** under concurrent load — `ReadPool` under 8 threads was measured;
  tokio/async-std/smol were not

If you want to publish what P1a *did* measure, `preload` costs ~71.5 µs per entry at 1M and a cold
`CacheEngine::open` against a 1M-row database costs ~0.6–0.8 s. Both are single-run figures; label
them as such or leave them out.

## B.4 — Replace the tmpfs caveat

The closing paragraph says the host kept the database on tmpfs and to "treat 1.40 s per million as
a floor." **Delete it.** It has been superseded by measurement, not merely made imprecise.

Replace it with what is now true and still limits the numbers:

- These are **real-storage** figures (btrfs on LUKS). Copy-on-write, checksumming, and encryption
  all add cost; ext4 or an unencrypted SSD will differ. This is *a* real-storage number, not *the*
  one.
- **One namespace holds every entry**, so operations narrowing only on `namespace = ?` scan
  nearly the whole table. That is the pessimistic case.
- `cleanup_missing_files` and `cleanup_expired` are **destructive and cannot be repeated within a
  run**, so their figures are single-sample and carry roughly ±15% session variance. Ratios
  measured as a same-session pair are reliable; absolute figures quoted across sessions are not.

## B.5 — Check the surrounding prose

`docs/src/api.md`, `docs/src/getting_started.md`, and `docs/src/changelog_summary.md` also mention
these methods. Check for any statement that a cleanup sweep is expensive, or any figure carried
over from the old table, and correct it. Report what you found rather than only what you changed.

---

# Part C — Publish the affected-version set

*(Added 2026-08-03, after a downstream report. Verification:
`.git-exclude/reviewed/architect-arama-0.19.1-defect-verification-2026-08-03.md`.)*

`docs/src/dependency_security.md` records the *cause* of the MSRV/`rusqlite` conflict and the two
cases that reported it, but **nowhere states which published versions are broken**. A consumer
resolving a broken version today gets `cannot find macro cfg_select` from a transitive build
script — no mention of localcache, MSRV, or `rusqlite` — and no published page names the version.

## C.1 — Add the affected-version table

In the § "Recorded cases" area, add a short subsection stating the set plainly:

**Affected published versions: `0.19.1` and `0.20.0`.** Both declare `rust-version = "1.85"` and
require `rusqlite ^0.40`, which resolves `libsqlite3-sys 0.38.x`, which uses the Rust 1.95
`cfg_select!` macro. Both fail to build on the baseline they declare.

Verified two ways on 2026-08-03: every published version enumerated from the sparse index, and
`localcache =0.19.1` compiled against `rust-version = "1.85"` with `cargo +1.85.0` — it fails in
`libsqlite3-sys 0.38.1`'s build script.

**Unaffected:** `0.19.0` and earlier (`rusqlite ^0.39`), and `0.20.1` onward (constrained back to
`^0.39`).

State that both are **yanked**, and that a yank prevents new resolution without affecting existing
lockfiles. Each caret range keeps a working version — `^0.19` resolves `0.19.0`, `^0.20` resolves
`0.20.1`, both verified to build on 1.85.0.

## C.2 — Correct the first recorded case

The 2026-08-01 row says the defect was *"Fixed here: `rusqlite` constrained to `^0.39` in v0.20.1"*.
True, but it implies a single broken version. Note that **`0.19.1` carried the same defect and was
identified later, by the same reporter**, and that the fix in 0.20.1 did not retroactively repair
the two published versions — which is why they are yanked.

## C.3 — Keep it short

This is a reference note, not a narrative. A reader arriving from a failed build wants the version
set, the reason, and where to go instead, in that order.

---

# Required evidence

- Version bump verified in `Cargo.toml`, `Cargo.lock`, and all three install examples
- `check_advisories.py` User-Agent updated **and** its pin re-computed in the same commit
- CHANGELOG `## [0.21.2]` section present, non-empty, undated
- The A.6 historical set confirmed untouched
- `docs/src/performance.md` figures traceable to the P1d logs — cite which log each row came from
  in the review request, not in the doc
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` clean
- `cargo fmt --all --check` clean
- `python3 scripts/source_integrity.py --require-tracked` OK
- Full suite green — **417 passed** as of `f56fa66`; report the number you observe, not this one

# What comes next

1. This work, reviewed
2. **Push, and confirm CI green on the exact tip** — owner-authorized; the RC must be produced on
   a pushed, CI-verified commit, in that order (the M6e lesson: a commit made between "confirm CI"
   and "produce the RC" reopens the gap)
3. RC production run against `scripts/release.py`
4. Release decision — mine
5. Tag, push the tag, `cargo publish --workspace --locked` — the owner's
6. RFC 020 moves to `rfcs/done/` with `Status: Implemented (0.21.2)`
