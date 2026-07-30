//! Large-namespace scale profile (Phase 22 N4).
//!
//! **This is a measurement harness, not a Criterion benchmark, and deliberately
//! not a release gate.** Criterion samples a fast operation many times to get a
//! statistically stable latency. That is the wrong instrument for whole-namespace
//! work such as `cleanup_missing_files` or LRU eviction, where the question is
//! "how long does this take once, on a namespace of size N" and where each run
//! mutates the state being measured.
//!
//! N4's purpose is to produce an evidence base that does not currently exist: the
//! Criterion suite's largest dataset is 250 entries, while the deferred
//! performance item targets >1M. Nothing here tunes anything. It measures, so that
//! any later tuning is aimed at a real hotspot rather than a guess.
//!
//! Run with:
//!
//! ```text
//! LOCALCACHE_SCALE=10000 cargo bench --features json --bench scale_profile
//! ```
//!
//! `LOCALCACHE_SCALE` defaults to 10_000 so an accidental invocation stays cheap.
//! Population cost is reported separately from every measured operation, because at
//! large N the setup dominates and conflating the two would misattribute it.
//!
//! # Reading the results honestly
//!
//! Two limitations that change how the numbers should be interpreted:
//!
//! 1. **`TMPDIR` is often a RAM-backed tmpfs.** Where it is, every source-file
//!    `stat` runs at memory speed, so `cleanup_missing_files` and any
//!    metadata-mode freshness check are **understated** relative to real storage.
//!    Set `TMPDIR` to a directory on the target filesystem before drawing
//!    conclusions about I/O-bound operations.
//! 2. **A single namespace holds every entry.** Operations whose plan narrows by
//!    `namespace=?` therefore scan nearly the whole table. That is the pessimistic
//!    case, and it is the right default for finding hotspots — but a deployment
//!    spreading entries across many namespaces will see different numbers, so do
//!    not read these as universal.
//!
//! Comparative measurements must return the **same number of rows**. An early
//! revision compared a 1000-row query against a 1-row query and appeared to show a
//! leading-wildcard glob was faster than a leading-literal one; it was measuring
//! result-set size and payload decoding. Row counts are asserted equal where a
//! ratio is reported.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use localcache::{CacheEngine, CacheOptions, ChangeDetectionMode, Codec};
use serde::{Deserialize, Serialize};
use tempfile::TempDir;

/// Files per shard directory. A single directory holding ~1M entries measures the
/// host filesystem's directory scaling, not this crate's, so entries are sharded to
/// keep the subject of measurement the cache rather than the filesystem.
const SHARD_SIZE: usize = 1_000;

/// Rows per `batch_set` call during population. Bounds peak memory: the full item
/// vector is never materialised for the whole namespace.
const POPULATE_CHUNK: usize = 5_000;

/// Repetitions for point operations fast enough that one sample is noise.
const POINT_SAMPLES: usize = 200;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ScalePayload {
    label: String,
    score: f64,
    vector: Vec<f32>,
}

impl ScalePayload {
    fn new(index: usize) -> Self {
        Self {
            label: format!("document-{index:07}"),
            score: index as f64,
            vector: (0..64)
                .map(|value| (value + index) as f32 * 0.001)
                .collect(),
        }
    }
}

fn shard_path(root: &TempDir, index: usize) -> PathBuf {
    root.path()
        .join(format!("shard-{:05}", index / SHARD_SIZE))
        .join(format!("document-{index:07}.txt"))
}

fn timed<T>(label: &str, work: impl FnOnce() -> T) -> (T, Duration) {
    let start = Instant::now();
    let value = work();
    let elapsed = start.elapsed();
    println!("  {label:<44} {:>12.3?}", elapsed);
    (value, elapsed)
}

fn report(label: &str, elapsed: Duration, rows: usize) {
    let per_row_ns = elapsed.as_nanos() as f64 / rows.max(1) as f64;
    println!("  {label:<44} {elapsed:>12.3?}  ({per_row_ns:>9.1} ns/row)");
}

fn main() {
    let scale: usize = std::env::var("LOCALCACHE_SCALE")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(10_000);

    println!("\n=== localcache scale profile: {scale} entries ===\n");

    let tempdir = tempfile::tempdir().expect("create scale tempdir");
    for shard in 0..=(scale / SHARD_SIZE) {
        std::fs::create_dir_all(tempdir.path().join(format!("shard-{shard:05}")))
            .expect("create shard directory");
    }

    println!("setup (excluded from every measurement below):");
    let (paths, _) = timed("create source files", || {
        (0..scale)
            .map(|index| {
                let path = shard_path(&tempdir, index);
                std::fs::write(&path, format!("scale document {index}\n"))
                    .expect("write source file");
                path
            })
            .collect::<Vec<_>>()
    });

    let database_path = tempdir.path().join("scale.sqlite3");
    let engine = CacheEngine::<ScalePayload>::open(CacheOptions {
        database_path: database_path.clone(),
        change_detection_mode: ChangeDetectionMode::MetadataOnly,
        codec: Codec::Json,
        ..CacheOptions::default()
    })
    .expect("open scale cache");

    let (_, populate) = timed("populate (chunked batch_set)", || {
        for chunk in paths.chunks(POPULATE_CHUNK) {
            let items: Vec<_> = chunk
                .iter()
                .map(|path| {
                    let index = path
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        .and_then(|stem| stem.rsplit('-').next())
                        .and_then(|digits| digits.parse::<usize>().ok())
                        .expect("recover index from path");
                    (path, ScalePayload::new(index))
                })
                .collect();
            let outcome = engine.batch_set(&items).expect("populate scale cache");
            assert!(outcome.failed.is_empty(), "population must not fail");
        }
    });
    report("  ↳ population, per row", populate, scale);

    let bytes = std::fs::metadata(&database_path)
        .map(|meta| meta.len())
        .unwrap_or_default();
    println!(
        "  {:<44} {:>12} ({:.1} bytes/row)",
        "database size on disk",
        bytes,
        bytes as f64 / scale.max(1) as f64
    );

    println!("\npoint operations ({POINT_SAMPLES} samples, mean per call):");
    let probe = &paths[scale / 2];
    let (_, get_total) = timed("get (warm hit)", || {
        for _ in 0..POINT_SAMPLES {
            engine.get(probe).expect("scale get");
        }
    });
    report("  ↳ per call", get_total, POINT_SAMPLES);

    let (_, fresh_total) = timed("get_if_fresh (metadata hit)", || {
        for _ in 0..POINT_SAMPLES {
            engine.get_if_fresh(probe).expect("scale get_if_fresh");
        }
    });
    report("  ↳ per call", fresh_total, POINT_SAMPLES);

    println!("\nwhole-namespace queries (one run each):");
    let (rows, _) = timed("query: field_gt + order_by + limit 25", || {
        engine
            .query()
            .field_gt("score", (scale / 2) as f64)
            .order_by_field("score", false)
            .limit(25)
            .run()
            .expect("scale JSON query")
    });
    println!("      → {} rows", rows.len());

    let one_shard = tempdir.path().join("shard-00000");
    let (rows, _) = timed("query: path_in_dir (non-recursive)", || {
        engine
            .query()
            .path_in_dir(&one_shard, false)
            .run()
            .expect("scale path_in_dir")
    });
    println!("      → {} rows", rows.len());

    // Hypothesis: a leading literal lets SQLite use idx_files_namespace_path,
    // while a leading wildcard cannot, forcing a full scan.
    //
    // The two patterns below are chosen to select the **same 1000 rows**, so the
    // only difference is whether the prefix is indexable. An earlier version of
    // this harness compared a 1000-row literal against a 1-row wildcard and
    // "showed" the wildcard was 5x faster — it was measuring result-set size and
    // payload decoding, not index usage. Keep the row counts equal or the
    // comparison is meaningless.
    let literal = format!("{}/shard-00000/*", tempdir.path().display());
    let (literal_rows, literal_time) = timed("query: path_glob, leading literal", || {
        engine
            .query()
            .path_glob(&literal)
            .run()
            .expect("scale path_glob literal")
    });

    let (wildcard_rows, wildcard_time) = timed("query: path_glob, LEADING WILDCARD", || {
        engine
            .query()
            .path_glob("*/shard-00000/*")
            .run()
            .expect("scale path_glob wildcard")
    });

    println!(
        "      → {} rows literal vs {} rows wildcard{}",
        literal_rows.len(),
        wildcard_rows.len(),
        if literal_rows.len() == wildcard_rows.len() {
            " (matched — comparison valid)"
        } else {
            " (MISMATCHED — ratio below is meaningless)"
        }
    );
    println!(
        "      → wildcard/literal ratio: {:.2}x",
        wildcard_time.as_secs_f64() / literal_time.as_secs_f64().max(f64::MIN_POSITIVE)
    );

    println!("\nquery plans (dry_run — what SQLite actually does):");
    for (label, plan) in [
        (
            "field_gt + order_by + limit",
            engine
                .query()
                .field_gt("score", (scale / 2) as f64)
                .order_by_field("score", false)
                .limit(25)
                .dry_run(),
        ),
        (
            "path_glob leading literal",
            engine.query().path_glob(&literal).dry_run(),
        ),
        (
            "path_glob leading wildcard",
            engine.query().path_glob("*/shard-00000/*").dry_run(),
        ),
        (
            "path_in_dir non-recursive",
            engine.query().path_in_dir(&one_shard, false).dry_run(),
        ),
    ] {
        match plan {
            Ok(text) => {
                println!("  {label}:");
                for line in text.lines() {
                    println!("      {line}");
                }
            }
            Err(error) => println!("  {label}: plan unavailable ({error})"),
        }
    }

    println!("\nmaintenance (one run each, destructive — ordered last):");
    let missing = scale / 10;
    for index in 0..missing {
        std::fs::remove_file(shard_path(&tempdir, index)).expect("delete source file");
    }
    let (removed, cleanup) = timed("cleanup_missing_files (10% absent)", || {
        engine
            .cleanup_missing_files()
            .expect("scale cleanup_missing_files")
    });
    println!("      → {removed} entries removed");
    report("  ↳ per surviving row scanned", cleanup, scale);

    let remaining = scale - missing;
    let evict_to = remaining / 2;
    let capped = CacheEngine::<ScalePayload>::open(CacheOptions {
        database_path,
        change_detection_mode: ChangeDetectionMode::MetadataOnly,
        codec: Codec::Json,
        max_entries: Some(evict_to),
        ..CacheOptions::default()
    })
    .expect("reopen with max_entries");
    let trigger = &paths[scale - 1];
    let (_, evict) = timed("LRU eviction (one set, ~50% over cap)", || {
        capped
            .set(trigger, &ScalePayload::new(scale))
            .expect("scale eviction set")
    });
    report(
        "  ↳ per evicted row",
        evict,
        remaining.saturating_sub(evict_to),
    );

    println!("\n=== end of profile ({scale} entries) ===\n");
}
