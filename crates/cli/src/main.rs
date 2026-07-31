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

mod commands;
mod text;

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use localcache::{CacheOptions, LocalFileCacheError};

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

    /// Export all entries to a JSON Lines file (one record per line).
    ///
    /// Payload bytes are Base64-encoded so the file is fully text-portable.
    /// Encrypted entries are exported verbatim (still encrypted).
    Export(ExportArgs),

    /// Import entries from a JSON Lines file produced by `export`.
    ///
    /// Existing entries for the same path are replaced.  The target namespace
    /// can be overridden with `-n`.
    Import(ImportArgs),

    /// Copy all entries from one namespace to another within the same database.
    ///
    /// Uses the fast `import_from` path (no Base64 round-trip).
    Copy(CopyArgs),

    /// Migrate a namespace: export from the source database and import into a
    /// new database, optionally changing namespace.
    ///
    /// Useful for moving data between database files or bumping schema versions.
    /// The source is opened writable and may be upgraded to the current
    /// localcache schema before its entries are copied.
    Migrate(MigrateArgs),

    /// Query cached entries by path prefix or suffix.
    ///
    /// Prints matching stored paths and their cache status.
    /// For payload content queries use the library API directly.
    Query(QueryArgs),

    /// Show detailed diagnostic information for a specific file.
    ///
    /// Reports staleness reason, metadata differences, hash comparison, TTL
    /// remaining time, and payload version status.
    Inspect(InspectArgs),

    /// Watch cached files for changes and print invalidation events.
    ///
    /// Monitors all source files that currently have a cache entry using
    /// OS-native file-system events.  Prints a line for each invalidated
    /// entry.  Press Ctrl-C to exit.
    ///
    /// Requires the `watching` Cargo feature in the library.
    Watch,

    /// List all namespaces present in the database.
    Namespaces,
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

#[derive(Args)]
struct ExportArgs {
    /// Output file path.  Use `-` to write to stdout.
    #[arg(short, long, default_value = "-")]
    output: String,
}

#[derive(Args)]
struct ImportArgs {
    /// Input file path.  Use `-` to read from stdin.
    #[arg(short, long, default_value = "-")]
    input: String,

    /// Overwrite existing entries with the same path. When false, records
    /// whose path already exists in the target namespace are left
    /// untouched and reported as skipped; the remaining records still
    /// import. Default is to overwrite. A bare `--overwrite` (no value)
    /// also means true; use `--overwrite=false` to skip existing entries.
    #[arg(
        long,
        default_value_t = true,
        default_missing_value = "true",
        num_args = 0..=1,
        require_equals = true,
        action = clap::ArgAction::Set
    )]
    overwrite: bool,
}

#[derive(Args)]
struct CopyArgs {
    /// Source namespace to copy from.
    #[arg(short, long)]
    from: String,

    /// Destination namespace to copy into.
    /// Defaults to the `-n / --namespace` global option.
    #[arg(short, long)]
    to: Option<String>,
}

#[derive(Args)]
struct MigrateArgs {
    /// Source database file.
    #[arg(long)]
    src_db: PathBuf,

    /// Source namespace.
    #[arg(long, default_value = "default")]
    src_ns: String,

    /// Destination database file (created if absent).
    /// Defaults to the `-d / --database` global option.
    #[arg(long)]
    dst_db: Option<PathBuf>,

    /// Destination namespace.
    /// Defaults to the `-n / --namespace` global option.
    #[arg(long)]
    dst_ns: Option<String>,
}

#[derive(Args)]
struct QueryArgs {
    /// SQL LIKE pattern matched against stored paths.
    /// Use `%` for any sequence, `_` for one character.
    /// Example: `%/docs/%`
    #[arg(short, long)]
    path_like: Option<String>,
}

#[derive(Args)]
struct InspectArgs {
    /// Path of the file to inspect.
    path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DatabaseAuthority {
    ReadOnly,
    Writable,
}

fn command_database_authority(command: &Commands) -> DatabaseAuthority {
    match command {
        Commands::List(_)
        | Commands::Stats
        | Commands::Check(_)
        | Commands::Scan(_)
        | Commands::Export(_)
        | Commands::Query(_)
        | Commands::Inspect(_)
        | Commands::Namespaces => DatabaseAuthority::ReadOnly,
        Commands::Cleanup
        | Commands::Vacuum
        | Commands::PurgeVersion(_)
        | Commands::Import(_)
        | Commands::Copy(_)
        | Commands::Migrate(_)
        | Commands::Watch => DatabaseAuthority::Writable,
    }
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
    let authority = command_database_authority(&cli.command);
    let opts = CacheOptions {
        database_path: cli.database,
        namespace: cli.namespace,
        read_only: authority == DatabaseAuthority::ReadOnly,
        ..CacheOptions::default()
    };

    match cli.command {
        Commands::List(args) => commands::read::cmd_list(opts, args),
        Commands::Stats => commands::read::cmd_stats(opts),
        Commands::Check(args) => commands::read::cmd_check(opts, args),
        Commands::Cleanup => commands::write::cmd_cleanup(opts),
        Commands::Vacuum => commands::write::cmd_vacuum(opts),
        Commands::PurgeVersion(args) => commands::write::cmd_purge_version(opts, args),
        Commands::Scan(args) => commands::read::cmd_scan(opts, args),
        Commands::Export(args) => commands::read::cmd_export(opts, args),
        Commands::Import(args) => commands::write::cmd_import(opts, args),
        Commands::Copy(args) => commands::write::cmd_copy(opts, args),
        Commands::Migrate(args) => commands::write::cmd_migrate(opts, args),
        Commands::Query(args) => commands::read::cmd_query(opts, args),
        Commands::Inspect(args) => commands::read::cmd_inspect(opts, args),
        Commands::Watch => commands::write::cmd_watch(opts),
        Commands::Namespaces => commands::read::cmd_namespaces(opts),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[cfg(feature = "watching")]
/// Current Unix timestamp in seconds.
fn now_secs() -> i64 {
    use std::time::UNIX_EPOCH;
    UNIX_EPOCH
        .elapsed()
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
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

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn every_command_has_explicit_database_authority() {
        let cases: &[(&[&str], DatabaseAuthority)] = &[
            (&["localcache", "list"], DatabaseAuthority::ReadOnly),
            (&["localcache", "stats"], DatabaseAuthority::ReadOnly),
            (
                &["localcache", "check", "file"],
                DatabaseAuthority::ReadOnly,
            ),
            (&["localcache", "scan", "."], DatabaseAuthority::ReadOnly),
            (&["localcache", "export"], DatabaseAuthority::ReadOnly),
            (&["localcache", "query"], DatabaseAuthority::ReadOnly),
            (
                &["localcache", "inspect", "file"],
                DatabaseAuthority::ReadOnly,
            ),
            (&["localcache", "namespaces"], DatabaseAuthority::ReadOnly),
            (&["localcache", "cleanup"], DatabaseAuthority::Writable),
            (&["localcache", "vacuum"], DatabaseAuthority::Writable),
            (
                &["localcache", "purge-version", "1"],
                DatabaseAuthority::Writable,
            ),
            (&["localcache", "import"], DatabaseAuthority::Writable),
            (
                &["localcache", "copy", "--from", "source"],
                DatabaseAuthority::Writable,
            ),
            (
                &["localcache", "migrate", "--src-db", "source.sqlite3"],
                DatabaseAuthority::Writable,
            ),
            (&["localcache", "watch"], DatabaseAuthority::Writable),
        ];

        for (arguments, expected) in cases {
            let cli = Cli::try_parse_from(*arguments).unwrap();
            assert_eq!(command_database_authority(&cli.command), *expected);
        }
    }

    #[test]
    fn migrate_help_discloses_source_schema_upgrade() {
        let mut command = Cli::command();
        let migrate = command
            .find_subcommand_mut("migrate")
            .expect("migrate subcommand");
        let help = migrate.render_long_help().to_string();
        assert!(help.contains("source is opened writable"), "{help}");
        assert!(help.contains("may be upgraded"), "{help}");
    }
}
