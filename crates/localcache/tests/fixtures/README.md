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

## RFC 010 historical migration fixtures

These fixtures are immutable inputs for the transactional-migration work in
RFC 010. Their executable SHA-256 and semantic checks live in
`tests/fixture_integrity.rs`. Tests must verify the digest before copying or
opening either database, and writable tests must operate on a temporary copy.
They contain synthetic data only and have no WAL/SHM sidecars.

### `compat-v0_1.sqlite3`

Generated through the public API of annotated tag `0.1.0`, peeled commit
`e44cfedc16cf96e3dfe97ad7ccbc1631b2398775`, using
`generators/compat_v0_1.rs`. The generator was compiled from an isolated
`git archive` of that commit with its committed lockfile. The archive was
mounted at `/fixture` while the generator ran, so its stored paths contain no
developer identity. The released API selected WAL; after the engine closed,
the fixture was checkpointed and switched to DELETE mode with SQLite before
being copied here.

- SHA-256: `bd0bb9ffb9e07abafebde2c8a492618bf23ba8cf0e8c29cd8a9a76a4f5153aac`
- Rust/Cargo: `rustc 1.97.1 (8bab26f4f 2026-07-14)`,
  `cargo 1.97.1 (c980f4866 2026-06-30)`, host and target
  `x86_64-unknown-linux-gnu`
- Locked crates: localcache 0.1.0, bincode 1.3.3, rusqlite 0.32.1,
  libsqlite3-sys 0.30.1
- Bundled SQLite runtime: 3.46.0; final checkpoint/journal-mode CLI: SQLite
  3.53.3
- Physical `user_version`: 0
- Objects: `files`, `payloads`, `idx_files_path`, `sqlite_sequence`, and the
  released UNIQUE autoindex
- Rows: file/payload IDs 1 and 3; `sqlite_sequence.files = 3`; both hashes are
  NULL; payloads decode as `[1.25, -2.5, 3.75]` and `[8.5, 13.0]`
- Stored paths: `/fixture/input-a.bin` and `/fixture/input-b.bin`

The exact isolated build/run operations were:

```sh
work="${RFC010_FIXTURE_WORKDIR:?}/rfc010"
mkdir -p "$work/v0_1/examples" "$work/v0_1/target/tmp"
git archive e44cfedc16cf96e3dfe97ad7ccbc1631b2398775 | tar -x -C "$work/v0_1"
cp crates/localcache/tests/fixtures/generators/compat_v0_1.rs \
  "$work/v0_1/examples/gen_rfc010_fixture.rs"
printf '\n[workspace]\n' >> "$work/v0_1/Cargo.toml"
env TMPDIR="$work/v0_1/target/tmp" cargo build --locked \
  --manifest-path "$work/v0_1/Cargo.toml" --example gen_rfc010_fixture
bwrap --die-with-parent --unshare-all --bind "$work/v0_1" /fixture \
  --dev /dev --proc /proc --ro-bind /usr /usr --ro-bind /etc /etc \
  --ro-bind /lib /lib --ro-bind /lib64 /lib64 --chdir /fixture \
  /fixture/target/debug/examples/gen_rfc010_fixture
sqlite3 "$work/v0_1/compat-v0_1.sqlite3" \
  'PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;'
```

### `compat-v0_19-user-index.sqlite3`

Generated through localcache 0.19.0's public `set` and `create_path_index`
APIs from annotated tag `0.19.0`, peeled commit
`6109f075bad0b830440d8ddd054a3c506fab5cde`, using
`generators/compat_v0_19_user_index.rs`. The isolated archive was likewise
mounted at `/fixture`.

- SHA-256: `585ea037ad94ef77696b3bb3c6d13d9778975057e2bdd7bdc5b01b299cfc86df`
- Rust/Cargo and host/target: same toolchain and target recorded above
- Locked crates: localcache 0.19.0, bincode 2.0.1, rusqlite 0.39.0,
  libsqlite3-sys 0.37.0
- Bundled SQLite runtime: 3.51.3
- Physical `user_version`: 4; DELETE journal mode
- Rows: file/payload ID 1, namespace `default`, raw bincode payload decoding
  as `[21.0, 34.0]`
- Public index: `lc_user_rfc010 ON files(namespace, path)`
- Stored path: `/fixture/input-user-index.bin`

The corresponding isolated build/run operations were:

```sh
work="${RFC010_FIXTURE_WORKDIR:?}/rfc010"
mkdir -p "$work/v0_19/examples" "$work/v0_19/benches" "$work/v0_19/target/tmp"
git archive 6109f075bad0b830440d8ddd054a3c506fab5cde | tar -x -C "$work/v0_19"
cp crates/localcache/tests/fixtures/generators/compat_v0_19_user_index.rs \
  "$work/v0_19/examples/gen_rfc010_fixture.rs"
printf 'fn main() {}\n' > "$work/v0_19/benches/cache_bench.rs"
env TMPDIR="$work/v0_19/target/tmp" cargo build --locked \
  --manifest-path "$work/v0_19/Cargo.toml" --example gen_rfc010_fixture
bwrap --die-with-parent --unshare-all --bind "$work/v0_19" /fixture \
  --dev /dev --proc /proc --ro-bind /usr /usr --ro-bind /etc /etc \
  --ro-bind /lib /lib --ro-bind /lib64 /lib64 --chdir /fixture \
  /fixture/target/debug/examples/gen_rfc010_fixture
```

Do not routinely regenerate either RFC 010 fixture. A deliberate replacement
requires architecture review, new provenance and digests, and corresponding
test updates.
