//! Read-only subcommand handlers.
//!
//! Extracted from `main.rs` (Phase 22 N5) to keep the primary implementation
//! file under the project's line-count guidance. This is a mechanical
//! split: no function here changes signature, visibility, or behavior. The
//! grouping matches `DatabaseAuthority::ReadOnly` in `main.rs`, an
//! already-established domain boundary, not an invented one.

use localcache::{
    CacheEngine, CacheOptions, CacheStatus, ChangeDetectionMode, LocalFileCacheError, ScanOptions,
};

use crate::text::{format_unix_nanoseconds, format_unix_seconds, truncate};
use crate::{CheckArgs, ExportArgs, InspectArgs, ListArgs, QueryArgs, ScanArgs};
use crate::{atty_check, fmt_bytes};

pub(crate) fn cmd_list(opts: CacheOptions, args: ListArgs) -> Result<(), LocalFileCacheError> {
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
        let updated = format_unix_seconds(e.updated_at);
        let accessed = if e.last_accessed_at == 0 {
            "never".to_owned()
        } else {
            format_unix_seconds(e.last_accessed_at)
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
pub(crate) fn cmd_stats(opts: CacheOptions) -> Result<(), LocalFileCacheError> {
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
            .map(format_unix_seconds)
            .unwrap_or_else(|| "—".into())
    );
    println!(
        "Newest entry:         {}",
        stats
            .newest_updated_at
            .map(format_unix_seconds)
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
pub(crate) fn cmd_check(opts: CacheOptions, args: CheckArgs) -> Result<(), LocalFileCacheError> {
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
pub(crate) fn cmd_scan(opts: CacheOptions, args: ScanArgs) -> Result<(), LocalFileCacheError> {
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

    println!("{:<8}  PATH", "STATUS");
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
pub(crate) fn cmd_export(opts: CacheOptions, args: ExportArgs) -> Result<(), LocalFileCacheError> {
    let engine = CacheEngine::<Vec<u8>>::open(opts)?;
    let records = engine.export_entries()?;

    let use_stdout = args.output == "-";
    let mut output: Box<dyn std::io::Write> = if use_stdout {
        Box::new(std::io::stdout())
    } else {
        Box::new(
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&args.output)
                .map_err(LocalFileCacheError::Io)?,
        )
    };

    for record in &records {
        let line = serde_json::to_string(record).map_err(|e| {
            LocalFileCacheError::UnsupportedFeature(format!("json serialisation: {e}"))
        })?;
        output
            .write_all(line.as_bytes())
            .map_err(LocalFileCacheError::Io)?;
        output.write_all(b"\n").map_err(LocalFileCacheError::Io)?;
    }

    if !use_stdout {
        eprintln!(
            "Exported {} entr{} → {}",
            records.len(),
            if records.len() == 1 { "y" } else { "ies" },
            args.output
        );
    }
    Ok(())
}
pub(crate) fn cmd_query(opts: CacheOptions, args: QueryArgs) -> Result<(), LocalFileCacheError> {
    let engine = CacheEngine::<Vec<u8>>::open(opts)?;

    let keys = engine.keys(args.path_like.as_deref())?;
    if keys.is_empty() {
        println!("(no matching entries)");
        return Ok(());
    }

    println!("{:<8}  PATH", "STATUS");
    println!("{}", "-".repeat(80));

    let statuses = engine.check_status_batch(&keys);
    let mut counts = (0usize, 0usize, 0usize);
    for (path, result) in keys.iter().zip(statuses.iter()) {
        let status = result.as_ref().unwrap_or(&CacheStatus::Missing);
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
        let use_color = std::env::var("NO_COLOR").is_err() && atty_check();
        if use_color {
            println!("{c}{:<8}\x1b[0m  {}", label, path.display());
        } else {
            println!("{:<8}  {}", label, path.display());
        }
    }

    println!(
        "\n{} entries  ({} fresh, {} stale, {} missing)",
        keys.len(),
        counts.0,
        counts.1,
        counts.2
    );
    Ok(())
}
pub(crate) fn cmd_inspect(
    opts: CacheOptions,
    args: InspectArgs,
) -> Result<(), LocalFileCacheError> {
    let engine = CacheEngine::<Vec<u8>>::open(CacheOptions {
        change_detection_mode: localcache::ChangeDetectionMode::MetadataThenFullHash,
        ..opts
    })?;

    let diag = engine.explain(&args.path)?;

    println!("=== Cache Diagnosis ===");
    println!("Path:          {}", diag.path.display());
    println!("Status:        {:?}", diag.status);
    println!("Entry exists:  {}", diag.entry_exists);
    println!("File exists:   {}", diag.file_exists);

    if let Some(ttl_rem) = diag.ttl_remaining_secs {
        if ttl_rem == 0 {
            println!("TTL:           EXPIRED");
        } else {
            println!("TTL remaining: {}s", ttl_rem);
        }
    } else {
        println!("TTL:           not configured");
    }

    if let Some(pv) = &diag.payload_version {
        println!(
            "Payload ver:   stored={} expected={} match={}",
            pv.stored, pv.expected, pv.matches
        );
    }

    if let Some(diff) = &diag.metadata_diff {
        println!("--- Metadata ---");
        println!(
            "  mtime:     stored={} current={} changed={}",
            format_unix_nanoseconds(diff.stored_mtime),
            format_unix_nanoseconds(diff.current_mtime),
            diff.mtime_changed
        );
        println!(
            "  file_size: stored={} current={} changed={}",
            fmt_bytes(diff.stored_file_size),
            fmt_bytes(diff.current_file_size),
            diff.size_changed
        );
    }

    if let Some(hash_match) = diag.hash_match {
        println!("Hash match:    {}", hash_match);
    }

    println!("\nSummary: {}", diag.summary);
    Ok(())
}
pub(crate) fn cmd_namespaces(opts: CacheOptions) -> Result<(), LocalFileCacheError> {
    let engine = CacheEngine::<Vec<u8>>::open(opts.clone())?;
    let namespaces = engine.namespace_list()?;

    if namespaces.is_empty() {
        println!("(no namespaces)");
        return Ok(());
    }

    println!("{:<30}  ENTRIES", "NAMESPACE");
    println!("{}", "-".repeat(50));
    for ns in &namespaces {
        let count = CacheEngine::<Vec<u8>>::open(CacheOptions {
            namespace: ns.clone(),
            ..opts.clone()
        })
        .ok()
        .and_then(|e| e.entry_count().ok())
        .unwrap_or(0);
        println!("{:<30}  {}", ns, count);
    }
    println!(
        "\n{} namespace{}",
        namespaces.len(),
        if namespaces.len() == 1 { "" } else { "s" }
    );
    Ok(())
}
