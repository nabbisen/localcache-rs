# Q11 Handoff — The Owned Payload Codec (RFC 028), v0.22.1

RFC: `rfcs/accepted/028-owned-payload-codec.md` (accepted by the owner, 2026-10-31, D1 to D4 as recommended).
Release: **v0.22.1**, a fix release, after v0.22.0 is published. **Work in local commits; do not push** until the
architect says v0.22.0 is released (the book deploys from `main` on every push, and the release gate expects
`main` to be what was reviewed). The plan: `.git-exclude/reviewed/058-architect-v0.22.1-and-v0.23.0-plan-2026-10-31.md`.
Format of the review request: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6.
Authorship: high-capability model. Authorized: the owner.

## 0. What this is

Replace `serialize_bincode` and `deserialize_bincode` (`crates/localcache/src/serialization.rs`) with an owned
codec for the **same wire format**; remove `bincode`. The RFC is the specification; this file orders the work and
says what must be shown. **If anything contradicts the RFC, or step 1 finds a row of the format table that is
wrong, stop and file a design request; do not decide it in code.** Principles: *finally clean, safe and secure,
robust and sophisticated; less is more*. The decoder is the critical read path: simple, `unsafe`-free, no
`unwrap()`, no allocation from a claimed length.

## 1. Step 1 — verify the specification before writing any decoder (report it; this is the independent review)

The owner accepted the RFC before the dev-team review that plan 058 had asked for, so that review is this step.
In a scratch crate under `.git-exclude/tmp/` depending on `bincode 2.0.1` (`serde` feature,
`config::legacy()`), and **without touching tracked files**:

1. **The format table (RFC R1).** Encode a value of every row and of nested combinations (structs, tuples, enums with
   unit/newtype/tuple/struct variants, `Option<Option<T>>`, maps, `char` of 1 to 4 bytes, `u128`/`i128`, `usize`,
   `f32` NaN payload bits, empty collections). Report any row where the bytes differ from R1's table. Report what
   `bincode` does with `flatten`, `untagged`, `skip_serializing_if`, `#[serde(default)]` and
   `deserialize_any` types (`serde_json::Value`): the owned codec must give an error too.
2. **Trailing bytes (R2.5).** Show that `decode_from_slice` accepts trailing bytes and what
   `deserialize_bincode` does with the count it returns.
3. **Depth.** Probe a recursive enum (`enum List { Cons(Box<List>), Nil }`) with a chain of 10 MB of variant
   indices under a 512 MiB `RLIMIT_AS`: does `bincode` overflow the stack, and at what depth on an 8 MiB main
   thread, on a 2 MiB spawned thread, and on a `spawn_blocking` thread of each async runtime the crate
   supports (Tokio, async-std, smol)? Is the RFC's depth limit of 128 safe for every thread the crate uses
   given the decoder's own frame size? State the stack cost per level of your decoder once it exists (measure,
   do not guess).
4. **R5.** Confirm `bincode` accepts `vec![(); 10]`, and what a forged `u64::MAX` count for `Vec<()>` does
   (hang, abort, error). Grep every test, example and bench for a sequence or map whose element type occupies no
   bytes; report the result.
5. Anything in the RFC that is wrong, unclear or larger than it needs to be: say so.

**Stop rule:** if 1, 2 or 4 shows a difference from the RFC, or 3 shows that a depth of 128 is unsafe for a thread
the crate uses, file a design request (`.git-exclude/review-request/NNN-dev-design-request-rfc028-<topic>-<date>.md`)
and wait. Otherwise record the results in the request and continue.

## 2. Step 2 — the golden vectors

Generate, once, with the scratch crate, the type zoo's encodings and commit them as test data with provenance:
`crates/localcache/tests/fixtures/payload_codec/` (the bytes and a short README with the `bincode` version, the
configuration and the generator's source, like the existing fixtures' provenance notes). `bincode` is **not**
added to the repository's manifests, not even as a dev-dependency.

## 3. Step 3 — the codec

A private module (suggested: `crates/localcache/src/serialization/codec.rs` or `payload_codec.rs`; your choice,
report it), exposing only what `serialization.rs` needs: encode a `T: Serialize` to `Vec<u8>`, decode a
`T: DeserializeOwned` from `&[u8]`.

- **Encoder:** a `serde::Serializer` per R1; sequences and maps without a length are errors; output byte-identical
  to the golden vectors.
- **Decoder:** a `serde::Deserializer` over `&[u8]` with a cursor; the rules of R2 (lengths checked against the
  remaining input before anything is allocated or looped; depth limit 128; `u64` to `usize` checked; invalid
  tags, `char`, UTF-8, variant indices and truncation are errors; trailing bytes accepted). No
  `Vec::with_capacity(claimed)`: let serde's visitors size their collections from what is decoded.
- **Errors:** map to `LocalFileCacheError::Serialization(String)` exactly where `bincode`'s errors were mapped
  (`serialize_bincode`, `deserialize_bincode`); the variant does not change. Messages are yours; they must not
  echo attacker-controlled bytes beyond a bounded length.
- **Integration:** `bincode` removed from the workspace `Cargo.toml` and the crate's; `Cargo.lock` follows
  (list the lockfile diff: `bincode`, `bincode_derive`, `virtue`, `unty`, and anything only they used);
  the `RUSTSEC-2025-0141` entry removed from `deny.toml`; `cargo tree` shows `bincode` absent.
  `Codec::Bincode`'s name and doc comment stay (it is the format's name); update the comment that says
  `bincode::config::legacy()` to say the legacy `bincode`-1 wire format of RFC 008.

## 4. Step 4 — the evidence (RFC R6)

1. Golden-vector tests: encode equals the vectors, decode of the vectors equals the values, for the whole zoo.
2. **The differential run, once, offline** (scratch crate with `bincode` and the owned codec side by side; the
   harness is not committed, its source and logs are kept under `.git-exclude/tmp/`): at least **10^7** generated
   values per type (encode: bytes equal; decode of the bytes: values equal) and **10^7** random and mutated byte
   strings per type, with the rule: a successful `bincode` decode implies an equal successful owned decode
   (except R5), and **neither may panic, abort or allocate more than 4× the input length**. Report seeds, counts,
   duration, and every discrepancy (there should be none beyond R5).
3. **Every compat fixture** decodes unmodified (`tests/compat.rs` and `fixture_integrity`).
4. **Hostile tests** in `tests/`: the `String` length claim (remove the `#[ignore]` from
   `a_string_length_claim_is_an_error_not_an_allocation` and delete its reason string; it must pass under the
   512 MiB `ulimit -v` child, and the four types that already returned an error still do); a forged
   `Vec<u64>`, `HashMap`, `Option<String>` count; every truncation point of a valid value of every zoo type
   returns an error; invalid bool/Option/char/UTF-8/variant index; a recursive type with a 10 MB chain returns the
   depth error under a small `RLIMIT_AS` (same child mechanism).
5. A deterministic property-style round-trip test (own generator, no new dependency).
6. `a_deeply_nested_json_document_is_an_error_not_a_stack_overflow` and all of Q10's tests still pass.

## 5. Documentation and CHANGELOG

- `CHANGELOG.md`: a new section **above** `## [0.22.0]`, titled `## [0.22.1] — Unreleased`
  with `*Date set at owner authorization.*`, "A fix release. Not breaking.", and entries: **Fixed** (a forged
  length prefix can no longer abort a read; a recursive payload type can no longer overflow the stack), **Removed
  dependency** under `### Security` or `### Internal` as the file already orders them (`bincode`, and the standing
  advisory exception), and the one narrowing of RFC R5 stated plainly (a sequence or map of zero-byte elements
  whose count exceeds the remaining bytes is rejected), plus "error text for a malformed payload changed".
  Do not touch the `[0.22.0]` section: it describes the released version, including its Known issue.
  `scripts/check_versions.py` requires a section for the workspace version only; check it still passes before
  the version bump (the release preparation for 0.22.1 comes later).
- Book: `docs/src/security.md` (the known-issue paragraph becomes "fixed in v0.22.1", with the narrowing; the
  decoding-limits table; the dependency paragraph: the second standing exception is gone), 
  `docs/src/dependency_security.md` (the list of accepted unmaintained crates loses `bincode`; say why), `docs/src/errors.md`
  row for `Serialization`, and the places that say "via bincode" where it now means "the bincode wire format"
  (`README.md:106`, `docs/src/features.md:15`, `architecture.md`). Re-read each page you touch in full. Do not
  rename `Codec::Bincode` or the encoding tags.
- `ROADMAP.md` is the architect's.

## 6. Gates and the request

`cargo fmt --all --check`; `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`;
`cargo make gate-features` (the matrix, not only `--all-features`); `cargo test --workspace --all-features
--locked --no-fail-fast` (report the count; 600 before, 1 ignored becomes 0); `RUSTDOCFLAGS="-D warnings"
cargo doc --workspace --all-features --no-deps --locked`; `cargo make gate-versions`; `rustup run 1.85.0 cargo
make gate-msrv`; `cargo make gate-deny` (the exception is gone and nothing else changed); `cargo make
bench-compile`; `mdbook build docs`. Report the benchmark effect: run the `cache_bench` `get`/`set` rows for
`Vec<f32>` (64, 384, 1536) and the `scale_profile` populate/read rows before and after on the same quiet-ish host,
paired; the owned codec should not be slower than `bincode` by more than noise, and a regression beyond 10% is a
finding to report, not hide.

One review request, `.git-exclude/review-request/NNN-dev-q11-owned-payload-codec-<date>.md`, describing the
**uncommitted** tree; the step-1 findings first. Commit locally after approval (suggested split: the vectors; the
codec; the integration and removal; the docs), no push, tag, version bump or `ROADMAP.md` edit. List every
required step and every judgement call.

## 7. Not yours

A new format or tag; limits on payload size; `json`; anything about v0.23.0 (schema, AAD); the version bump and
the v0.22.1 release preparation (a later handoff); the date.
