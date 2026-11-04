# localcache-format

Implementation crate of `localcache`. Depend on `localcache`; this crate's API is not a separate stability promise.

## What it holds

The payload format: the codec (the bincode-1 wire format of RFC 028, written by the project and without `bincode` as a dependency), compression (`compression`, zstd), JSON (`json`), the table of encoding tags, and sealing and opening (`encryption`, AES-256-GCM bound to the entry's path), with `Key`, `Codec` and `FormatError`. It knows nothing of SQLite, files or caches.

The format's commitment is its golden vectors, `tests/fixtures/payload_codec/vectors.txt`: bytes written by `bincode` 2.0.1 for every case of a type zoo. A change that alters one of them silently corrupts every stored database, so `tests/fixture_digests.rs` pins the vectors, their generator and the zoo by digest.

## Checks that are not gates

Miri (the unit tests, with smaller sizes under `cfg(miri)`; needs a nightly toolchain with the `miri` component):

```sh
cargo +nightly miri test -p localcache-format --all-features
```

Fuzz targets (`from_slice`: the bincode-1 decoder through `PayloadFormat::decode`, and `open`: arbitrary bytes, tags and paths, with and without a key; needs `cargo install cargo-fuzz` and a nightly toolchain; `fuzz/` is its own workspace):

```sh
cd crates/localcache-format
cargo +nightly fuzz run from_slice -- -max_total_time=600
cargo +nightly fuzz run open -- -max_total_time=600
```
