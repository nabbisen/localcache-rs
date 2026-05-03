//! `localcache` — CLI inspection and maintenance tool for localcache databases.
//!
//! # Usage
//!
//! ```text
//! localcache [OPTIONS] <COMMAND>
//!
//! Options:
//!   -d, --database <PATH>    SQLite database file [default: localcache.sqlite3]
//!   -n, --namespace <NS>     Cache namespace     [default: default]
//!
//! Commands:
//!   list            List all entries with metadata
//!   stats           Show aggregate cache statistics
//!   check <PATH>    Check freshness status of a file
//!   cleanup         Delete entries for files no longer on disk
//!   vacuum          Run SQLite VACUUM to reclaim disk space
//!   purge-version   Delete all entries whose payload_version != <VERSION>
//! ```

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use localcache::{
    CacheEngine, CacheOptions, CacheStatus, ChangeDetectionMode, LocalFileCacheError, ScanOptions,
};

// ---------------------------------------------------------------------------
// CLI structure
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(
    name    = "localcache",
    version = env!("CARGO_PKG_VERSION"),
    about   = "Inspect and maintain localcache SQLite databases",
    long_about = None,
)]
struct Cli {
    /// Path to the SQLite database file.
    #[arg(short, long, global = true, default_value = "localcache.sqlite3")]
    database: PathBuf,

    /// Namespace to operate on.
    #[arg(short, long, global = true, default_value = "default")]
    namespace: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List all cached entries with their metadata.
    List(ListArgs),

    /// Show aggregate cache statistics.
    Stats,

    /// Check the freshness status of a specific file.
    Check(CheckArgs),

    /// Delete cache entries whose source files no longer exist on disk.
    Cleanup,

    /// Run SQLite VACUUM to reclaim disk space after deletions.
    Vacuum,

    /// Delete all entries whose payload_version differs from VERSION.
    ///
    /// After bumping `CacheOptions::payload_version` in your application,
    /// run this command to free disk space occupied by old-format entries.
    #[command(name = "purge-version")]
    PurgeVersion(PurgeVersionArgs),

    /// Scan a directory and show the cache status of each file.
    Scan(ScanArgs),
}

#[derive(Args)]
struct ListArgs {
    /// Limit the number of rows printed (0 = unlimited).
    #[arg(short, long, default_value_t = 0)]
    limit: usize,
}

#[derive(Args)]
struct CheckArgs {
    /// Path of the file to check.
    path: PathBuf,
}

#[derive(Args)]
struct PurgeVersionArgs {
    /// The payload version to **keep** (all other versions are removed).
    version: u32,
}

#[derive(Args)]
struct ScanArgs {
    /// Directory to scan.
    directory: PathBuf,

    /// Descend into subdirectories.
    #[arg(short, long)]
    recursive: bool,

    /// Only include files whose extension matches one of these
    /// (comma-separated, without dot, e.g. "txt,md").
    #[arg(short, long, value_delimiter = ',')]
    extensions: Vec<String>,

    /// Glob pattern matched against file names (e.g. "*.txt", "report_*").
    #[arg(short, long)]
    glob: Option<String>,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), LocalFileCacheError> {
    let opts = CacheOptions {
        database_path: cli.database,
        namespace: cli.namespace,
        // Read-only for safe inspection (except write commands which open r/w).
        ..CacheOptions::default()
    };

    match cli.command {
        Commands::List(args) => cmd_list(opts, args),
        Commands::Stats => cmd_stats(opts),
        Commands::Check(args) => cmd_check(opts, args),
        Commands::Cleanup => cmd_cleanup(opts),
        Commands::Vacuum => cmd_vacuum(opts),
        Commands::PurgeVersion(args) => cmd_purge_version(opts, args),
        Commands::Scan(args) => cmd_scan(opts, args),
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

fn cmd_list(opts: CacheOptions, args: ListArgs) -> Result<(), LocalFileCacheError> {
    let engine = CacheEngine::<Vec<u8>>::open(opts)?;

    let mut entries = engine.list_entries()?;
    if args.limit > 0 {
        entries.truncate(args.limit);
    }

    if entries.is_empty() {
        println!("(no entries)");
        return Ok(());
    }

    // Header
    println!(
        "{:<55}  {:<7}  {:<8}  {:<18}  {:<18}",
        "PATH", "VERSION", "ENCODING", "UPDATED_AT", "LAST_ACCESS"
    );
    println!("{}", "-".repeat(120));

    for e in &entries {
        let path_str = e.path.display().to_string();
        let updated = fmt_ts(e.updated_at);
        let accessed = if e.last_accessed_at == 0 {
            "never".to_owned()
        } else {
            fmt_ts(e.last_accessed_at)
        };
        println!(
            "{:<55}  {:<7}  {:<8}  {:<18}  {:<18}",
            truncate(&path_str, 55),
            e.payload_version,
            e.encoding,
            updated,
            accessed,
        );
    }
    println!("\n{} entries", entries.len());
    Ok(())
}

fn cmd_stats(opts: CacheOptions) -> Result<(), LocalFileCacheError> {
    let engine = CacheEngine::<Vec<u8>>::open(opts)?;
    let stats = engine.cache_stats()?;

    println!("Namespace:            {}", stats.namespace);
    println!("Total entries:        {}", stats.total_entries);
    println!(
        "Total payload bytes:  {} ({})",
        stats.total_payload_bytes,
        fmt_bytes(stats.total_payload_bytes)
    );
    println!(
        "Oldest entry:         {}",
        stats
            .oldest_updated_at
            .map(fmt_ts)
            .unwrap_or_else(|| "—".into())
    );
    println!(
        "Newest entry:         {}",
        stats
            .newest_updated_at
            .map(fmt_ts)
            .unwrap_or_else(|| "—".into())
    );

    if !stats.entries_by_encoding.is_empty() {
        println!("\nEncoding breakdown:");
        for (enc, count) in &stats.entries_by_encoding {
            println!("  {:<20} {}", enc, count);
        }
    }

    if !stats.entries_by_payload_version.is_empty() {
        println!("\nPayload version breakdown:");
        for (ver, count) in &stats.entries_by_payload_version {
            println!("  v{:<19} {}", ver, count);
        }
    }

    Ok(())
}

fn cmd_check(opts: CacheOptions, args: CheckArgs) -> Result<(), LocalFileCacheError> {
    let engine = CacheEngine::<Vec<u8>>::open(CacheOptions {
        change_detection_mode: ChangeDetectionMode::MetadataThenFullHash,
        ..opts
    })?;
    let status = engine.check_status(&args.path)?;
    let label = match status {
        CacheStatus::Fresh => "FRESH",
        CacheStatus::Stale => "STALE",
        CacheStatus::Missing => "MISSING",
    };
    println!("{:<10}  {}", label, args.path.display());
    Ok(())
}

fn cmd_cleanup(opts: CacheOptions) -> Result<(), LocalFileCacheError> {
    let engine = CacheEngine::<Vec<u8>>::open(opts)?;
    let removed = engine.cleanup_missing_files()?;
    println!(
        "Removed {} entr{}",
        removed,
        if removed == 1 { "y" } else { "ies" }
    );
    Ok(())
}

fn cmd_vacuum(opts: CacheOptions) -> Result<(), LocalFileCacheError> {
    let engine = CacheEngine::<Vec<u8>>::open(opts)?;
    print!("Running VACUUM … ");
    engine.shrink_database()?;
    println!("done.");
    Ok(())
}

fn cmd_purge_version(
    opts: CacheOptions,
    args: PurgeVersionArgs,
) -> Result<(), LocalFileCacheError> {
    let engine = CacheEngine::<Vec<u8>>::open(CacheOptions {
        payload_version: args.version,
        ..opts
    })?;
    let removed = engine.purge_stale_versions()?;
    println!(
        "Removed {} entr{} (payload_version ≠ {})",
        removed,
        if removed == 1 { "y" } else { "ies" },
        args.version
    );
    Ok(())
}

fn cmd_scan(opts: CacheOptions, args: ScanArgs) -> Result<(), LocalFileCacheError> {
    let engine = CacheEngine::<Vec<u8>>::open(opts)?;

    let scan_opts = ScanOptions {
        recursive: args.recursive,
        extensions: args.extensions,
        glob_pattern: args.glob,
        ..ScanOptions::default()
    };

    let results = engine.scan_dir_filtered(&args.directory, scan_opts)?;

    if results.is_empty() {
        println!("(no files found)");
        return Ok(());
    }

    println!("{:<8}  {}", "STATUS", "PATH");
    println!("{}", "-".repeat(80));

    let mut counts = (0usize, 0usize, 0usize);
    for (path, status) in &results {
        let (label, c) = match status {
            CacheStatus::Fresh => {
                counts.0 += 1;
                ("FRESH", "\x1b[32m")
            }
            CacheStatus::Stale => {
                counts.1 += 1;
                ("STALE", "\x1b[33m")
            }
            CacheStatus::Missing => {
                counts.2 += 1;
                ("MISSING", "\x1b[31m")
            }
        };
        // Only colour if stdout is a terminal.
        let use_color = std::env::var("NO_COLOR").is_err() && atty_check();
        if use_color {
            println!("{c}{:<8}\x1b[0m  {}", label, path.display());
        } else {
            println!("{:<8}  {}", label, path.display());
        }
    }

    println!(
        "\n{} files  ({} fresh, {} stale, {} missing)",
        results.len(),
        counts.0,
        counts.1,
        counts.2
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Format a Unix timestamp as `YYYY-MM-DD HH:MM:SS`.
fn fmt_ts(ts: i64) -> String {
    let secs = ts as u64;
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let days = secs / 86400;
    let (y, mo, d) = days_to_ymd(days);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02}")
}

/// Convert days-since-epoch to (year, month, day) in UTC.
fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Algorithm from https://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn fmt_bytes(n: u64) -> String {
    if n >= 1_073_741_824 {
        format!("{:.1} GiB", n as f64 / 1_073_741_824.0)
    } else if n >= 1_048_576 {
        format!("{:.1} MiB", n as f64 / 1_048_576.0)
    } else if n >= 1024 {
        format!("{:.1} KiB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        format!("…{}", &s[s.len().saturating_sub(max - 1)..])
    }
}

/// Very lightweight "is stdout a TTY" check that avoids extra dependencies.
fn atty_check() -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        libc_isatty(std::io::stdout().as_raw_fd())
    }
    #[cfg(not(unix))]
    {
        false
    }
}

#[cfg(unix)]
unsafe extern "C" {
    fn isatty(fd: i32) -> i32;
}

#[cfg(unix)]
fn libc_isatty(fd: i32) -> bool {
    // SAFETY: `isatty` is a POSIX function and always safe to call with a
    // valid file descriptor.
    unsafe { isatty(fd) != 0 }
}
