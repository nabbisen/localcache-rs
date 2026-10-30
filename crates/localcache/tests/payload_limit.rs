//! RFC 025 Amendment 2 — bounded decompression.
//!
//! A few kilobytes of compressed data can expand to gigabytes, so a stored
//! compressed payload is decompressed into at most
//! `CacheOptions::max_decompressed_bytes`, on read and on write. Only
//! decompression is limited: an uncompressed payload is as large as the blob
//! that is stored.
//!
//! The hostile frames are built here byte by byte (zstd run-length blocks), so
//! the tests need no compressor and no large allocation of their own.
#![cfg(feature = "compression")]

mod common;

use std::path::PathBuf;

use common::{store_raw_payload, write_file};
use localcache::{
    CacheEngine, CacheOptions, DEFAULT_MAX_DECOMPRESSED_BYTES, LocalFileCacheError, ReadPool,
    SyncCacheEngine,
};
use tempfile::TempDir;

const KIB: usize = 1024;
const MIB: usize = 1024 * KIB;
const GIB: u64 = 1024 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Hostile frames
// ---------------------------------------------------------------------------

/// A valid zstd frame that decompresses to `size` zero bytes, built from
/// run-length blocks of at most `block` bytes: 4 bytes of compressed data per
/// block, so one gibibyte of output is about 32 KiB of input.
///
/// With `declared == None` the header carries no content size (what
/// `zstd::encode_all`, and so this crate, writes). With `Some(n)` the header
/// declares `n` as the content size, which `n != size` forges.
fn rle_frame(size: u64, block: usize, declared: Option<u64>) -> Vec<u8> {
    rle_frame_with_window(size, block, declared, 0x38)
}

/// As [`rle_frame`], with an explicit window descriptor byte for an
/// undeclared-size frame: `(exponent << 3) | mantissa`, a window of
/// `2^(10 + exponent)` bytes (`0x38` is 128 KiB, `0x88` is 128 MiB).
fn rle_frame_with_window(size: u64, block: usize, declared: Option<u64>, window: u8) -> Vec<u8> {
    assert!(block > 0 && block <= 128 * KIB);
    let mut frame = vec![0x28, 0xB5, 0x2F, 0xFD]; // magic number
    match declared {
        None => {
            frame.push(0x00); // no content size, not single-segment
            // Window descriptor (128 KiB by default, the largest block).
            frame.push(window);
        }
        Some(declared) => {
            frame.push(0xE0); // single-segment, 8-byte content size
            frame.extend_from_slice(&declared.to_le_bytes());
        }
    }
    let mut remaining = size;
    while remaining > 0 {
        let this = remaining.min(block as u64) as u32;
        remaining -= u64::from(this);
        let last = u32::from(remaining == 0);
        // Block header: last (1 bit) | type RLE = 1 (2 bits) | size (21 bits).
        let header = last | (1 << 1) | (this << 3);
        frame.extend_from_slice(&header.to_le_bytes()[..3]);
        frame.push(0x00); // the repeated byte
    }
    frame
}

/// A frame with no declared content size, decompressing to `size` zeros.
fn bomb(size: u64) -> Vec<u8> {
    rle_frame(size, 128 * KIB, None)
}

/// A frame whose header declares `size`, truthfully.
fn declared_bomb(size: u64) -> Vec<u8> {
    rle_frame(size, 128 * KIB, Some(size))
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

struct Fixture {
    directory: TempDir,
    database: PathBuf,
    source: PathBuf,
    stored: PathBuf,
}

impl Fixture {
    /// Plant `frame` as the stored payload (`zstd` tag) of the one entry.
    fn plant(&self, frame: &[u8]) {
        store_raw_payload(&self.database, &self.stored, frame, "zstd");
    }

    fn options(&self, limit: usize) -> CacheOptions {
        CacheOptions {
            database_path: self.database.clone(),
            compress_payloads: true,
            max_decompressed_bytes: limit,
            ..CacheOptions::default()
        }
    }

    fn reopen(&self, limit: usize) -> CacheEngine<Vec<u8>> {
        CacheEngine::open(self.options(limit)).unwrap()
    }
}

/// A file-backed database with one entry (`Vec<u8>` payload, compressed).
fn fixture(limit: usize) -> (Fixture, CacheEngine<Vec<u8>>) {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("limit.sqlite3");
    let source = write_file(&directory, "source.txt", b"x");
    let engine: CacheEngine<Vec<u8>> = CacheEngine::builder()
        .database(&database)
        .compress()
        .max_decompressed_bytes(limit)
        .build()
        .unwrap();
    engine.set(&source, &vec![1_u8, 2, 3]).unwrap();
    let stored = engine.keys(None).unwrap().remove(0);
    (
        Fixture {
            directory,
            database,
            source,
            stored,
        },
        engine,
    )
}

fn assert_too_large<T: std::fmt::Debug>(result: Result<T, LocalFileCacheError>, limit: usize) {
    match result {
        Err(LocalFileCacheError::PayloadTooLarge { limit: found }) => assert_eq!(found, limit),
        other => panic!("expected PayloadTooLarge {{ limit: {limit} }}, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// The option and the variant
// ---------------------------------------------------------------------------

#[test]
fn the_default_limit_is_64_mib_and_each_engine_has_its_own() {
    assert_eq!(DEFAULT_MAX_DECOMPRESSED_BYTES, 64 * 1024 * 1024);
    assert_eq!(
        CacheOptions::default().max_decompressed_bytes,
        DEFAULT_MAX_DECOMPRESSED_BYTES
    );
    // Independent limits on one database: 3000 bytes fit in 4096, not in 1024.
    let (fx, writer) = fixture(4096);
    writer.set(&fx.source, &vec![7_u8; 3000 - 8]).unwrap();
    let small = fx.reopen(1024);
    assert_too_large(small.get(&fx.source), 1024);
    let large = fx.reopen(4096);
    assert_eq!(large.get(&fx.source).unwrap().unwrap().payload.len(), 2992);
}

/// The builder's default is the documented 64 MiB: a payload of exactly that
/// size (highly compressible, so cheap) round-trips and one byte more is
/// refused on write.
#[test]
fn the_builder_default_is_the_documented_64_mib() {
    let directory = TempDir::new().unwrap();
    let file = write_file(&directory, "d.txt", b"x");
    let engine: CacheEngine<Vec<u8>> = CacheEngine::builder()
        .database(directory.path().join("d.sqlite3"))
        .compress()
        .build()
        .unwrap();
    let exact = bincode_vec_of_serialized_len(64 * MIB);
    engine.set(&file, &exact).unwrap();
    assert_eq!(
        engine.get(&file).unwrap().unwrap().payload.len(),
        exact.len()
    );
    assert_too_large(
        engine.set(&file, &bincode_vec_of_serialized_len(64 * MIB + 1)),
        64 * MIB,
    );
}

#[test]
fn the_message_names_the_limit_and_the_option() {
    let error = LocalFileCacheError::PayloadTooLarge { limit: 1234 };
    assert_eq!(
        error.to_string(),
        "payload exceeds the 1234-byte limit for compressed payloads (CacheOptions::max_decompressed_bytes)"
    );
}

// ---------------------------------------------------------------------------
// Reads: a stored bomb is refused by every read path
// ---------------------------------------------------------------------------

#[test]
fn a_bomb_is_refused_by_get_get_if_fresh_batch_get_and_the_pool() {
    let limit = MIB;
    let (fx, engine) = fixture(limit);
    // Both header shapes, each at the smallest size over the limit and at 1 GiB.
    let frames = [
        ("undeclared, limit + 1", bomb(limit as u64 + 1)),
        ("undeclared, 1 GiB", bomb(GIB)),
        ("declared, limit + 1", declared_bomb(limit as u64 + 1)),
        ("declared, 1 GiB", declared_bomb(GIB)),
    ];
    for (name, frame) in frames {
        assert!(frame.len() < 40 * KIB, "{name}: the bomb is small");
        fx.plant(&frame);

        assert_too_large(engine.get(&fx.source), limit);
        assert_too_large(engine.get_if_fresh(&fx.source), limit);
        let batch = engine.batch_get(std::slice::from_ref(&fx.source));
        assert_eq!(batch.len(), 1);
        assert_too_large(batch.into_iter().next().unwrap(), limit);

        // The shared wrapper and the read pool decode through the same function.
        let sync = SyncCacheEngine::<Vec<u8>>::open(fx.options(limit)).unwrap();
        assert_too_large(sync.get(&fx.source), limit);
        let pool = ReadPool::<Vec<u8>>::open(fx.options(limit), 2).unwrap();
        assert_too_large(pool.get(&fx.source), limit);
    }
}

#[test]
fn a_bomb_is_a_skipped_entry_in_a_query_and_run_returns_its_error() {
    let limit = MIB;
    let (fx, engine) = fixture(limit);
    let second = write_file(&fx.directory, "second.txt", b"y");
    engine.set(&second, &vec![9_u8]).unwrap();
    fx.plant(&bomb(GIB));

    // Index tier (no payload predicate): the other entry is returned, the
    // bomb is listed with its error, and `run()` returns that error (B1).
    let report = engine.query().run_report().unwrap();
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.skipped.len(), 1);
    assert_too_large(
        Err::<(), _>(report.skipped.into_iter().next().unwrap().error),
        limit,
    );
    assert_too_large(engine.query().run(), limit);
}

#[cfg(feature = "json")]
#[test]
fn a_bomb_is_refused_by_the_json_and_decode_everything_tiers() {
    use localcache::Codec;

    let limit = MIB;
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("json.sqlite3");
    let engine: CacheEngine<serde_json::Value> = CacheEngine::builder()
        .database(&database)
        .codec(Codec::Json)
        .compress()
        .max_decompressed_bytes(limit)
        .build()
        .unwrap();
    for name in ["a.txt", "b.txt"] {
        let file = write_file(&directory, name, b"x");
        engine
            .set(&file, &serde_json::json!({ "score": 5.0 }))
            .unwrap();
    }
    let bomb_path = engine.keys(None).unwrap().remove(0);
    store_raw_payload(&database, &bomb_path, &bomb(GIB), "json-zstd");

    // A numeric predicate would use the SQL (tier 2) pushdown, but a
    // namespace with any non-`json` tag falls through to the
    // decode-everything tier; `field_eq` is decode-everything by itself.
    for query in [
        engine.query().field_gt("score", 1.0),
        engine.query().field_eq("score", serde_json::json!(5.0)),
        engine.query().payload_contains("5"),
    ] {
        let report = query.run_report().unwrap();
        assert_eq!(report.skipped.len(), 1, "the bomb is skipped");
        assert_eq!(report.entries.len(), 1, "the other entry is returned");
        assert_too_large(
            Err::<(), _>(report.skipped.into_iter().next().unwrap().error),
            limit,
        );
    }
    assert_too_large(engine.query().field_gt("score", 1.0).run(), limit);
}

// ---------------------------------------------------------------------------
// No decode at import, export or preload of a stored entry (D5)
// ---------------------------------------------------------------------------

#[test]
fn export_and_import_do_not_decode_and_the_next_read_refuses() {
    let limit = MIB;
    let (fx, engine) = fixture(limit);
    fx.plant(&bomb(GIB));

    // Export writes the stored bytes; nothing is decompressed.
    let records = engine.export_entries().unwrap();
    assert_eq!(records.len(), 1);

    // Import stores them verbatim; the hostile record is harmless until read.
    let other_dir = TempDir::new().unwrap();
    let target: CacheEngine<Vec<u8>> = CacheEngine::builder()
        .database(other_dir.path().join("target.sqlite3"))
        .compress()
        .max_decompressed_bytes(limit)
        .build()
        .unwrap();
    assert_eq!(target.import_entries(&records).unwrap(), 1);
    assert_too_large(target.get(PathBuf::from(&records[0].path)), limit);
}

/// `preload` never reads a stored payload (freshness comes from file
/// metadata), so a stored bomb is not touched by it; what it can hit is the
/// write limit, when its factory produces an oversized payload.
#[test]
fn preload_does_not_decode_a_stored_bomb_and_reports_an_oversized_new_payload() {
    let limit = 4 * KIB;
    let (fx, engine) = fixture(limit);
    fx.plant(&bomb(GIB));

    let report = engine
        .preload(
            fx.directory.path(),
            localcache::ScanOptions::default(),
            false,
            |_| Ok(vec![0_u8; 8]),
        )
        .unwrap();
    assert_eq!(
        report.already_fresh, 1,
        "the bomb's entry is fresh, not read"
    );

    let fresh = TempDir::new().unwrap();
    write_file(&fresh, "big.txt", b"x");
    let result = engine.preload(
        fresh.path(),
        localcache::ScanOptions::default(),
        false,
        |_| Ok(vec![0_u8; limit]),
    );
    assert_too_large(result, limit);
}

// ---------------------------------------------------------------------------
// A forged declared size
// ---------------------------------------------------------------------------

/// A header that declares 4 KiB over a stream of 1 MiB of 4 KiB blocks.
///
/// Observed: libzstd itself stops at the declared size ("Destination buffer is
/// too small"), so the forgery is an `Io` error whether or not the stream would
/// have fitted the limit. The bound is the second line of defence, for a frame
/// with no declared size.
#[test]
fn a_forged_small_declared_size_is_a_malformed_frame_and_never_an_abort() {
    let forged = rle_frame(MIB as u64, 4 * KIB, Some(4 * KIB as u64));
    for limit in [64 * KIB, 2 * MIB] {
        let (fx, engine) = fixture(limit);
        fx.plant(&forged);
        match engine.get(&fx.source) {
            Err(LocalFileCacheError::Io(_)) => {}
            other => panic!("limit {limit}: expected a malformed-frame Io error, got {other:?}"),
        }
    }
}

/// The opposite forgery: a header declaring more than the limit over a tiny
/// stream is refused from the header alone, before anything is decoded. The
/// header is trusted only in this direction (to refuse early), never to
/// size a buffer beyond the limit.
#[test]
fn a_forged_large_declared_size_is_refused_from_the_header() {
    let limit = 64 * KIB;
    let (fx, engine) = fixture(limit);
    fx.plant(&rle_frame(1024, 1024, Some(GIB)));
    assert_too_large(engine.get(&fx.source), limit);
}

#[test]
fn a_malformed_frame_is_still_an_io_error() {
    let (fx, engine) = fixture(MIB);
    fx.plant(&[0x28, 0xB5, 0x2F, 0xFD, 0xFF, 0xFF, 0xFF]);
    assert!(matches!(
        engine.get(&fx.source),
        Err(LocalFileCacheError::Io(_))
    ));
    fx.plant(b"not a zstd frame at all");
    assert!(matches!(
        engine.get(&fx.source),
        Err(LocalFileCacheError::Io(_))
    ));
}

// ---------------------------------------------------------------------------
// Boundaries: exactly the limit round-trips, one more byte does not
// ---------------------------------------------------------------------------

/// `Vec<u8>` with bincode's legacy encoding serializes to `8 + len` bytes.
fn bincode_vec_of_serialized_len(serialized: usize) -> Vec<u8> {
    vec![0xAB; serialized - 8]
}

#[test]
fn a_compressed_payload_of_exactly_the_limit_round_trips_and_one_more_byte_does_not() {
    let limit = 4096;
    let (fx, engine) = fixture(limit);

    let exact = bincode_vec_of_serialized_len(limit);
    engine.set(&fx.source, &exact).unwrap();
    assert_eq!(engine.get(&fx.source).unwrap().unwrap().payload, exact);

    let over = bincode_vec_of_serialized_len(limit + 1);
    assert_too_large(engine.set(&fx.source, &over), limit);
    // The refused `set` changed nothing.
    assert_eq!(engine.get(&fx.source).unwrap().unwrap().payload, exact);

    // `batch_set` reports it per item, as other encode failures.
    let second = write_file(&fx.directory, "second.txt", b"y");
    let report = engine
        .batch_set(&[(fx.source.clone(), exact.clone()), (second.clone(), over)])
        .unwrap();
    assert_eq!(report.succeeded, 1);
    assert_eq!(report.failed.len(), 1);
    assert_eq!(report.failed[0].0, second);
    assert_too_large(
        Err::<(), _>(report.failed.into_iter().next().unwrap().1),
        limit,
    );

    // Stored by other means, a frame of `limit` bytes is within the bound
    // (its zeros decode as an empty vector) and one of `limit + 1` is refused.
    fx.plant(&bomb(limit as u64));
    assert_eq!(
        engine.get(&fx.source).unwrap().unwrap().payload,
        Vec::<u8>::new()
    );
    fx.plant(&bomb(limit as u64 + 1));
    assert_too_large(engine.get(&fx.source), limit);
}

#[cfg(feature = "json")]
#[test]
fn the_same_boundary_holds_for_json_zstd() {
    use localcache::Codec;

    let limit = 4096;
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("json-boundary.sqlite3");
    let file = write_file(&directory, "j.txt", b"x");
    let engine: CacheEngine<String> = CacheEngine::builder()
        .database(&database)
        .codec(Codec::Json)
        .compress()
        .max_decompressed_bytes(limit)
        .build()
        .unwrap();
    // A JSON string serializes to its length plus two quotes.
    let exact = "a".repeat(limit - 2);
    engine.set(&file, &exact).unwrap();
    assert_eq!(engine.get(&file).unwrap().unwrap().payload, exact);
    assert_too_large(engine.set(&file, &"a".repeat(limit - 1)), limit);

    let stored = engine.keys(None).unwrap().remove(0);
    store_raw_payload(&database, &stored, &bomb(limit as u64 + 1), "json-zstd");
    assert_too_large(engine.get(&file), limit);
}

#[cfg(feature = "encryption")]
#[test]
fn the_same_boundary_holds_after_decryption() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("encrypted.sqlite3");
    let file = write_file(&directory, "e.txt", b"x");
    let key = vec![0x42_u8; 32];
    let open = |limit: usize| -> CacheEngine<Vec<u8>> {
        CacheEngine::builder()
            .database(&database)
            .compress()
            .encryption_key(key.clone())
            .max_decompressed_bytes(limit)
            .build()
            .unwrap()
    };

    let limit = 4096;
    let engine = open(limit);
    let exact = bincode_vec_of_serialized_len(limit);
    engine.set(&file, &exact).unwrap();
    assert_eq!(engine.get(&file).unwrap().unwrap().payload, exact);
    assert_too_large(
        engine.set(&file, &bincode_vec_of_serialized_len(limit + 1)),
        limit,
    );

    // An encrypted entry written under a larger limit is refused by an
    // engine with a smaller one, after decryption and before the full
    // decompression.
    open(8 * KIB)
        .set(&file, &bincode_vec_of_serialized_len(limit + 1))
        .unwrap();
    assert_too_large(engine.get(&file), limit);
}

/// The guard that the option limits only what it should: without
/// compression, a payload larger than the limit still sets and gets, and so
/// does a stored uncompressed blob.
#[test]
fn an_uncompressed_payload_larger_than_the_limit_still_sets_and_gets() {
    let limit = KIB;
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("plain.sqlite3");
    let file = write_file(&directory, "p.txt", b"x");
    let engine: CacheEngine<Vec<u8>> = CacheEngine::builder()
        .database(&database)
        .max_decompressed_bytes(limit)
        .build()
        .unwrap();
    let big = vec![0x5A_u8; 100 * KIB];
    engine.set(&file, &big).unwrap();
    assert_eq!(engine.get(&file).unwrap().unwrap().payload, big);
    assert_eq!(
        engine.query().run().unwrap().len(),
        1,
        "and so does a query"
    );

    // A compressing engine with the same limit still reads that entry: the
    // limit is about compressed payloads, not engines.
    let compressing = CacheEngine::<Vec<u8>>::open(CacheOptions {
        database_path: database,
        compress_payloads: true,
        max_decompressed_bytes: limit,
        ..CacheOptions::default()
    })
    .unwrap();
    assert_eq!(compressing.get(&file).unwrap().unwrap().payload, big);
}

// ---------------------------------------------------------------------------
// Memory: the refusal is bounded, measured in a child process
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
const CHILD: &str = "LOCALCACHE_Q10_CHILD";

/// What the child does: refuse a 1 GiB bomb under a small limit, and print the
/// resident and address-space peaks before and after. `window` is the frame's
/// window descriptor byte.
#[cfg(target_os = "linux")]
fn refuse_a_bomb_and_report(limit: usize, window: u8) {
    let (fx, engine) = fixture(limit);
    let frame = rle_frame_with_window(GIB, 128 * KIB, None, window);
    fx.plant(&frame);
    let rss_before = common::proc_status_kib("VmHWM:").unwrap_or(0);
    let vm_before = common::proc_status_kib("VmPeak:").unwrap_or(0);
    for _ in 0..3 {
        assert_too_large(engine.get(&fx.source), limit);
    }
    let rss_after = common::proc_status_kib("VmHWM:").unwrap_or(0);
    let vm_after = common::proc_status_kib("VmPeak:").unwrap_or(0);
    println!(
        "Q10 compressed_bytes={} limit_kib={} rss_before_kib={rss_before} rss_after_kib={rss_after} vm_before_kib={vm_before} vm_after_kib={vm_after}",
        frame.len(),
        limit / KIB,
    );
}

#[cfg(target_os = "linux")]
fn field(output: &str, name: &str) -> u64 {
    output
        .split_whitespace()
        .find_map(|word| word.strip_prefix(&format!("{name}=")))
        .unwrap_or_else(|| panic!("no {name} in: {output}"))
        .parse()
        .unwrap()
}

/// The address-space ceiling for the child: what the test binary needs to run
/// (measured by the first run, below) plus `3 x limit` plus 64 MiB of slack.
#[cfg(target_os = "linux")]
#[test]
fn a_gigabyte_bomb_is_refused_within_a_few_times_the_limit() {
    const LIMIT: usize = 8 * MIB;
    const NAME: &str = "a_gigabyte_bomb_is_refused_within_a_few_times_the_limit";
    if std::env::var_os(CHILD).is_some() {
        refuse_a_bomb_and_report(LIMIT, 0x38);
        return;
    }

    // First, an unlimited child to learn the binary's own address-space need.
    let probe = common::run_self_with_address_limit(NAME, 16 * 1024 * 1024, &[(CHILD, "1")]);
    let probe_out = String::from_utf8_lossy(&probe.stdout).into_owned();
    assert!(
        probe.status.success(),
        "unlimited child failed: {probe_out}"
    );
    let baseline_vm_kib = field(&probe_out, "vm_before_kib");

    let ceiling_kib = baseline_vm_kib + 3 * (LIMIT / KIB) as u64 + 64 * 1024;
    let output = common::run_self_with_address_limit(NAME, ceiling_kib, &[(CHILD, "1")]);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        output.status.success(),
        "the refusal did not survive an address-space ceiling of {ceiling_kib} KiB \
         (baseline {baseline_vm_kib} KiB): status {:?}\n{stdout}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    let growth = field(&stdout, "rss_after_kib").saturating_sub(field(&stdout, "rss_before_kib"));
    println!("{stdout}");
    println!("ceiling={ceiling_kib} KiB baseline_vm={baseline_vm_kib} KiB rss_growth={growth} KiB");
    // The claim: near the limit, not a multiple of the 1 GiB expansion.
    assert!(
        growth <= 2 * (LIMIT / KIB) as u64,
        "resident growth {growth} KiB exceeds twice the {} KiB limit",
        LIMIT / KIB
    );
}

/// A frame header can also ask the decoder for a large *window*, whatever the
/// output: this one claims 128 MiB (zstd's default maximum). The decoder
/// reserves that much address space (not resident memory) before the first
/// byte is produced, independent of `max_decompressed_bytes`. Measured, not
/// asserted away: the refusal still comes back as `PayloadTooLarge`, resident
/// growth is about twice the limit (the decoder's window buffer holds what
/// has been written to it, and our output buffer holds the same again), and
/// the peak address space grows by most of the window.
#[cfg(target_os = "linux")]
#[test]
fn a_frame_claiming_the_largest_window_is_still_refused_and_stays_resident_small() {
    const LIMIT: usize = 8 * MIB;
    const NAME: &str =
        "a_frame_claiming_the_largest_window_is_still_refused_and_stays_resident_small";
    if std::env::var_os(CHILD).is_some() {
        refuse_a_bomb_and_report(LIMIT, 0x88);
        return;
    }
    // No ceiling: report what an unlimited process does.
    let output = common::run_self_with_address_limit(NAME, 16 * 1024 * 1024, &[(CHILD, "1")]);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(output.status.success(), "{stdout}");
    println!("{stdout}");
    let growth = field(&stdout, "rss_after_kib").saturating_sub(field(&stdout, "rss_before_kib"));
    let vm_growth = field(&stdout, "vm_after_kib").saturating_sub(field(&stdout, "vm_before_kib"));
    println!("window=128MiB rss_growth={growth} KiB vm_peak_growth={vm_growth} KiB");
    // The decoder's window buffer is written as output flows, so the refusal
    // holds about the limit in the window and the limit in `out`: bounded by
    // the limit, not by the expansion.
    assert!(
        growth <= 3 * (LIMIT / KIB) as u64,
        "resident growth {growth} KiB"
    );
}
