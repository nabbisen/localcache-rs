//! Mutating subcommand handlers.
//!
//! Extracted from `main.rs` (Phase 22 N5) to keep the primary implementation
//! file under the project's line-count guidance. This is a mechanical
//! split: no function here changes signature, visibility, or behavior. The
//! grouping matches `DatabaseAuthority::Writable` in `main.rs`, an
//! already-established domain boundary, not an invented one.

use std::io::BufRead;

use localcache::{CacheEngine, CacheOptions, LocalFileCacheError};

#[cfg(feature = "watching")]
use crate::now_secs;
#[cfg(feature = "watching")]
use crate::text::format_unix_seconds;
use crate::{CopyArgs, ImportArgs, MigrateArgs, PurgeVersionArgs};

pub(crate) fn cmd_cleanup(opts: CacheOptions) -> Result<(), LocalFileCacheError> {
    let engine = CacheEngine::<Vec<u8>>::open(opts)?;
    let removed = engine.cleanup_missing_files()?;
    println!(
        "Removed {} entr{}",
        removed,
        if removed == 1 { "y" } else { "ies" }
    );
    Ok(())
}
pub(crate) fn cmd_vacuum(opts: CacheOptions) -> Result<(), LocalFileCacheError> {
    let engine = CacheEngine::<Vec<u8>>::open(opts)?;
    print!("Running VACUUM … ");
    engine.shrink_database()?;
    println!("done.");
    Ok(())
}
pub(crate) fn cmd_purge_version(
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
pub(crate) fn cmd_import(opts: CacheOptions, args: ImportArgs) -> Result<(), LocalFileCacheError> {
    let engine = CacheEngine::<Vec<u8>>::open(opts)?;

    let input: Box<dyn std::io::BufRead> = if args.input == "-" {
        Box::new(std::io::BufReader::new(std::io::stdin()))
    } else {
        Box::new(std::io::BufReader::new(
            std::fs::File::open(&args.input).map_err(LocalFileCacheError::Io)?,
        ))
    };

    let mut records = Vec::new();
    for (lineno, line) in input.lines().enumerate() {
        let line = line.map_err(LocalFileCacheError::Io)?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let record: localcache::ExportRecord = serde_json::from_str(trimmed).map_err(|e| {
            LocalFileCacheError::UnsupportedFeature(format!(
                "json parse error at line {}: {e}",
                lineno + 1
            ))
        })?;
        records.push(record);
    }

    if args.overwrite {
        let imported = engine.import_entries(&records)?;
        eprintln!(
            "Imported {} entr{}",
            imported,
            if imported == 1 { "y" } else { "ies" }
        );
        return Ok(());
    }

    // `--overwrite=false`: skip records whose path already exists in the
    // target namespace, importing only the rest. Compare against the exact
    // stored key set — the same key `import_entries`'s
    // `ON CONFLICT(namespace, path)` upsert conflicts on — rather than
    // `CacheEngine::contains`, which canonicalises per the RFC 008 path
    // contract and could disagree with the raw upsert conflict key.
    let existing: std::collections::HashSet<String> = engine
        .keys(None)?
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let (to_import, skipped): (Vec<_>, Vec<_>) = records
        .into_iter()
        .partition(|r| !existing.contains(&r.path));
    let skipped = skipped.len();

    let imported = engine.import_entries(&to_import)?;
    if skipped == 0 {
        eprintln!(
            "Imported {} entr{}",
            imported,
            if imported == 1 { "y" } else { "ies" }
        );
    } else {
        eprintln!(
            "Imported {} entr{}, skipped {} existing",
            imported,
            if imported == 1 { "y" } else { "ies" },
            skipped
        );
    }
    Ok(())
}
pub(crate) fn cmd_copy(opts: CacheOptions, args: CopyArgs) -> Result<(), LocalFileCacheError> {
    let dst_ns = args.to.unwrap_or_else(|| opts.namespace.clone());

    let dst: CacheEngine<Vec<u8>> = CacheEngine::open(CacheOptions {
        namespace: dst_ns.clone(),
        read_only: false,
        ..opts.clone()
    })?;

    let src: CacheEngine<Vec<u8>> = CacheEngine::open(CacheOptions {
        namespace: args.from.clone(),
        read_only: true,
        ..opts
    })?;

    let copied = dst.import_from(&src)?;
    eprintln!(
        "Copied {} entr{} from namespace '{}' → '{}'",
        copied,
        if copied == 1 { "y" } else { "ies" },
        args.from,
        dst_ns
    );
    Ok(())
}
pub(crate) fn cmd_migrate(
    opts: CacheOptions,
    args: MigrateArgs,
) -> Result<(), LocalFileCacheError> {
    let dst_db = args.dst_db.unwrap_or_else(|| opts.database_path.clone());
    let dst_ns = args.dst_ns.unwrap_or_else(|| opts.namespace.clone());

    let src: CacheEngine<Vec<u8>> = CacheEngine::open(CacheOptions {
        database_path: args.src_db.clone(),
        namespace: args.src_ns.clone(),
        read_only: false,
        ..CacheOptions::default()
    })?;

    let dst: CacheEngine<Vec<u8>> = CacheEngine::open(CacheOptions {
        database_path: dst_db.clone(),
        namespace: dst_ns.clone(),
        read_only: false,
        ..CacheOptions::default()
    })?;

    let migrated = dst.import_from(&src)?;
    eprintln!(
        "Migrated {} entr{}: {}::{} → {}::{}",
        migrated,
        if migrated == 1 { "y" } else { "ies" },
        args.src_db.display(),
        args.src_ns,
        dst_db.display(),
        dst_ns,
    );
    Ok(())
}
pub(crate) fn cmd_watch(opts: CacheOptions) -> Result<(), LocalFileCacheError> {
    #[cfg(feature = "watching")]
    {
        use localcache::CacheWatcher;

        let engine = localcache::CacheEngine::<Vec<u8>>::open(opts)?;
        let count = engine.entry_count()?;

        if count == 0 {
            eprintln!("No cached entries to watch.");
            return Ok(());
        }

        println!(
            "Watching {} cached entr{} for changes. Press Ctrl-C to stop.",
            count,
            if count == 1 { "y" } else { "ies" }
        );
        println!("{}", "-".repeat(60));

        let watcher: CacheWatcher<Vec<u8>> = engine.watcher()?;
        let rx = watcher.events();

        for event in rx.iter() {
            let reason = match event.reason {
                localcache::InvalidationReason::FileModified => "MODIFIED",
                localcache::InvalidationReason::FileRemoved => "REMOVED ",
                localcache::InvalidationReason::FileRenamed => "RENAMED ",
            };
            println!(
                "[{}] {} {}",
                format_unix_seconds(now_secs()),
                reason,
                event.path.display()
            );
        }
        Ok(())
    }
    #[cfg(not(feature = "watching"))]
    {
        let _ = opts;
        eprintln!("error: the `watch` command requires the `watching` feature.");
        eprintln!("       Rebuild localcache-cli with: --features watching");
        std::process::exit(1);
    }
}
