# Q19 Handoff — every single-feature doc build passes, and a gate keeps it so

Origin: the deferred-register row of 2026-11-04 (request 065) and review 097. Format of the review request: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6. Authorship: high-capability model. Authorized: the architect's maintenance remit (no behaviour, no API, no release).

## 1. The defect, reproduced by the architect

`RUSTDOCFLAGS="-D warnings" cargo doc -p localcache --no-deps --no-default-features --features encryption` fails: `unresolved link to Self::watcher` (`crates/localcache/src/cache/engine.rs`, the `rotate_encryption_key` rustdoc, line about 866, a link to a method that exists only with `watching`). The other nine single-feature rows of `localcache` and the three of `localcache-format` pass (checked 2026-11-07).

## 2. The change (two commits, local until reviewed)

1. `docs: no intra-doc link from encryption rustdoc to a watching-only method` — the link becomes a plain code span, as in the other three fixes of v0.24.0. Look for the same pattern elsewhere: any link in the rustdoc of an item compiled under feature A to an item compiled only under feature B. `git grep -n "Self::watcher\|Self::debounced_watcher\|CacheWatcher\|AsyncCacheEngine" -- crates/localcache/src` and judge each against its `cfg`; report the hits and verdicts.
2. `ci: gate-docs builds the docs with each single feature` — in `Makefile.toml`, `gate-docs` runs `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked` once per feature of `localcache` and of `localcache-format` with `--no-default-features --features <f>` (use the feature list the `gate-features-assert` task already holds, so a new feature cannot be forgotten). Time the added rows (they are small). Show the gate failing on the pre-fix tree (the defect above) and passing after.

## 3. Gates and the request

All gates of the previous slices on the committed tree (`gate-msrv` as `rustup run 1.85.0 cargo make gate-msrv`). One short request: the two hashes, the grep verdicts, the failing-before output, the added time. Do not push; the architect pushes the reviewed commits (they change no shipped behaviour and are released with the next release that has a reason of its own).

## 4. Not yours

The version, any CHANGELOG entry (a documentation-link fix needs none), `ROADMAP.md`.
