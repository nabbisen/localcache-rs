//! Large-namespace scale profile (Phase 22 N4; extended in Phase 23 P1a).
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
//! **P1a addition:** set `TMPDIR` to a directory on real storage (not tmpfs)
//! before drawing any conclusion about I/O-bound operations — see limitation 1
//! below, which N4 recorded and did not follow. Set `LOCALCACHE_SCALE_COLD=1` to
//! additionally measure `get_if_fresh` and `cleanup_missing_files` with the page
//! cache dropped between population and measurement (see "Reading the results
//! honestly", limitation 3). Set `LOCALCACHE_SCALE_QUERY_REPEATS=3` (or more) at
//! large scale to replicate the whole-namespace query block and report a median
//! and spread instead of a single, potentially unreliable run (limitation 5).
//! Example:
//!
//! ```text
//! LOCALCACHE_SCALE=1000000 LOCALCACHE_SCALE_COLD=1 LOCALCACHE_SCALE_QUERY_REPEATS=3 \
//!   TMPDIR=/home/you/localcache-scale-tmp \
//!   cargo bench -p localcache --features json --bench scale_profile
//! ```
//!
//! `LOCALCACHE_SCALE` defaults to 10_000 so an accidental invocation stays cheap.
//! Population cost is reported separately from every measured operation, because at
//! large N the setup dominates and conflating the two would misattribute it.
//!
//! # Reading the results honestly
//!
//! Limitations that change how the numbers should be interpreted:
//!
//! 1. **`TMPDIR` is often a RAM-backed tmpfs.** Where it is, every source-file
//!    `stat` runs at memory speed, so `cleanup_missing_files` and any
//!    metadata-mode freshness check are **understated** relative to real storage.
//!    Set `TMPDIR` to a directory on the target filesystem before drawing
//!    conclusions about I/O-bound operations. The filesystem type and mount point
//!    of `TMPDIR` are printed at the top of every run's output for exactly this
//!    reason — a profile that cannot say what it ran on is the defect P1a exists
//!    to fix.
//! 2. **A single namespace holds every entry.** Operations whose plan narrows by
//!    `namespace=?` therefore scan nearly the whole table. That is the pessimistic
//!    case, and it is the right default for finding hotspots — but a deployment
//!    spreading entries across many namespaces will see different numbers, so do
//!    not read these as universal.
//! 3. **Moving off tmpfs is not sufficient on its own.** The harness creates
//!    every source file and then `stat`s it almost immediately; on a host with
//!    enough RAM, the kernel's inode/dentry/page cache can serve those `stat`
//!    calls without touching the disk at all, silently reproducing a tmpfs-like
//!    result while appearing to run on real storage. `LOCALCACHE_SCALE_COLD=1`
//!    re-measures `get_if_fresh` and `cleanup_missing_files` after the operator
//!    drops the page cache (`sync && echo 3 | sudo tee /proc/sys/vm/drop_caches`)
//!    between population and measurement. Dropping the cache needs root and is
//!    never attempted by this harness; the run pauses and waits for a marker
//!    file instead. A per-row `cleanup_missing_files` cost that lands near a
//!    previously-recorded tmpfs figure, on a run that claims to be on real
//!    storage, is a signal to check whether the cache was actually dropped —
//!    not a result to trust at face value.
//! 4. **btrfs-on-LUKS (or any specific real filesystem) is not neutral.**
//!    Copy-on-write, checksumming, and encryption all add cost that will differ
//!    from ext4 or a bare unencrypted SSD. A real-storage number from one
//!    filesystem is *a* real-storage number, not *the* one — the filesystem type
//!    printed at the top of the run is part of the result, not incidental.
//! 5. **Run-to-run variance on real storage is large enough that a single 1M run
//!    cannot be trusted for a single-digit-multiple effect.** N4's original
//!    judgement — "adequate for spotting a 115x effect; inadequate for detecting
//!    a 10% regression" — was made on tmpfs, where that variance is negligible.
//!    It does not hold on real storage: two identical 1M runs on this harness
//!    have disagreed by up to 3.56x on `path_in_dir`, and one run's `field_gt`
//!    figure came in 2.02x its replicated median purely from run-to-run noise.
//!    `LOCALCACHE_SCALE_QUERY_REPEATS` (default 1) re-runs the whole-namespace
//!    query block — the unstable part — without re-populating, and reports the
//!    median plus min–max spread; treat any single-run whole-namespace query
//!    figure at large scale as unverified until it has been replicated.
//!
//!    **An uncontrolled input that looks like part of the environment is a
//!    second, distinct cause of irreproducible figures, and replication does
//!    not catch it.** `TMPDIR`'s absolute path length is stored twice per
//!    entry (the `files` table and the covering index), so a longer path grows
//!    both and slows any scan that touches them. Moving `TMPDIR` from a
//!    45-character path to a 105-character one (a nested `.git-exclude/tmp/`
//!    under this repository) changed the on-disk database size 18.6% and
//!    `path_in_dir` 1.71x with **zero code changes and three tight,
//!    low-spread replications on each side** — replication only detects
//!    variance *within* a fixed configuration, not a variable that moved
//!    *between* runs. `TMPDIR`'s character length and one example stored
//!    path's length are printed at the top of every run for exactly this
//!    reason; when comparing two runs, confirm they match (or that database
//!    size per row matches) before reading any timing.
//!
//!    **`LOCALCACHE_SCALE_QUERY_REPEATS` covers the query block only.** The
//!    destructive whole-namespace operations — `cleanup_missing_files` and LRU
//!    eviction — mutate the state they measure, so they cannot be repeated
//!    without a full re-population; their figures are always single-sample and
//!    carry session-level variance on top of everything above. Observed: five
//!    otherwise-comparable btrfs 1M `cleanup_missing_files` runs, taken across
//!    two sessions, spread 1.24x (5.715 s to 7.076 s) for reasons that were not
//!    fully explained by path length. A **paired** comparison taken in one
//!    session (both sides measured minutes apart, same code, same database
//!    shape) is sound; a single absolute figure quoted across sessions is not.
//!
//! Comparative measurements must return the **same number of rows**. An early
//! revision compared a 1000-row query against a 1-row query and appeared to show a
//! leading-wildcard glob was faster than a leading-literal one; it was measuring
//! result-set size and payload decoding. Row counts are asserted equal where a
//! ratio is reported.
//!
//! 6. **Host memory pressure and load from unrelated processes are an unrecorded
//!    input, same as `TMPDIR` path length was before limitation 5 named it.**
//!    (P2d, R5.) A tier-1 `limit(25)` query — normally a covering-index scan
//!    served from page cache, ~230 ms at 1M — measured **2.1 s, a 9x anomaly**,
//!    on a host with 34 GB of swap in active use from several unrelated
//!    processes at the time. The same run's tier-2 query was unaffected (it is
//!    already I/O- and CPU-bound decoding payload content, so cache pressure's
//!    *proportional* effect is much smaller) — that asymmetry, not a gut feeling,
//!    is what distinguished contention from a real regression. Available RAM,
//!    swap in use, and load average are now printed alongside the filesystem
//!    substrate line at the top of every run for exactly this reason: a profile
//!    that cannot say what else the host was doing is the defect.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use localcache::{CacheEngine, CacheOptions, ChangeDetectionMode, Codec, ReadPool, ScanOptions};
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

/// P1a: how long to wait for the operator to drop the page cache and create the
/// marker file before giving up and falling back to a warm-only report.
const COLD_DROP_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// P1a: threads used for the scoped-down concurrent-access measurement (§4.4).
const CONCURRENT_THREADS: usize = 8;

/// P1a: `get` calls per thread in the concurrent-access measurement.
const CONCURRENT_OPS_PER_THREAD: usize = 200;

/// R2 (P1a revision): default number of times the whole-namespace query block
/// runs. 1 keeps a bare invocation cheap and matches the pre-revision shape;
/// large-scale evidence-gathering runs should raise this via
/// `LOCALCACHE_SCALE_QUERY_REPEATS`.
const DEFAULT_QUERY_REPEATS: usize = 1;

/// R2: a spread (max/min) above this multiple is flagged as unstable rather
/// than presented as a precise median.
const UNSTABLE_SPREAD_THRESHOLD: f64 = 1.5;

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
    shard_path_under(root.path(), index)
}

/// P1a: same sharding scheme as [`shard_path`], generalised to any root so the
/// additions (preload, bincode) can create their own independent fileset without
/// touching the primary one `shard_path`/`TempDir` populate.
fn shard_path_under(root: &Path, index: usize) -> PathBuf {
    root.join(format!("shard-{:05}", index / SHARD_SIZE))
        .join(format!("document-{index:07}.txt"))
}

fn create_shard_dirs(root: &Path, scale: usize) {
    for shard in 0..=(scale / SHARD_SIZE) {
        std::fs::create_dir_all(root.join(format!("shard-{shard:05}")))
            .expect("create shard directory");
    }
}

fn recover_index(path: &Path) -> usize {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(|stem| stem.rsplit('-').next())
        .and_then(|digits| digits.parse::<usize>().ok())
        .expect("recover index from path")
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

/// R1 (P1a revision): run `work` `samples` times, timing the first call
/// separately from the rest and excluding it from the per-call mean. On real
/// storage a first access after population can cost orders of magnitude more
/// than steady state (a page/inode the OS has not yet re-cached for this
/// process), and averaging it into a 200-sample mean silently inflates the
/// reported per-call figure -- invisibly on tmpfs, where the first call is
/// cheap enough not to matter. `check` runs on every call (not just the
/// first) so a silent early return can never be reported as a hit.
fn timed_point_op<T>(
    label: &str,
    samples: usize,
    mut work: impl FnMut() -> T,
    mut check: impl FnMut(&T),
) {
    println!("  {label}");
    let start = Instant::now();
    let first = work();
    let first_elapsed = start.elapsed();
    check(&first);
    println!("    ↳ first call (excluded from mean)     {first_elapsed:>12.3?}");
    let remaining = samples.saturating_sub(1);
    let start = Instant::now();
    for _ in 0..remaining {
        let value = work();
        check(&value);
    }
    let rest_elapsed = start.elapsed();
    report("    ↳ per call, samples 2..N", rest_elapsed, remaining);
}

/// R2: the middle value of `values` (sorted). Not interpolated -- fine for the
/// small repeat counts (3-5) this harness uses.
fn median_duration(mut values: Vec<Duration>) -> Duration {
    values.sort();
    values[values.len() / 2]
}

/// R2: print a replicated measurement's median and min-max spread, flagging
/// anything wide enough that the median should not be read as precise.
fn report_spread(label: &str, values: &[Duration]) {
    let median = median_duration(values.to_vec());
    let min = *values.iter().min().expect("at least one sample");
    let max = *values.iter().max().expect("at least one sample");
    let spread = max.as_secs_f64() / min.as_secs_f64().max(f64::MIN_POSITIVE);
    let flag = if values.len() > 1 && spread > UNSTABLE_SPREAD_THRESHOLD {
        "  ⚠ spread > 1.5x — treat as unstable, not precise"
    } else {
        ""
    };
    println!(
        "  {label:<32} median {median:>10.3?}  min {min:>10.3?}  max {max:>10.3?}  ({spread:.2}x spread){flag}"
    );
}

/// P1a §2 requirement: record what the harness actually ran on, in the harness's
/// own output, not only in the review request that quotes it.
fn filesystem_info(path: &Path) -> String {
    let fstype = std::process::Command::new("stat")
        .args(["-f", "-c", "%T", &path.to_string_lossy()])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let mount = std::process::Command::new("df")
        .args(["--output=source,target", &path.to_string_lossy()])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .nth(1)
                .unwrap_or("unknown")
                .trim()
                .to_string()
        })
        .unwrap_or_else(|| "unknown".to_string());
    format!("{fstype} ({mount})")
}

/// P2d R5: record host memory and load state at run start, next to
/// `filesystem_info` -- same principle, a different unrecorded input. Reads
/// `/proc/meminfo` and `/proc/loadavg` directly rather than shelling out to
/// `free`/`uptime`, whose human-readable output is locale- and
/// version-dependent; the `/proc` fields used here are stable across
/// kernels. Returns "unavailable" rather than failing the run on non-Linux
/// hosts or restricted environments where `/proc` is absent.
fn memory_and_load_info() -> String {
    fn kb_field(text: &str, key: &str) -> Option<u64> {
        text.lines()
            .find(|line| line.starts_with(key))
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|value| value.parse::<u64>().ok())
    }

    let mem = std::fs::read_to_string("/proc/meminfo").ok().map(|text| {
        let gib = |kb: u64| kb as f64 / 1024.0 / 1024.0;
        let total = kb_field(&text, "MemTotal:").unwrap_or(0);
        let available = kb_field(&text, "MemAvailable:").unwrap_or(0);
        let swap_total = kb_field(&text, "SwapTotal:").unwrap_or(0);
        let swap_free = kb_field(&text, "SwapFree:").unwrap_or(0);
        let swap_used = swap_total.saturating_sub(swap_free);
        format!(
            "available {:.1} GiB / {:.1} GiB total, swap {:.1} GiB used / {:.1} GiB total",
            gib(available),
            gib(total),
            gib(swap_used),
            gib(swap_total),
        )
    });

    let load = std::fs::read_to_string("/proc/loadavg")
        .ok()
        .and_then(|text| {
            let mut fields = text.split_whitespace();
            Some(format!(
                "{} {} {} (1/5/15 min)",
                fields.next()?,
                fields.next()?,
                fields.next()?
            ))
        });

    format!(
        "memory: {}; load average: {}",
        mem.unwrap_or_else(|| "unavailable".to_string()),
        load.unwrap_or_else(|| "unavailable".to_string()),
    )
}

/// P1a §2.5: pause and poll for `marker` rather than attempting to drop the page
/// cache ourselves. Dropping the cache needs root; this harness never attempts to
/// acquire privilege. Returns `true` if the marker appeared before the timeout.
fn wait_for_cache_drop(marker: &Path) -> bool {
    println!();
    println!("  >>> COLD MEASUREMENT PAUSE <<<");
    println!("  To measure real-storage cost rather than page-cache cost, drop the");
    println!("  page cache now, in another shell, as the machine's owner:");
    println!();
    println!("      sync && echo 3 | sudo tee /proc/sys/vm/drop_caches");
    println!();
    println!("  Then create this marker file to resume:");
    println!();
    println!("      touch {}", marker.display());
    println!();
    println!(
        "  Waiting up to {:?} before falling back to a warm-only report...",
        COLD_DROP_TIMEOUT
    );

    let deadline = Instant::now() + COLD_DROP_TIMEOUT;
    while Instant::now() < deadline {
        if marker.exists() {
            let _ = std::fs::remove_file(marker);
            println!("  Marker found — resuming with cold measurements.\n");
            return true;
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    println!(
        "  Timed out waiting for {} — no cold measurement taken this run.\n",
        marker.display()
    );
    false
}

fn main() {
    let scale: usize = std::env::var("LOCALCACHE_SCALE")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(10_000);
    let cold_requested = std::env::var("LOCALCACHE_SCALE_COLD")
        .map(|value| value == "1")
        .unwrap_or(false);
    let query_repeats: usize = std::env::var("LOCALCACHE_SCALE_QUERY_REPEATS")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value >= 1)
        .unwrap_or(DEFAULT_QUERY_REPEATS);

    println!("\n=== localcache scale profile: {scale} entries ===\n");

    let tempdir = tempfile::tempdir().expect("create scale tempdir");
    let tmpdir_path_str = tempdir.path().display().to_string();
    println!(
        "storage substrate: TMPDIR = {} -> {}",
        tmpdir_path_str,
        filesystem_info(tempdir.path())
    );
    // P2d R5: host memory pressure and load from unrelated processes are an
    // unrecorded input too -- see limitation 6. Captured at run start,
    // before population begins consuming memory itself.
    println!("  host substrate: {}", memory_and_load_info());
    // R8 (P1a revision 2): path length is a measurement input, not incidental --
    // see limitation 5. A longer TMPDIR lengthens every stored path, which is
    // stored twice per entry (the `files` table and the covering index), which
    // grows the table and index and slows scans that touch them -- silently,
    // and by exactly the mechanism that made the revision-1 figures wrong.
    println!("  TMPDIR path length: {} characters", tmpdir_path_str.len());
    create_shard_dirs(tempdir.path(), scale);

    println!("\nsetup (excluded from every measurement below):");
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
    let example_path = paths[0].display().to_string();
    println!(
        "  example stored path length: {} characters ({})",
        example_path.len(),
        example_path
    );

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
                .map(|path| (path, ScalePayload::new(recover_index(path))))
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

    println!(
        "\npoint operations ({POINT_SAMPLES} samples; first call excluded from the mean — R1):"
    );
    let probe = &paths[scale / 2];
    timed_point_op(
        "get (warm hit)",
        POINT_SAMPLES,
        || engine.get(probe).expect("scale get"),
        |result| assert!(result.is_some(), "probe get must return Some"),
    );

    timed_point_op(
        "get_if_fresh (metadata hit, WARM cache)",
        POINT_SAMPLES,
        || engine.get_if_fresh(probe).expect("scale get_if_fresh"),
        |result| assert!(result.is_some(), "probe get_if_fresh must return Some"),
    );

    // Hypothesis: a leading literal lets SQLite use idx_files_namespace_path,
    // while a leading wildcard cannot, forcing a full scan.
    //
    // The two patterns below are chosen to select the **same 1000 rows**, so the
    // only difference is whether the prefix is indexable. An earlier version of
    // this harness compared a 1000-row literal against a 1-row wildcard and
    // "showed" the wildcard was 5x faster — it was measuring result-set size and
    // payload decoding, not index usage. Keep the row counts equal or the
    // comparison is meaningless.
    let one_shard = tempdir.path().join("shard-00000");
    let literal = format!("{}/shard-00000/*", tempdir.path().display());

    // R2 (P1a revision): the whole-namespace query block is the unstable part
    // of this harness on real storage (see limitation 5) — repeated
    // `query_repeats` times without re-populating, rather than trusted from one
    // run. `query_repeats` is 1 by default, matching the pre-revision shape and
    // keeping a bare invocation cheap.
    println!("\nwhole-namespace queries ({query_repeats} repeat(s); see R2 for large scale):");
    let mut tier1_times = Vec::with_capacity(query_repeats);
    let mut field_gt_times = Vec::with_capacity(query_repeats);
    let mut path_in_dir_times = Vec::with_capacity(query_repeats);
    let mut literal_times = Vec::with_capacity(query_repeats);
    let mut wildcard_times = Vec::with_capacity(query_repeats);
    let mut tier1_row_count = None;
    let mut field_gt_row_count = None;
    let mut path_in_dir_row_count = None;
    let mut literal_row_count = None;
    let mut wildcard_row_count = None;

    for repeat in 1..=query_repeats {
        if query_repeats > 1 {
            println!("  -- repeat {repeat}/{query_repeats} --");
        }

        // RFC 021 P2c: tier 1 — no field predicate, no field sort, so no
        // payload is decoded until the winners of `limit` are known. The
        // largest effect RFC 021 produced; nothing above this line measured
        // it. Labeled explicitly as tier 1 so a reader does not need to know
        // the tier taxonomy to see two different things are being measured.
        let (rows, elapsed) = timed("  query: [tier 1] limit 25, no predicate", || {
            engine.query().limit(25).run().expect("scale tier-1 query")
        });
        if let Some(expected) = tier1_row_count {
            assert_eq!(
                rows.len(),
                expected,
                "tier-1 query row count changed across repeats"
            );
        }
        tier1_row_count = Some(rows.len());
        tier1_times.push(elapsed);

        let (rows, elapsed) = timed("  query: [tier 2] field_gt + order_by + limit 25", || {
            engine
                .query()
                .field_gt("score", (scale / 2) as f64)
                .order_by_field("score", false)
                .limit(25)
                .run()
                .expect("scale JSON query")
        });
        if let Some(expected) = field_gt_row_count {
            assert_eq!(
                rows.len(),
                expected,
                "field_gt row count changed across repeats"
            );
        }
        field_gt_row_count = Some(rows.len());
        field_gt_times.push(elapsed);

        let (rows, elapsed) = timed("  query: path_in_dir (non-recursive)", || {
            engine
                .query()
                .path_in_dir(&one_shard, false)
                .run()
                .expect("scale path_in_dir")
        });
        if let Some(expected) = path_in_dir_row_count {
            assert_eq!(
                rows.len(),
                expected,
                "path_in_dir row count changed across repeats"
            );
        }
        path_in_dir_row_count = Some(rows.len());
        path_in_dir_times.push(elapsed);

        let (literal_rows, literal_time) = timed("  query: path_glob, leading literal", || {
            engine
                .query()
                .path_glob(&literal)
                .run()
                .expect("scale path_glob literal")
        });
        let (wildcard_rows, wildcard_time) = timed("  query: path_glob, LEADING WILDCARD", || {
            engine
                .query()
                .path_glob("*/shard-00000/*")
                .run()
                .expect("scale path_glob wildcard")
        });
        if literal_rows.len() != wildcard_rows.len() {
            println!(
                "      ⚠ MISMATCHED row counts ({} literal vs {} wildcard) — ratio meaningless \
                 this repeat",
                literal_rows.len(),
                wildcard_rows.len()
            );
        }
        if let Some(expected) = literal_row_count {
            assert_eq!(
                literal_rows.len(),
                expected,
                "literal-glob row count changed across repeats"
            );
        }
        literal_row_count = Some(literal_rows.len());
        literal_times.push(literal_time);
        if let Some(expected) = wildcard_row_count {
            assert_eq!(
                wildcard_rows.len(),
                expected,
                "wildcard-glob row count changed across repeats"
            );
        }
        wildcard_row_count = Some(wildcard_rows.len());
        wildcard_times.push(wildcard_time);
    }

    println!(
        "      → tier1 {} rows; field_gt {} rows; path_in_dir {} rows; glob {} rows literal / \
         {} rows wildcard{}",
        tier1_row_count.unwrap_or(0),
        field_gt_row_count.unwrap_or(0),
        path_in_dir_row_count.unwrap_or(0),
        literal_row_count.unwrap_or(0),
        wildcard_row_count.unwrap_or(0),
        if literal_row_count == wildcard_row_count {
            " (glob comparison matched — valid)"
        } else {
            " (glob comparison MISMATCHED)"
        }
    );

    if query_repeats > 1 {
        println!("\nreplication summary (median, min–max — R2):");
        report_spread("[tier 1] limit 25, no predicate", &tier1_times);
        report_spread("[tier 2] field_gt + order_by + limit 25", &field_gt_times);
        report_spread("path_in_dir (non-recursive)", &path_in_dir_times);
        report_spread("path_glob, leading literal", &literal_times);
        report_spread("path_glob, LEADING WILDCARD", &wildcard_times);
    }
    let literal_median = median_duration(literal_times.clone());
    let wildcard_median = median_duration(wildcard_times.clone());
    println!(
        "      → wildcard/literal ratio (median): {:.2}x",
        wildcard_median.as_secs_f64() / literal_median.as_secs_f64().max(f64::MIN_POSITIVE)
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
    let (removed, cleanup) = timed("cleanup_missing_files (10% absent, WARM cache)", || {
        engine
            .cleanup_missing_files()
            .expect("scale cleanup_missing_files")
    });
    println!("      → {removed} entries removed");
    report("  ↳ per surviving row scanned", cleanup, scale);

    // ------------------------------------------------------------------
    // P1a §2.5 — cold-cache re-measurement of the two stat-bound operations,
    // plus §4.2's cold-open cost. A second, disjoint 10% of the namespace is
    // reserved for the cold cleanup pass so it has real absent files to find,
    // independent of the warm pass above.
    // ------------------------------------------------------------------
    if cold_requested {
        let missing_cold = missing..(2 * missing).min(scale);
        for index in missing_cold.clone() {
            std::fs::remove_file(shard_path(&tempdir, index)).expect("delete source file (cold)");
        }

        let marker = tempdir.path().join("cold-drop-ready");
        if wait_for_cache_drop(&marker) {
            println!("cold measurements (page cache dropped by operator):");

            let (cold_engine, cold_open) =
                timed("CacheEngine::open (cold-open cost, §4.2)", || {
                    CacheEngine::<ScalePayload>::open(CacheOptions {
                        database_path: database_path.clone(),
                        change_detection_mode: ChangeDetectionMode::MetadataOnly,
                        codec: Codec::Json,
                        ..CacheOptions::default()
                    })
                    .expect("cold-open scale cache")
                });
            report("  ↳ (single call)", cold_open, 1);

            timed_point_op(
                "get_if_fresh (metadata hit, COLD cache)",
                POINT_SAMPLES,
                || {
                    cold_engine
                        .get_if_fresh(probe)
                        .expect("scale get_if_fresh (cold)")
                },
                |result| {
                    assert!(
                        result.is_some(),
                        "probe get_if_fresh (cold) must return Some"
                    )
                },
            );

            let before_cold_cleanup = cold_engine
                .entry_count()
                .expect("entry_count before cold cleanup");
            let (removed_cold, cold_cleanup) =
                timed("cleanup_missing_files (10% absent, COLD cache)", || {
                    cold_engine
                        .cleanup_missing_files()
                        .expect("scale cleanup_missing_files (cold)")
                });
            println!("      → {removed_cold} entries removed");
            report(
                "  ↳ per surviving row scanned",
                cold_cleanup,
                before_cold_cleanup,
            );

            let per_row_ns = cold_cleanup.as_nanos() as f64 / before_cold_cleanup.max(1) as f64;
            if per_row_ns < 200.0 {
                println!(
                    "      ⚠ per-row cost ({per_row_ns:.1} ns/row) is suspiciously fast for \
                     real storage — this may still be measuring cache, not disk. Investigate \
                     before trusting this figure."
                );
            }
            println!();
        } else {
            println!(
                "cold measurements: NOT TAKEN — cache-drop marker did not appear in time.\n\
                 Reporting warm figures above only; they remain a floor, not a real-storage \
                 result for the stat-bound operations.\n"
            );
        }
    } else {
        println!(
            "cold measurements: not requested (set LOCALCACHE_SCALE_COLD=1). Warm figures above \
             are a floor, not a real-storage result, for the stat-bound operations.\n"
        );
    }

    // `remaining` is read live rather than computed as `scale - missing` because
    // the optional cold-cleanup pass above may have removed additional entries;
    // this keeps the eviction threshold below correct in both configurations
    // without changing what eviction measures.
    let remaining = engine
        .entry_count()
        .expect("entry_count before eviction setup");
    let evict_to = remaining / 2;
    let capped = CacheEngine::<ScalePayload>::open(CacheOptions {
        database_path: database_path.clone(),
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

    // ------------------------------------------------------------------
    // P1a §4 — the four unmeasured operations from N4 §6, in its stated order
    // of value (concurrent access, §4.4, is the loosest and is scoped down —
    // see its own section below).
    // ------------------------------------------------------------------

    // §4.1 — preload, on its own fileset so it exercises the same real-storage
    // stat/read cost without disturbing the primary namespace's row counts used
    // above.
    println!("\nadditions (P1a §4):");
    let preload_dir = tempdir.path().join("preload-source");
    create_shard_dirs(&preload_dir, scale);
    let (preload_paths, _) = timed("  [preload] create source files", || {
        (0..scale)
            .map(|index| {
                let path = shard_path_under(&preload_dir, index);
                std::fs::write(&path, format!("preload document {index}\n"))
                    .expect("write preload source file");
                path
            })
            .collect::<Vec<_>>()
    });
    let preload_db = tempdir.path().join("preload.sqlite3");
    let preload_engine = CacheEngine::<ScalePayload>::open(CacheOptions {
        database_path: preload_db,
        change_detection_mode: ChangeDetectionMode::MetadataOnly,
        codec: Codec::Json,
        ..CacheOptions::default()
    })
    .expect("open preload cache");
    let (preload_report, preload_elapsed) = timed("preload (whole namespace, §4.1)", || {
        preload_engine
            .preload(
                &preload_dir,
                ScanOptions {
                    recursive: true,
                    ..ScanOptions::default()
                },
                false,
                |path| {
                    let index = recover_index(path);
                    Ok(ScalePayload::new(index))
                },
            )
            .expect("scale preload")
    });
    report("  ↳ per row", preload_elapsed, scale);
    println!(
        "      → stored {}, already_fresh {}, skipped {}",
        preload_report.stored, preload_report.already_fresh, preload_report.skipped
    );

    // P1d §4 — cleanup_expired, never measured before RFC 020 changed it
    // identically to cleanup_missing_files. Its own database and fileset, a
    // short ttl, and a real sleep past it (reported as setup, not measured
    // time) -- the primary engine deliberately has no ttl, since giving it
    // one would change `get_if_fresh` and every other measurement taken
    // against it above.
    let expired_dir = tempdir.path().join("expired-source");
    create_shard_dirs(&expired_dir, scale);
    let (expired_paths, _) = timed("  [cleanup_expired] create source files", || {
        (0..scale)
            .map(|index| {
                let path = shard_path_under(&expired_dir, index);
                std::fs::write(&path, format!("expired document {index}\n"))
                    .expect("write expired source file");
                path
            })
            .collect::<Vec<_>>()
    });
    let expired_db = tempdir.path().join("expired.sqlite3");
    let expired_ttl = Duration::from_secs(2);
    let expired_engine = CacheEngine::<ScalePayload>::open(CacheOptions {
        database_path: expired_db,
        change_detection_mode: ChangeDetectionMode::MetadataOnly,
        codec: Codec::Json,
        ttl: Some(expired_ttl),
        ..CacheOptions::default()
    })
    .expect("open cleanup_expired cache");
    timed("  [cleanup_expired] populate (chunked batch_set)", || {
        for chunk in expired_paths.chunks(POPULATE_CHUNK) {
            let items: Vec<_> = chunk
                .iter()
                .map(|path| (path, ScalePayload::new(recover_index(path))))
                .collect();
            let outcome = expired_engine
                .batch_set(&items)
                .expect("populate cleanup_expired cache");
            assert!(
                outcome.failed.is_empty(),
                "cleanup_expired population must not fail"
            );
        }
    });
    timed(
        "  [cleanup_expired] sleep past ttl (setup, not measured)",
        || std::thread::sleep(expired_ttl + Duration::from_secs(1)),
    );
    let (removed_expired, cleanup_expired_elapsed) =
        timed("cleanup_expired (whole namespace, RFC 020 R3)", || {
            expired_engine
                .cleanup_expired()
                .expect("scale cleanup_expired")
        });
    println!("      → {removed_expired} entries removed");
    assert_eq!(
        removed_expired, scale,
        "every entry shares the same short ttl and was populated before the sleep, so all \
         must be expired"
    );
    report("  ↳ per row", cleanup_expired_elapsed, scale);

    // §4.3 — bincode codec at scale. `field_gt` cannot run against bincode
    // payloads (no JSON extraction), so this covers get/set/cleanup/eviction
    // only, per the handoff. Reuses the preload fileset (untouched by preload,
    // which only reads) rather than creating a third full-scale set of files.
    let bincode_db = tempdir.path().join("bincode.sqlite3");
    let bincode_engine = CacheEngine::<ScalePayload>::open(CacheOptions {
        database_path: bincode_db.clone(),
        change_detection_mode: ChangeDetectionMode::MetadataOnly,
        codec: Codec::Bincode,
        ..CacheOptions::default()
    })
    .expect("open bincode cache");
    let (_, bincode_populate) = timed("bincode: populate (chunked batch_set, §4.3)", || {
        for chunk in preload_paths.chunks(POPULATE_CHUNK) {
            let items: Vec<_> = chunk
                .iter()
                .map(|path| (path, ScalePayload::new(recover_index(path))))
                .collect();
            let outcome = bincode_engine
                .batch_set(&items)
                .expect("populate bincode scale cache");
            assert!(
                outcome.failed.is_empty(),
                "bincode population must not fail"
            );
        }
    });
    report("  ↳ per row", bincode_populate, scale);

    let bincode_probe = &preload_paths[scale / 2];
    timed_point_op(
        "bincode: get (warm hit)",
        POINT_SAMPLES,
        || bincode_engine.get(bincode_probe).expect("bincode get"),
        |result| assert!(result.is_some(), "bincode probe get must return Some"),
    );

    let bincode_missing = scale / 10;
    for index in 0..bincode_missing {
        std::fs::remove_file(shard_path_under(&preload_dir, index))
            .expect("delete bincode source file");
    }
    let (bincode_removed, bincode_cleanup) =
        timed("bincode: cleanup_missing_files (10% absent)", || {
            bincode_engine
                .cleanup_missing_files()
                .expect("bincode cleanup_missing_files")
        });
    println!("      → {bincode_removed} entries removed");
    report("  ↳ per surviving row scanned", bincode_cleanup, scale);

    let bincode_remaining = bincode_engine
        .entry_count()
        .expect("bincode entry_count before eviction setup");
    let bincode_evict_to = bincode_remaining / 2;
    let bincode_capped = CacheEngine::<ScalePayload>::open(CacheOptions {
        database_path: bincode_db,
        change_detection_mode: ChangeDetectionMode::MetadataOnly,
        codec: Codec::Bincode,
        max_entries: Some(bincode_evict_to),
        ..CacheOptions::default()
    })
    .expect("bincode reopen with max_entries");
    let bincode_trigger = &preload_paths[scale - 1];
    let (_, bincode_evict) = timed("bincode: LRU eviction (one set, ~50% over cap)", || {
        bincode_capped
            .set(bincode_trigger, &ScalePayload::new(scale))
            .expect("bincode eviction set")
    });
    report(
        "  ↳ per evicted row",
        bincode_evict,
        bincode_remaining.saturating_sub(bincode_evict_to),
    );

    // §4.4 — concurrent access. Deliberately scoped down to `ReadPool` under
    // `std::thread`, against the primary (already-populated) database. Async
    // backends (tokio/async-std/smol) are explicitly out of scope for this
    // pass: wiring an async runtime into a `harness = false` bench binary for
    // three backends is disproportionate to the value here, and `ReadPool`
    // already isolates the contended resource (connection checkout) that a
    // concurrent measurement is meant to characterise. Reported as an explicit
    // scoping decision, not silently omitted.
    let read_pool = ReadPool::<ScalePayload>::open(
        CacheOptions {
            database_path: database_path.clone(),
            change_detection_mode: ChangeDetectionMode::MetadataOnly,
            codec: Codec::Json,
            ..CacheOptions::default()
        },
        CONCURRENT_THREADS,
    )
    .expect("open scale read pool");
    let concurrent_probe = probe.clone();
    let (_, concurrent_elapsed) = timed(
        "concurrent: ReadPool get, 8 threads x 200 calls (§4.4, scoped)",
        || {
            std::thread::scope(|scope| {
                for _ in 0..CONCURRENT_THREADS {
                    let pool = &read_pool;
                    let target = &concurrent_probe;
                    scope.spawn(move || {
                        for _ in 0..CONCURRENT_OPS_PER_THREAD {
                            pool.get(target).expect("concurrent scale get");
                        }
                    });
                }
            });
        },
    );
    let total_ops = CONCURRENT_THREADS * CONCURRENT_OPS_PER_THREAD;
    report(
        "  ↳ per call (aggregate/total_ops)",
        concurrent_elapsed,
        total_ops,
    );
    println!(
        "      → {CONCURRENT_THREADS} threads × {CONCURRENT_OPS_PER_THREAD} calls = {total_ops} total; \
         async runtime backends (tokio/async-std/smol) not measured this pass — scoped out, see review request"
    );

    println!("\n=== end of profile ({scale} entries) ===\n");
}
