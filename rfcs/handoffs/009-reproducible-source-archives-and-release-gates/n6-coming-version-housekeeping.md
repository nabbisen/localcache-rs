# N6 Implementation Handoff — Coming-Version Housekeeping for v0.21.0

## 1. Summary

Phase 22 **N6**, first sub-task. Set the authorized coming version **v0.21.0** across
everything that carries it, without disturbing anything that records history.

This is RFC 009 **R10/R11** territory — the same work M6d did for v0.20.1 — and it must
happen **before** N6 constructs a release candidate, because the version-reference and
changelog gates run inside the `source` gate.

**No new RFC.** The version is authorized: N1's `#[non_exhaustive]` change is breaking, so
v0.21.0 is forced, and the owner confirmed it on 2026-07-30.

**I have already completed the CHANGELOG content** — entries for N1, N2, N3, N5, and the
`event-listener` security fix are written under `## [Unreleased]`. What remains is the
version heading and the mechanical propagation below.

## 2. Must update

### 2.1 — The workspace version

`Cargo.toml:16` — `version = "0.20.1"` → `"0.21.0"`.

Both members inherit through `version.workspace = true`; do **not** add a version to either
member manifest.

### 2.2 — The three gated install examples

Each must read `localcache = "0.21.0"`:

| File | Line |
|---|---|
| `README.md` | 52 |
| `docs/src/getting_started.md` | 9 |
| `docs/src/introduction.md` | 7 |

These are exactly `VERSION_REFERENCE_TARGETS` in `scripts/release.py`, matched by
`^localcache = "([^"]+)"$`. I confirmed that install-line form appears in **no other
tracked file**, so this list is complete.

### 2.3 — The CHANGELOG heading

`## [Unreleased]` → `## [0.21.0] — <date>`.

Follow the Phase 21 precedent: the section body is already written, and the **date is set at
owner authorization, not before**. If you are doing this before authorization, use the
RC-placeholder form — R11 accepts "its intended release date **or** an explicitly approved
RC placeholder", and `verify_changelog_has_coming_version_section` only requires the
`0.21.0` heading to exist with a non-empty body.

### 2.4 — An ungated live version string *(the one the gate will not catch)*

`scripts/check_advisories.py:535`:

```python
"User-Agent": "localcache-rfc014-security-gate/0.20.1",
```

This is a **live** string sent to crates.io on every advisory-gate run, and it is outside
`VERSION_REFERENCE_TARGETS`, so no gate will flag it. It went stale silently through the
whole of Phase 22. Update it to `0.21.0`.

**This edit changes the file's SHA-256**, so `scripts/release-tools.toml`'s
`[implementations]` pin for `check-advisories` must be updated in the same commit or the
gate fails closed on a hash mismatch.

### 2.5 — `Cargo.lock`

Regenerates from the workspace version bump. Confirm the diff contains **only** the two
`localcache`/`localcache-cli` version lines and nothing else — a stray dependency change
here would ride into the release unreviewed.

## 3. Must NOT update — historical or deliberate

Getting this wrong is worse than missing an update, because it falsifies the record. M6d's
checklist made the same point: *"deliberate compatibility ranges and historical changelog
entries are excluded explicitly, not silently rewritten."*

- **`Cargo.toml`'s `localcache = { version = "0", path = ... }`.** The `^0` requirement is
  deliberate, recorded in the **2026-07-28 owner resolution** under RFC 009 R9. Leave it.
- **`rfcs/done/*.md` `| Status | Implemented (0.21.0) |`** — wait: those currently read
  `Implemented (0.20.1)` and are **correct history**. RFCs 009–015 and 017 shipped *in*
  v0.20.1. Do not touch them. RFCs **018 and 019** are still in `rfcs/accepted/` and move to
  `done/` with `Implemented (0.21.0)` only when v0.21.0 actually ships — that is a separate
  step, not this one.
- **`rfcs/README.md`'s index versions** — historical.
- **`ROADMAP.md` and `CHANGELOG.md` historical entries** — the v0.20.1 section and every
  phase narrative.
- **`rfcs/handoffs/**`** — records of what was true when written.
- **`docs/src/querying.md:181`** ("the v0.20.1 schema") — a factual statement about which
  schema version introduced those indexes. The schema did not change in v0.21.0, so it stays
  accurate.
- **`scripts/release.py:72` and `:433`** — explanatory prose and a docstring citing the M6d
  incident. Historical.
- **`scripts/tests/test_release_runner.py`'s `_version_reference_fixture(..., version="0.20.1")`**
  — fixture input, deliberately arbitrary. Changing it tests nothing new.

### One judgment call I am leaving to you

`security/advisory-policy.json`'s two `reason` fields say *"Preserve the advertised v0.20.1
async-std runtime feature"* and *"…the established bincode legacy payload wire format for
v0.20.1"*. These are **live dispositions** whose stated reason names a superseded version.

Neither reading is clearly right: the reason records why the decision was made *then*
(historical), but the disposition is in force *now* (live). My preference is to drop the
version from both — the reasoning does not actually depend on it — but that edits a security
policy file, so **report what you did rather than deciding silently.** Note this also
re-pins nothing, since the policy file is not hash-pinned.

## 4. Required evidence

- `python3 -c "…verify_version_references(root, '0.21.0')"` passes.
- `verify_changelog_has_coming_version_section(root, '0.21.0')` passes.
- The full `source` gate passes on a clean tree — it runs both of the above plus the archive
  construction, and the archive filename must become `localcache-v0.21.0.tar.gz`.
- `python3 scripts/release.py security` still exits 0 **after** the User-Agent change and
  re-pin, proving the pin was updated correctly.
- `cargo package --workspace --locked` succeeds under stable.
- The `Cargo.lock` diff contains only the two version lines.
- A grep showing no remaining **live** `0.20.1` reference, with every surviving occurrence
  identified as historical per §3. List them; do not just assert the grep is clean, because
  it will not be.

## 5. Prohibited shortcuts

- Do not run a blanket `sed s/0.20.1/0.21.0/` across the tree. It would rewrite RFC
  statuses, changelog history, and roadmap narratives into falsehoods, and the version gate
  would still pass — so nothing would catch it.
- Do not change the `^0` CLI dependency requirement.
- Do not move RFC 018 or 019 to `rfcs/done/` in this task.
- Do not set a release date without owner authorization.
