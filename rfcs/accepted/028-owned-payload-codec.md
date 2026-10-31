# RFC 028 — An Owned, Bounds-Checked Codec for the Legacy Payload Format

| Field | Value |
|-------|-------|
| Status | Accepted (owner, 2026-10-31), D1 to D4 adopted as recommended |
| Feature | *(core)* |
| Touches | `crates/localcache/src/serialization.rs`, a new private module `payload_codec`, `Cargo.toml` (removes `bincode`), `deny.toml`, `docs/src/` (security model, dependency security) |
| Release | **v0.22.1** (a fix release: no API, schema, wire-format or MSRV change) |
| Closes | the v0.22.0 Known issue (review 056); ROADMAP deferred register |

## Summary

Replace the two functions that call `bincode` (`serialize_bincode`, `deserialize_bincode`) with a small codec of our
own for the **same wire format**, whose decoder checks every length it reads against the bytes that remain and
bounds its recursion. Remove the `bincode` dependency and its advisory exception. Stored data and written bytes do
not change.

## Motivation

- **A reproduced abort.** A stored uncompressed payload whose 8-byte length prefix is forged can make `get`
  abort the process when the payload type contains a `String` (exit 134, `memory allocation of
  2305843009213693951 bytes failed`, design request 043). `bincode 2.0.1` decodes `String` as
  `vec![0u8; claimed_len]` under `NoLimit`. Anyone who can write the database file, or an imported export, can
  trigger it; the entry is not lost. It ships as a Known issue in v0.22.0.
- **It cannot be fixed from outside.** `bincode::de::Decoder` is `Sealed`: a crate cannot wrap it to check a claim
  against the remaining input. Its only limit is a **const** (`with_limit::<N>()`), which cannot follow the input
  and would half-close the hole (review 056).
- **Upstream will not fix it.** `bincode` is unmaintained (RUSTSEC-2025-0141). The standing exception in
  `deny.toml` keeps it "to preserve the legacy payload wire format", and its follow-up condition reads: *reassess
  if a maintained format-compatible implementation becomes available*. An owned codec is one.
- **A second abort is latent for the same reason.** A recursive payload type (`enum List { Cons(Box<List>),
  Nil }`) decodes one level per four bytes, so a 64 MiB hostile payload can recurse millions of levels and
  overflow the stack. As far as the source shows, `bincode` applies no depth limit to nested values; `serde_json` does (128). Not yet probed: the implementation's first step is to probe it (R6.5).
- **The surface is tiny.** `bincode` is used in exactly two functions. The format is fixed forever by RFC 008.

## Goals

1. No stored byte sequence can make a read abort, allocate more than a constant multiple of its own length, or
   overflow the stack. It returns an error.
2. **Every byte sequence `bincode` 2.0.1 (legacy configuration, serde mode) decodes successfully, this codec
   decodes to the same value**, with one stated exception (R5). Every value encodes to the **same bytes**.
3. Fewer dependencies: `bincode` leaves the tree; the `RUSTSEC-2025-0141` exception is deleted.
4. No API change. `Codec::Bincode` keeps its name and meaning.

## Non-goals

- A new format, a new tag, a version marker: the format is RFC 008's and does not change.
- Limits on payload size (`max_decompressed_bytes` is Amendment 2's and covers decompression only).
- Changing `json`. `serde_json` is maintained and already bounds its depth.

## Requirements

### R1 — The format (specification of what exists, not a design)

Little-endian, fixed-width. Serde data model mapped as `bincode` 2 `config::legacy()` does:

| Serde type | Bytes |
|---|---|
| `bool` | 1 byte, `0` or `1` (other values are an error) |
| `u8`/`i8` … `u64`/`i64`, `u128`/`i128` | 1, 2, 4, 8, 16 bytes |
| `usize`/`isize` | encoded as `u64`/`i64` (serde presents them as `u64`/`i64`) |
| `f32`/`f64` | IEEE-754 bits, 4/8 bytes |
| `char` | UTF-8, 1 to 4 bytes (length from the first byte); an invalid sequence is an error |
| `str`, `String`, bytes | `u64` length, then that many bytes; `str` must be valid UTF-8 |
| `Option<T>` | 1 byte `0` (None) or `1` (Some), then `T` |
| unit, unit struct | no bytes |
| newtype struct | the inner value |
| sequence, `Vec<T>` | `u64` count, then the elements |
| map | `u64` count, then key, value pairs |
| tuple, tuple struct, struct | the fields in order, no prefix |
| enum | `u32` variant index, then the variant's fields |

Not supported, as in `bincode` (the format is not self-describing): `deserialize_any`, `deserialize_ignored_any`,
and serialization of a sequence or map of unknown length. They return an error.

The implementation first checks this table against `bincode`'s behaviour on a **type zoo** (every row, nested;
serde attributes that break non-self-describing formats, such as `flatten`, `untagged` and
`skip_serializing_if`, are recorded as the errors `bincode` gives, and the owned codec must give an error too,
never a panic) before a line of the decoder is written; any row that differs from the table is a finding and
the table is corrected first.

### R2 — Hostile input (the decoder's rules)

1. **Every claimed length is checked against the remaining input before anything is allocated or looped:** a
   `str`/`String`/bytes length greater than the bytes left is an error; a sequence or map count greater than
   the bytes left is an error (see R5 for the one case this narrows). No `Vec::with_capacity(claimed)`: collections
   grow as elements are decoded, with serde's cautious pre-allocation.
2. **Recursion depth is limited to 128**, as `serde_json` does. Deeper input returns an error.
3. A `u64` length that does not fit `usize` is an error (32-bit targets).
4. Invalid `bool`/`Option` tags, invalid `char`, invalid UTF-8, an enum variant index the type does not have, and
   truncation are errors. **No `unsafe`.** No `unwrap()` (the crate's lint).
5. Trailing bytes after a complete value are accepted, as `bincode`'s `decode_from_slice` does (it reports the
   count read and the current caller ignores it). The RFC keeps that behaviour (decision D2).

### R3 — The encoder

A `serde::Serializer` writing to a `Vec<u8>` per R1. A sequence or map without a length is an error. Its output
is **byte-identical** to `bincode`'s for every value in the type zoo (R6). It is not hostile-input code and is as
small as the table.

### R4 — Integration

`serialize_bincode` / `deserialize_bincode` call the owned codec; `bincode` is removed from `Cargo.toml` and
`Cargo.lock`; the `RUSTSEC-2025-0141` entry is removed from `deny.toml` (the stale-ignore gate would fail if it
stayed). The doc comments that cite `bincode` (`Codec::Bincode`, `serialization.rs`) say "the legacy `bincode`-1
wire format" and name RFC 008. Error text for a malformed payload changes (`Serialization("…")`); the variant does not.

### R5 — The one deliberate narrowing

A sequence or map whose **elements occupy no bytes** (`Vec<()>`, `HashMap<(), ()>`) cannot be told apart, byte
for byte, from a forged count: both claim `n` elements and supply none. The decoder rejects `n` greater than the
bytes left. A payload `vec![(); 10]` therefore no longer decodes, where `bincode` accepted it. This is the only
case where the owned decoder rejects what `bincode` accepted. It is rejected on purpose, because the alternative
(accepting a count of 2^60 empty elements) is a CPU abort of the same class as the allocation. No known payload
uses such a type; the Known-issue text of the release says so. (Decision D1.)

### R6 — Evidence required (the acceptance criteria)

1. **The type zoo and the table** (R1), in a committed test file, with the bytes `bincode` produced written down as
   golden vectors (generated once, offline, with a scratch crate that depends on `bincode`; the vectors and the
   generator's source are committed, `bincode` is not).
2. **A differential run, once, offline**, between `bincode` and the owned codec: at least 10^7 generated values per
   type (encode: bytes equal; decode of the bytes: values equal) and at least 10^7 random and mutated byte strings
   per type (a successful bincode decode implies an equal successful owned decode, except R5; a failed one may be
   either; **neither may panic, abort or exceed 4× the input length in allocation**). The result, with seeds and
   counts, goes in the review request. It is not repeated in CI.
3. **Every compat fixture** (databases written by released versions) decodes unmodified.
4. **The acceptance tests that exist:** `a_string_length_claim_is_an_error_not_an_allocation` loses its `#[ignore]`
   and passes; the four types that already returned an error still do.
5. **New hostile tests:** a recursive type with a 10 MB chain of variant indices returns the depth error under a
   small `RLIMIT_AS`; a forged `Vec<u64>`/`HashMap`/`Option<String>` count; every truncation point of a valid
   value of every zoo type returns an error; invalid tags and UTF-8.
6. A property-style test, with a deterministic generator (no new dependency): encode then decode returns the value.

## Test plan

R6, plus the existing suite unchanged, plus `gate-deny` (the exception is gone and nothing else changed), plus
`cargo tree` showing `bincode` absent.

## Security considerations

This closes the abort in the Known issue and the latent stack overflow, and removes an unmaintained dependency.
It adds roughly 500 lines of parsing code on the critical read path, which is the cost: the mitigation is R6
(an oracle that is `bincode` itself, a type zoo, fixtures from released versions, and hostile tests), no
`unsafe`, and a decoder that never allocates from a claimed length. An error in the decoder would be a wrong
value or a wrong error, not memory unsafety.

## Compatibility

Wire format unchanged in both directions: new writes are readable by every older version (byte-identical), old
data reads unchanged. API unchanged. One narrowing (R5). Error text for malformed payloads changes. Binary size
shrinks slightly. Not breaking: **v0.22.1**.

## Alternatives considered

- **Keep `bincode` and accept the Known issue.** Rejected: it is the kind of defect this release series exists to
  remove, and the crate is unmaintained.
- **`with_limit::<N>()`.** Rejected: a const; half-closes the hole; magic number (review 056).
- **Dispatch over size classes of `N`.** Rejected: monomorphizes every user type about twenty times.
- **Fork or vendor `bincode`.** Rejected: more code, an unmaintained base, a supply-chain claim to carry.
- **A different codec for new writes (`postcard`, `rmp`).** Rejected: breaks rollback (older versions could not
  read new payloads) and needs a format tag; it solves a different problem.
- **Keep `bincode` as a dev-dependency oracle.** Rejected: `cargo deny` scans dev-dependencies, so the advisory
  exception would stay; the format is frozen, so golden vectors plus a one-time differential run carry the same
  assurance without the dependency.

## Rollback

A revert restores `bincode`; the bytes on disk are the same either way.

## Decisions

Accepted by the owner on 2026-10-31 as written, **with the four recommended answers**:
1. **D1 — R5:** a count greater than the remaining bytes is rejected, including for zero-byte element types.
2. **D2 — R2.5:** trailing bytes after a complete value are still accepted.
3. **D3 — scope:** the encoder and the decoder are owned, and `bincode` is removed entirely.
4. **D4 — release:** v0.22.1, after v0.22.0.

The owner accepted before the independent review the architect had asked of the dev team (plan 058, gate B).
That check is therefore not skipped: it becomes **step 0 of the implementation** (handoff
`../handoffs/028-owned-payload-codec/q11-owned-payload-codec.md`), with a rule that a finding stops the work and goes
back to the owner before any code is written.

## Open questions

None. The depth limit of 128 follows `serde_json`; if the type zoo shows a legitimate payload deeper than that, R2.2
is revisited before implementation.

## Amendment 1 (architect, 2026-10-31) — what the independent verification found

Found by the dev team's step 1 (request 046), reviewed by the architect (review 060). The RFC's decisions D1 to D4 are
unchanged; these close gaps the verification found in its rules.

1. **R1 omission: `is_human_readable()` is `false`** in both directions (`Ipv4Addr` is four bytes, not text), as in
   `bincode`. Pinned by golden vectors.
2. **F1 — the encoder refuses what the decoder would refuse (R3).** A value nested deeper than the depth limit is
   an encode error with the same error as the decoder's. An engine never stores what it would refuse to read (the
   principle of Amendment 2 of RFC 025, D4). Without it `set` succeeded and `get` failed.
3. **F2 — a per-decode count budget (R2.1).** R2.1 bounds each sequence or map count by the bytes left, which does not
   bound their **sum**: a type nesting sequences of zero-byte elements (`Vec<Vec<()>>`) took quadratic time (128 KB of
   input: 0.094 s; each doubling costs 4×). The sum of all counts claimed in one decode may not exceed the input length.
   Honest payloads always satisfy it: every element of a type that occupies bytes takes at least one, and a container's
   header is itself bytes. The error is the existing count error.
4. **F3 — R6.2 is restated.** "No more than 4× the input length in allocation" cannot hold: serde reserves room for up to
   1 MiB per open sequence from the size hint, and a one-byte `None` becomes a 16-byte `Option` in memory. The measured
   truth, and the requirement: **no allocation is made from a claimed length; the decoder allocates strings and byte buffers
   at their verified length; a sequence or map reserves at most serde's cautious 1 MiB per open nesting level (the
   reservation is address space, not touched memory), so at most 128 MiB however large the claim; and never more than
   `bincode` allocated on the same input.** The differential run checks the last clause (zero violations).
5. **F4 — what a "level" is (R2.2).** One `Option` that holds a value, newtype struct, sequence, tuple (struct,
   tuple struct), map, or enum being decoded inside another. An enum variant's fields add no level beyond the enum. So
   `struct Node { next: Option<Box<Node>> }` costs two levels per node (64 nodes decode, 65 do not). Safe by
   construction; stated here so it is not a surprise.
6. **Accepted additions:** the encoder returns `LengthMismatch` when a sequence or map writes a different number of
   elements than it declared (`bincode` wrote an inconsistent, unreadable payload; this turns that into a write error).
