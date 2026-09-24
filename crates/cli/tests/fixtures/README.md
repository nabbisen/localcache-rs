# CLI test fixtures

The published `localcache-cli` package must build and test without the sibling
`localcache` crate's directory tree, so its fixtures live here and no test
reaches into `../localcache/`.

## `compat-v0_18.sqlite3`

A byte-for-byte copy of `crates/localcache/tests/fixtures/compat-v0_18.sqlite3`,
made on 2026-09-24 for RFC 024 R10 (`copy --from-db` with an old-schema source).
Its provenance is recorded in that crate's `tests/fixtures/README.md`: a golden
database written by localcache **v0.18.0** through the public API, committed
permanently, synthetic data only, journal mode `Delete` (no WAL/SHM sidecars).

- SHA-256: `9046c0d81ac51ba59ca45de0849a7955d3d4f04a92ff39adfd0042e35c9b31bb`
- Physical `user_version`: 4. The current schema is version 5, so a **read-only**
  open must refuse it and a **writable** open upgrades it.
- Contents: namespace `plain` holds two entries
  (`/tmp/localcache_compat_a.bin`, `/tmp/localcache_compat_b.bin`); namespace
  `compressed` holds one (`/tmp/localcache_compat_c.bin`). The stored paths need
  not exist on the test machine.

### Do NOT modify or regenerate it

Tests verify the digest above before using the file, and only ever operate on a
temporary copy. If the library's fixture is ever regenerated, this copy stays as
it is: it stands for "a database written by v0.18.0", not "the library's current
fixture".
