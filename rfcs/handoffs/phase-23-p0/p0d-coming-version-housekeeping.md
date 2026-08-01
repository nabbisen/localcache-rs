# P0d Implementation Handoff — Coming-Version Housekeeping for v0.21.1

## 0. Why this exists as its own document

The reviewer told the owner that
`rfcs/handoffs/009-…/n6-coming-version-housekeeping.md` was "version-agnostic apart from the
target number" and could be reused directly. **That was wrong.** It contains 15 occurrences of
`0.21.0`, states the exact `0.20.1 → 0.21.0` pairs, and — more importantly — its § "Must NOT
update" list was built against the historical set as it stood on 2026-07-31. That set has
since changed: RFCs 018 and 019 now read `Implemented (0.21.0)`, and a v0.21.0 changelog
section and Phase 22 narrative now exist, all of which must be preserved.

Handing over a stale exclusion list is exactly the failure mode that list exists to prevent, so
this is a fresh document with the **current** set. The n6 handoff stays as the record of what
was done for v0.21.0.

## 1. Summary

Set the authorized coming version **v0.21.1** across everything that carries it, without
disturbing anything that records history.

**This is a patch release.** No breaking change, no migration note, no `#[non_exhaustive]`-style
caveat. P0's four accepted parts are a documentation addition, a lock-failure correctness fix,
release tooling, and internal test structure.

RFC 009 **R10/R11** governs this, and both gates run inside the `source` gate — so this must
land before the RC is produced.

## 2. Must update

### 2.1 — The workspace version

`Cargo.toml:16` — `version = "0.21.0"` → `"0.21.1"`.

Both members inherit via `version.workspace = true`. Do **not** add a version to either member
manifest.

### 2.2 — The three gated install examples

Each must read `localcache = "0.21.1"`:

| File | Line |
|---|---|
| `README.md` | 52 |
| `docs/src/getting_started.md` | 9 |
| `docs/src/introduction.md` | 7 |

These are exactly `VERSION_REFERENCE_TARGETS` in `scripts/release.py`, matched by
`^localcache = "([^"]+)"$`. Verified: that form appears in no other tracked file.

### 2.3 — The CHANGELOG heading

Add a **new** `## [0.21.1]` section above the existing `## [0.21.0] — 2026-08-01`. Do not edit
the 0.21.0 section.

Follow the established pattern: the date is set at **owner authorization, not before**, so use
the RC-placeholder form. `verify_changelog_has_coming_version_section` only requires a
`0.21.1` heading with a non-empty body.

The body should cover P0's four parts — the glob documentation, the `ConnectionPool` batch
length fix, the tooling hygiene, and the test deduplication. **State plainly that the
`ConnectionPool` change alters behaviour on the lock-failure path**, since that is the only
shipped behaviour change in the release.

### 2.4 — The ungated live version string

`scripts/check_advisories.py:555`:

```python
"User-Agent": "localcache-rfc014-security-gate/0.21.0",
```

Live, sent to crates.io on every advisory-gate run, and **outside** `VERSION_REFERENCE_TARGETS`
— no gate catches it. It went stale through all of Phase 22 before being noticed. Update to
`0.21.1`.

**This changes the file's SHA-256**, so `scripts/release-tools.toml`'s
`[implementations.check-advisories]` pin must be updated in the same commit or the gate fails
closed.

### 2.5 — `Cargo.lock`

Regenerates from the workspace bump. Confirm the diff contains **only** the two
`localcache`/`localcache-cli` version lines. A stray dependency change here would ride into the
release unreviewed — and we are two days past exactly that with `event-listener`.

## 3. Must NOT update — the current historical set

This list is rebuilt for v0.21.1. Getting it wrong is worse than missing an update, because it
falsifies the record.

- **`Cargo.toml`'s `localcache = { version = "0", path = ... }`** — the `^0` requirement,
  deliberate per the **2026-07-28 owner resolution** under RFC 009 R9.
- **`CHANGELOG.md`'s `## [0.21.0] — 2026-08-01` section** and every earlier release section.
- **`rfcs/done/018-truthful-error-taxonomy.md`** and
  **`rfcs/done/019-standing-dispositions-for-unmaintained-dependencies.md`** —
  `| Status | Implemented (0.21.0) |` is **correct history**. Those RFCs shipped *in* v0.21.0.
- **`rfcs/done/015-async-runtime-and-watcher-failure-safety.md:261`** — a forward reference to
  "the next breaking-change slot (0.21.0)", now satisfied. Historical.
- **`rfcs/README.md`** — the index rows recording 018/019 at v0.21.0.
- **`ROADMAP.md`** and **`docs/src/roadmap.md`** — every Phase 22 narrative.
- **`docs/src/errors.md`** — the `v0.21.0 migration note` heading and its prose. That migration
  is *from* 0.20.x *to* 0.21.0; it does not become a 0.21.1 note.
- **`docs/src/api.md:53`** — `### ReadPool poisoning (v0.21.0)`, same reasoning.
- **`rfcs/handoffs/**`** — records of what was true when written, including this directory's
  own P0 handoff and the n6 document.

**No RFC moves to `rfcs/done/` in this task.** P0 implemented no RFC; `rfcs/accepted/` is
already empty.

## 4. Prohibited shortcuts

- **Do not run a blanket `sed s/0.21.0/0.21.1/`.** It would rewrite RFC statuses, changelog
  history, roadmap narratives, and two migration-note headings into falsehoods — **and the
  version gate would still pass**, so nothing would catch it. There are 19 tracked files
  containing `0.21.0` and only five carry a value that should change.
- Do not change the `^0` CLI dependency requirement.
- Do not set a release date without owner authorization.
- Do not touch `scripts/release.py`'s gate composition.

## 5. Required evidence

- `verify_version_references(root, "0.21.1")` passes.
- `verify_changelog_has_coming_version_section(root, "0.21.1")` passes.
- `verify_implementations(root)` passes — proving the `check-advisories` re-pin is correct.
- `python3 scripts/release.py security` exits 0 **after** the User-Agent change and re-pin.
- `cargo package --workspace --locked` succeeds under stable.
- The `Cargo.lock` diff contains only the two version lines.
- A grep sweep for remaining `0.21.0`, **with every surviving occurrence identified as
  historical per §3**. List them; do not assert the sweep is clean, because it will not be —
  roughly fourteen files should still contain it afterwards.

## 6. What comes after

The full `source` gate cannot run until this is committed — it requires a clean tree, and the
review happens before the commit. That ordering constraint has appeared in every release so
far; it is expected, not a gap. The reviewer runs the full gate post-commit, then pushes and
confirms CI before the RC is produced.
