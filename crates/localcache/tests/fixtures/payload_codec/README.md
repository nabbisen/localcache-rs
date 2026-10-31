# Payload codec golden vectors (RFC 028)

The bytes the legacy payload format (RFC 008: the `bincode` 1 wire format, as
`bincode` 2 writes it with `config::legacy()`) produces for a fixed set of
values. They pin the owned codec in
`src/serialization/payload_codec` to the format that every stored payload in
every released version already has.

| File | What it is |
|---|---|
| `zoo.rs` | The type zoo: one value for every row of RFC 028's format table, and the nested combinations a payload can have. Shared by the generator and the test, so they cannot describe different values. |
| `generate.rs` | The generator. **Not compiled by the workspace.** |
| `vectors.txt` | The output: one line per case, `name = <lowercase hex>`; an empty encoding is `name =`. 118 cases. |

## Provenance

Generated once, offline, by running `bincode` itself. `bincode` is **not** a
dependency of any manifest in this repository, not even a dev-dependency (RFC 028
"Alternatives considered"); the vectors are the oracle.

| | |
|---|---|
| Generator | `generate.rs` + `zoo.rs` as committed (SHA-256 `b80cd75cb64ffdeaad46eb062036cf281a1afed5448eba4f321615ece3ce08d0` and `12d72f33cfd6c8b74a2693cc1d2024dbe3bfc469ddf029af8d9687ad5b8accf7`); the output of the committed sources is byte-identical to `vectors.txt` |
| `bincode` | `2.0.1` (`features = ["serde"]`), checksum `36eaf5d7b090263e8150820482d5d93cd964a81e4019913c972f4edcc6edb740` (the one in `Cargo.lock` before RFC 028 removed it) |
| Configuration | `bincode::config::legacy()` (little-endian, fixed-width integers, no limit) |
| `serde` / `serde_derive` | `1.0.228` |
| Toolchain | `rustc 1.99.1 (58cf8f9c3 2026-10-13)`, `--release` |
| Date | 2026-10-31 |
| `vectors.txt` SHA-256 | `f3c51b8244af1c439ee980049641ef231564f6ebfc73c841b8bee3fe4a50e129` |

The generator checks, before it writes a line, that `bincode` decodes its own
bytes to the same value and encodes that value to the same bytes. Running it
twice gave identical output.

### How to regenerate (only for a deliberate format change)

Regenerating the vectors is the loud act that signals a **wire-format change**.
It is never needed for a codec bug fix: if the test disagrees with a vector,
the codec is wrong.

```sh
mkdir -p <scratch>/payload-codec-vectors/src && cd <scratch>/payload-codec-vectors
cat > Cargo.toml <<'TOML'
[package]
name = "payload-codec-vectors"
version = "0.0.0"
edition = "2024"
publish = false

[workspace]

[dependencies]
bincode = { version = "=2.0.1", features = ["serde"] }
serde   = { version = "1", features = ["derive"] }
TOML
cp <repository>/crates/localcache/tests/fixtures/payload_codec/zoo.rs      src/zoo.rs
cp <repository>/crates/localcache/tests/fixtures/payload_codec/generate.rs src/main.rs
cargo run --release > vectors.txt
```

## How the test uses them

`src/serialization/payload_codec/tests.rs` compiles `zoo.rs` against the owned
codec and, for every case, asserts that

1. the value encodes to exactly the vector's bytes;
2. the vector decodes to the value, and the decoded value encodes to the same bytes
   (this is what compares a `NaN`'s payload bits);
3. every proper prefix of the vector is an error;

and that `vectors.txt` and the zoo have exactly the same case names.
