# Changelog

The authoritative release history is
[CHANGELOG.md](https://github.com/nabbisen/localcache-rs/blob/main/CHANGELOG.md),
which carries an entry for every published version — what changed, whether it was
breaking, and which release candidate and decision it shipped under.

Per-version detail also appears on
[docs.rs](https://docs.rs/localcache) and
[crates.io](https://crates.io/crates/localcache).

## Why this page is a pointer

This page previously carried a hand-written feature summary. It stopped at v0.19.0
and stayed there through six subsequent releases, so a reader arriving here for
"what changed recently" got a confident answer that was badly out of date.

A summary maintained in parallel with the changelog has to be updated by hand at
every release, and no release gate checks it — the same shape of problem as the
advisory-gate User-Agent version string, which went stale across an entire phase
before anyone noticed. One authoritative record that the release process already
enforces is worth more than two records that can disagree.

Nothing was lost in removing it. Every release it described, and every release it
had missed, is in `CHANGELOG.md` in more detail — including the schema v5 mtime
precision fix in v0.20.0, which the old summary never reached.

## Where to look for specific things

| Looking for | Go to |
|---|---|
| What changed in a release | [CHANGELOG.md](https://github.com/nabbisen/localcache-rs/blob/main/CHANGELOG.md) |
| Whether an upgrade is breaking | That release's entry — each states this explicitly |
| Moving across a breaking change | [Migration Guide](./migration.md) |
| What is coming next | [Roadmap](./roadmap.md) |
| Measured performance characteristics | [Performance and Capacity](./performance.md) |
| MSRV and dependency constraints | [MSRV & Dependency Security](./dependency_security.md) |
