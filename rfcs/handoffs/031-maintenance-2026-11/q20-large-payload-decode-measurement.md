# Q20 Handoff — Measure the large-payload read path properly

Origin: request 066 § 6 and request 068 (the 1 MiB `get` takes about 490 µs with `codegen-units = 1` and about 900 µs at the default 16; the unkeyed 1 MiB `set` shows discrete levels from about 1150 to 1640 µs in every build). The owner's instruction (2026-11-07): this is a real, possibly small, defect; **measure it, do not explain it away.** Format of the review request: `rfcs/handoffs/025-error-taxonomy-and-input-validation/implementation-handoff.md` § 6. Authorship: high-capability model.

## 0. What this is

Measurement and diagnosis only: no tracked file changes (harnesses and logs under `.git-exclude/tmp/`), prototypes in scratch worktrees of a committed tree (`origin/main`). You report; the architect decides what, if anything, is changed, and under which RFC (the payload wire format is a permanent commitment: the golden vectors and the differential run in RFC 028 are the guard for any decoder change).

## 1. What to establish, each with command, output and exit code

1. **Reproduce, as users compile.** `get` and `set` through the engine, no key, no compression, the types users store: `Vec<u8>` and `Vec<f32>` (embeddings: 768, 1,536 and 4,096 floats, and 1 MiB / 4 MiB of each), plus a `String` and a small struct, on the **release profile as a downstream crate gets it** (a consumer crate with `localcache = "=0.25.0"` from crates.io, default `cargo build --release`: `codegen-units = 16`, no LTO), then with `lto = "thin"`, `lto = "fat"`, `codegen-units = 1`, `opt-level = 3` and `opt-level = 2`. Nine alternating rounds at least, pinned core, `/proc/loadavg` logged, medians with spread. The earlier figures were from a workspace build; say how the consumer-crate numbers differ.
2. **Where the time goes.** For the 1 MiB `Vec<u8>` `get` and the 4 MiB `Vec<f32>` `get`: split it into (a) SQLite reading the blob (a raw `rusqlite` read of the same row), (b) the copy into the engine, (c) `PayloadFormat::decode` alone on the bytes in memory (the standalone format crate, no SQLite), (d) the use-order write of the hit (`record_use`), (e) everything else. `perf record` with call graphs or `cargo flamegraph` on the release profile; report the top frames and the share of each part. If `perf` is not permitted, say so and use counters you can (`valgrind --tool=callgrind` on a smaller input).
3. **The per-element decode.** `Vec<u8>` and `Vec<f32>` are decoded through serde's sequence visitor, one `next_element` per item with a length check and a budget decrement each (`crates/localcache-format/src/payload_codec/decoder.rs`). Measure the cost per element for `u8`, `f32`, `f64`, `u64`; show how it scales with length (1 KiB to 16 MiB), and whether it is memory-bound or instruction-bound (instructions per element, IPC from `perf stat`).
4. **The same on the write side.** `PayloadFormat::encode` for the same types and sizes, and the `set` path (encode, SQLite insert, the `max_entries` bookkeeping, the use value).
5. **What the codegen-units effect is.** Why `codegen-units = 1` halves the decode: which function is no longer inlined at 16 (the sequence visitor, `take`, the budget check), shown by the symbols of the release binary (`nm`/`cargo asm`) or by the profile. Does a `#[inline]` on the named functions give the `codegen-units = 1` figure at 16? (A scratch prototype; do not commit.)
6. **The discrete levels of the 1 MiB write** (about 1153, 1170, 1264, 1455, 1640 µs): `perf stat` counters (cache misses, 4K aliasing `ld_blocks_partial.address_alias` if the CPU has it, page faults, context switches) for two builds that differ by a level, and a run with the allocator replaced by `jemalloc` or `mimalloc` in the harness only, to separate the allocator and the page-fault effect from the code. Say what separates the levels.
7. **A bulk path, as a design sketch only.** A decoder that, for a sequence whose element type is a fixed-width primitive, reads the whole run with one bounds check is not possible through serde's generic `Deserialize for Vec<T>` without specialization; list what is possible within the wire format and the safety bounds of RFC 028 (the count is bounded by the remaining bytes before anything is allocated): e.g. a fast path inside `next_element` for the decoder's own `SeqAccess` (batch bounds checks), or reading ahead. Estimate the gain from your measurements (item 3), not by guess. **Do not implement it.**

## 2. The report

One review request: each of 1 to 7 answered with evidence; the per-type, per-profile tables; the top frames; **your estimate of the user-visible cost** (for `Vec<f32>` embeddings of 768 to 4,096 floats, which is the crate's primary use, as a share of a `get`); and a verdict on whether a change is worth its risk. You report; you do not decide.

## 3. Not yours

Any tracked-file change; the implementation; the version.
