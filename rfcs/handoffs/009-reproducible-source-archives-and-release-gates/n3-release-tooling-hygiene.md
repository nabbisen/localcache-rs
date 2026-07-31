# N3 Implementation Handoff — Release and Security Tooling Hygiene

## 1. Summary

Phase 22 **N3**. Seven recorded findings against the release and advisory tooling, all
under `scripts/`. **No new RFC** — every item was raised and accepted in a prior review;
this handoff collects them and names the exact sites.

Nothing here changes library code, the schema, the payload wire format, or any public
API. It changes no gate's pass/fail semantics except where a finding says so explicitly.

## 2. Sequencing — read before starting

> **Discharged 2026-07-30.** N2 was committed at `ae07d79` and independently accepted.
> `scripts/check_advisories.py` is stable, so **all parts A–G may proceed** with no
> ordering constraint between them. The note below is retained as the record of why the
> constraint existed.

~~**Items D–G modify `scripts/check_advisories.py`, which N2 (RFC 019) is modifying right
now.**~~

- ~~**Do not start D–G until N2 is committed.**~~ Two people editing the same file for
  unrelated reasons produces a conflict neither can review cleanly, and both change the
  same `[implementations]` hash pin.
- **A–C touch `scripts/release.py` only** and were always independent of N2.

**Rebase before starting.** N2 changed `check_advisories.py` substantially — `expires`
is now `date | None`, and `classify_findings` reports standing dispositions. D's
`classification = {...}[entry.action]` change lands in code that N2 already edited, so
work from `ae07d79` or later, not from an older checkout.

## 3. Part A–C: `scripts/release.py`

### A — `command_version` merges stderr into parsed and evidence strings

`command_version` runs with `stderr=subprocess.STDOUT` and returns
`completed.stdout.strip()`. Those strings are then **both**:

- prefix-parsed by `verify_declared_toolchain` (`startswith(f"rustc {declared}.")`), and
- stored verbatim as R4 evidence fields in `toolchain_identity()`.

So any diagnostic written to stderr by `git`, `cargo`, `rustc`, or `mdbook` either fails
the MSRV check with a misleading message, or silently embeds noise into an evidence
field.

**This is the same defect class as RC-4**, one layer over. RC-4 fixed it inside
`run_gate` by adding an opt-in `separate_stderr`; `command_version` does not use
`run_gate` and was missed.

**Change:** capture stdout and stderr separately. Parse and store stdout only. Keep
stderr available for the error message when the command fails, so a genuine failure is
still diagnosable.

**Do not** simply discard stderr — a failing `mdbook --version` whose reason is on
stderr must still produce a useful `ReleaseError`.

### B — `target_triple` is not a target triple

`toolchain_identity()` builds it as
`f"{platform.machine()}-{platform.system().lower()}"`, yielding `x86_64-linux`. That is
not a Rust target triple; the real one is `x86_64-unknown-linux-gnu`, and `rustc -vV`
already reports it on its `host:` line.

RFC 009 R14 names "target triple" as a required evidence field, so the bundle currently
records a field it does not actually contain.

**Change:** derive it from `rustc -vV`'s `host:` line.

**Note:** `platform` already carries the richer
`Linux-7.1.5-1-cachyos-x86_64-with-glibc2.44`, so keep both — they answer different
questions.

### C — `rc_eligible` does not derive from anything

`rc_eligibility(clean_worktree=True, all_required_gates_passed=True,
evidence_complete=True)` is called with three hard-coded literals. The derivation lives
in control flow: the call is only reached after every prior step succeeded, so the
literals are true *today*.

RFC 017 R3 says `rc_eligible` **derives from gates**. It does not — and if a future
change ever writes a manifest after a partial failure, the literals will lie.

**Change:** thread the actual outcomes through, so the value is computed rather than
asserted.

**Constraint:** the behaviour must stay **fail-closed**. Today a failure raises before
the manifest is written, so no manifest is produced at all — which is stronger than a
manifest saying `false`. Do not weaken that: this change must not introduce a path that
writes a manifest with `rc_eligible: false` where it previously wrote none. If your
design would, stop and report.

## 4. Part D–G: `scripts/check_advisories.py` *(after N2)*

Verbatim from the RFC 014 M4 review, §H1–H4.

### D — H1: label accepted vulnerabilities distinctly

`classify_findings` renders every non-denied entry as `WARN` or `PASS`. **A knowingly
accepted vulnerability must never render as `PASS`.**

```python
classification = {"warn": "WARN", "exception": "EXCEPTION"}[entry.action]
```

and count exceptions separately in the `RESULT` line.

No entry currently uses `exception`, so this is pure signal with no effect on today's
graph — which is exactly why it should land **before** the first exception entry ever
exists. Update `docs/src/dependency_security.md` to document all three labels.

**Interaction with N2:** RFC 019 keeps `exception` mandatory-expiry for
`vulnerability`/`unsound`. D is complementary, not conflicting — but it is a second
reason to sequence after N2.

### E — H2: bounded retry on transient sparse-index failures

`live_fetch` / `build_registry_snapshot` make 233 sequential requests with zero
tolerance. Retry at most **3** times with exponential backoff, **only** for
`URLError`/`OSError` and HTTP 5xx.

**Never retry** a validation failure, a non-200/non-5xx status, or a size-limit breach.
Respect the existing `FETCH_DEADLINE_SECONDS` budget. Record the attempt count per
package in the manifest.

The rationale matters: a flaky security gate gets disabled, and a disabled gate is the
real risk. This makes it more reliable, not more permissive — do not let the retry
swallow a genuine failure.

### F — H3: record excluded packages in evidence

`load_registry_packages` silently drops path and git dependencies. Return them too, add
an `"excluded"` array to the registry manifest with `name`, `version`, and reason
(`path`/`git`), and extend the coverage line to
`233 locked crates.io packages, N excluded (path/git), 0 yanked`.

Keep the manifest additions inside the existing `require_exact_keys` validation — the
completeness claim becomes self-evidencing instead of requiring a manual `Cargo.lock`
cross-check.

### G — H4: one clarifying comment

Above `advisory_database()`: state that the path deliberately mirrors `cargo-audit`'s
own `CARGO_HOME` resolution, so the identity check describes the database actually used.

## 5. Explicit non-change scope

- No library, CLI, schema, migration, or payload change.
- No change to which advisory kinds map to `warn` versus `exception` — that is RFC 019's
  territory.
- No change to gate composition in `release.py`, or to which jobs CI requires.
- Do **not** renew or edit advisory expiry dates. RFC 019 removes them for
  `unmaintained`; N3 must not pre-empt or duplicate that.
- Do **not** bump versions.

## 6. Required tests

- **A:** a command emitting text on stderr but a valid version on stdout parses
  correctly and stores a clean evidence string. A command that *fails* still produces an
  error message containing its stderr.
- **B:** `target_triple` matches `rustc -vV`'s `host:` value exactly.
- **C:** the fail-closed property is preserved — a failing gate still produces **no**
  manifest, not a manifest with `rc_eligible: false`.
- **D:** a fixture with one `exception`-action vulnerability yields `EXCEPTION` and
  `exceptions=1` in the summary, and **exit status stays 0**.
- **E:** a simulated 5xx retries and then succeeds; a 404, a validation failure, and a
  size-limit breach each fail **without** retrying.
- **F:** a lockfile containing a path or git dependency produces a matching `excluded`
  entry, and the manifest still passes `require_exact_keys`.

## 7. Required evidence

- Full `scripts/tests` suite, counts before and after.
- The suite passing under **RC-3's restricted `PATH`** (no `cargo`/`rustc`/`mdbook`/
  `rustup`/`cargo-audit`) — this suite runs in a toolchain-free CI job.
- A **cold-`CARGO_HOME`** `release.py source` run (RC-4's standing requirement), since
  Part A touches version capture.
- `scripts/release-tools.toml` pins updated for every modified script, and a one-byte
  change to each still failing verification.
- Confirmation of whether N2 was already committed when you started (§2).

## 8. Recommended order

A → B → C (all `release.py`, independently reviewable), then D → G once N2 has landed.
Two review requests are acceptable and probably better than one; say which you are
doing.
