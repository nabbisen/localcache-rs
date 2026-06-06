# Test Fixtures

## `compat-v0_18.sqlite3`

A golden SQLite database written by localcache **v0.18.0** and committed
permanently into the repository.  `tests/compat.rs` opens it on every CI
run and asserts that all payloads decode to their expected values.

### What it contains

| Namespace | Path (stored) | Payload | Feature |
|---|---|---|---|
| `plain` | `/tmp/localcache_compat_a.bin` | `vec![1.0, 2.0, 3.0]` | always |
| `plain` | `/tmp/localcache_compat_b.bin` | `vec![4.0, 5.0, 6.0]` | always |
| `compressed` | `/tmp/localcache_compat_c.bin` | `vec![7.0, 8.0, 9.0]` | `compression` |

Codec: `Bincode` with `config::legacy()`.  Journal mode: `Delete`
(no WAL sidecars committed).

### Do NOT regenerate this file routinely

Regenerating the fixture is **the loud act that signals a wire-format
change**.  If you regenerate and commit a new fixture:

1. Old builds that still reference the old fixture will fail — that is
   *intentional*; the failure surfaces the breaking change.
2. You must update `tests/compat.rs` with the new expected values (or
   the new fixture file name).
3. You must add a CHANGELOG entry and bump the schema version.

To regenerate (only when a deliberate format change is needed):

```sh
cargo run --example gen_compat_fixture --features compression
```

Then commit the new fixture alongside updated tests and CHANGELOG.

### How the test reads it

`tests/compat.rs` copies the fixture to a `tempfile::TempDir` before
opening it (a read-only open of a Delete-journal SQLite file is fine, but
copying avoids any accidental write to the committed file).

The stored paths (`/tmp/localcache_compat_*.bin`) do not need to exist
on the test machine — `engine.query()` retrieves entries via stored path
strings without checking disk.
