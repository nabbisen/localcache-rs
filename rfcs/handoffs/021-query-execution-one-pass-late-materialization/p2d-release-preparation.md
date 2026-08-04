# P2d Handoff — Release Preparation for v0.21.3

Milestone: Phase 23 **P2d**
Ships: RFC 021 (`rfcs/accepted/021-query-execution-one-pass-late-materialization.md`)
Three parts: **A** — coming-version housekeeping. **B** — rebuild the performance figures.
**C** — one line in `querying.md`.

**No release action here.** Tagging and publishing remain the owner's under RFC 009 R15.

---

# Part A — Coming-version housekeeping

Every line reference re-verified against the working tree today. **Do not carry numbers over from
`p1e-release-preparation.md`** — that is v0.21.2's and is historical.

## A.1 — Workspace version

`Cargo.toml:16` — `version = "0.21.2"` → `"0.21.3"`. Both members inherit via
`version.workspace = true`; do not add a version to either member manifest.

## A.2 — The three gated install examples

Each must read `localcache = "0.21.3"`:

| File | Line |
|---|---|
| `README.md` | 52 |
| `docs/src/getting_started.md` | 9 |
| `docs/src/introduction.md` | 7 |

Exactly `VERSION_REFERENCE_TARGETS` in `scripts/release.py`. Re-verified with `git grep` today:
that form appears in no other tracked file.

## A.3 — CHANGELOG

New `## [0.21.3]` section above `## [0.21.2] — 2026-08-03`, which stays untouched. Date is set at
owner authorization — use the RC-placeholder form.

Body covers RFC 021. State plainly:

- **A performance release.** No public API, schema, or wire-format change.
- Query execution no longer fetches and decodes every candidate's payload before applying `limit`.
  One streaming query replaces a per-row `SELECT` pair, and payloads are decoded only for rows that
  survive ordering and limiting.
- **Two figures, not one** — they differ by 7× and quoting a single number would misrepresent both:
  a `limit`-only query is **~15× faster** at 1M entries; a JSON field query with `order_by_field`
  is **~1.9× faster**.
- `dry_run()` now also reports which execution path a query will take.
- Result ordering is unchanged, including ties.

## A.4 — The ungated live version string

`scripts/check_advisories.py:555` — `"…-security-gate/0.21.2"` → `0.21.3`. Outside
`VERSION_REFERENCE_TARGETS`; no gate catches it.

**This changes the file's SHA-256**, so `scripts/release-tools.toml:25`'s
`[implementations.check-advisories]` pin must be updated in the same change. Current value, verified
today: `99235ed80adce72b78d6d68149d1077f42448ecbc8b216c66410a2a956ff4519`.

## A.5 — `Cargo.lock`

Regenerates from the bump. Confirm the diff is **only** the two version lines.

## A.6 — Must NOT update

Every `0.21.2` occurrence in these is historical and stays: `CHANGELOG.md`'s `## [0.21.2]` section,
`ROADMAP.md`, and `rfcs/handoffs/**` (including `p1e-release-preparation.md`).

---

# Part B — Rebuild `docs/src/performance.md` from ONE fresh run

## B.1 — Do not splice P2c's numbers into the existing table

**This is the instruction that matters most in Part B.**

The published table was built from P1d's runs at a **71-character** stored path (1031 B/row).
P2c ran at a **146-character** stored path (1292 B/row). Path length is stored twice per entry —
in the table and again in the covering index — so it changes database size and every scan-bound
timing. P1a measured `path_in_dir` moving **1.71×** from path length alone.

Inserting P2c's query rows into P1d's table would silently mix two substrates, which is precisely
the confound P1a spent three review rounds removing.

**Instead: run the full profile once, on the current code, at one path length, and rebuild every
row of the table from that single run.** Same requirement as P1a's R7 — confirm the harness's own
`TMPDIR path length` and `example stored path length` lines are identical across the three scales
before reading any timing, and state the path length in the document next to the storage figure, as
the current page already does.

Scratch under `.git-exclude/tmp/`.

## B.2 — The table gains a row and a distinction

Add the **tier-1 query** (`limit(25)`, no field predicate) that P2c added to the harness. The page
currently shows only the field query, which is the slower of the two by 7×.

Present them so a reader sees they are different shapes, not one number with variance — a
`limit`-only query is bounded by the limit, a field query scans the namespace.

## B.3 — Explain why, briefly

The existing "Why the last row is so slow" section explains the field query's cost as JSON
extraction and sorting. **That explanation is now wrong**, and RFC 021's motivation section has the
correct one: most of the cost was fetching and decoding every candidate's payload, which is now
fixed; what remains is that SQLite's `json1` is not streaming, so evaluating a JSON field parses the
whole stored document once per candidate.

Keep it short — a reader wants the shape of the cost and what to do about it (`limit` alone is
cheap; a field query over a whole namespace is not).

## B.4 — Carry the caveats forward

The "Limits of these numbers" section stays as-is: real-storage-but-one-filesystem,
single-namespace-is-pessimistic, and the destructive-operations-are-single-sample note. All three
still apply.

---

# Part C — `docs/src/querying.md`

Line 206 says `dry_run()` runs `EXPLAIN QUERY PLAN` on the path-listing SQL only. Still true of the
first line of output, but it now also reports the execution path R4 added. Line 79's table entry
(`Return EXPLAIN QUERY PLAN output without loading payloads`) needs the same treatment.

One or two sentences. Do not restructure the page.

---

# Required evidence

- Version bump verified in `Cargo.toml`, `Cargo.lock`, and all three install examples
- `check_advisories.py` User-Agent updated **and** its pin re-computed in the same change
- CHANGELOG `## [0.21.3]` present, non-empty, undated
- A.6 historical set confirmed untouched
- **Every `performance.md` figure from one run**, with the path length stated — cite the log in the
  review request, not in the document
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` clean
- `cargo fmt --all --check` clean
- `python3 scripts/source_integrity.py --require-tracked` OK
- Full suite green — report the number you observe (425 as of `ed9b1d6`; report yours)

# What comes next

1. This work, reviewed
2. **Push, and confirm CI green on the exact tip** — owner-authorized; the RC must be produced on a
   pushed, CI-verified commit, in that order
3. RC production run
4. Release decision — mine
5. Tag, push the tag, `cargo publish --workspace --locked`
6. RFC 021 → `rfcs/done/` with `Status: Implemented (0.21.3)`
