//! Example: a cache shared by multiple threads with `SyncCacheEngine`.
//!
//! Shows how `SyncCacheEngine` lets multiple threads share a single
//! `CacheEngine` without boilerplate `Arc<Mutex<…>>` management.
//!
//! Run with:
//! ```text
//! cargo run --example multithreaded_cache
//! ```

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use tempfile::TempDir;

use localcache::{ScanOptions, SyncCacheEngine};

static HITS: AtomicUsize = AtomicUsize::new(0);
static MISSES: AtomicUsize = AtomicUsize::new(0);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = Arc::new(TempDir::new()?);

    // Create 20 sample files.
    let paths: Vec<_> = (0..20)
        .map(|i| {
            let p = dir.path().join(format!("item_{i:02}.txt"));
            std::fs::write(&p, format!("File {i}: {}", "data ".repeat(i + 1))).unwrap();
            p
        })
        .collect();

    // Open a shared engine on an in-memory database.
    let engine = SyncCacheEngine::<Vec<f32>>::open(localcache::CacheOptions {
        database_path: ":memory:".into(),
        max_entries: Some(15), // keep only the 15 most recently used
        ..localcache::CacheOptions::default()
    })?;

    // Pre-populate half the entries.
    for p in paths.iter().take(10) {
        let payload: Vec<f32> = (0..64).map(|i| i as f32).collect();
        engine.set(p, &payload)?;
    }

    println!("=== Spawning 8 worker threads ===");
    let engine = Arc::new(engine);
    let paths = Arc::new(paths);

    let handles: Vec<_> = (0..8)
        .map(|tid| {
            let engine = Arc::clone(&engine);
            let paths = Arc::clone(&paths);
            let dir = Arc::clone(&dir);

            thread::spawn(move || {
                for i in 0..20usize {
                    let path = &paths[i % paths.len()];
                    match engine.get_if_fresh(path) {
                        Ok(Some(_)) => {
                            HITS.fetch_add(1, Ordering::Relaxed);
                        }
                        _ => {
                            MISSES.fetch_add(1, Ordering::Relaxed);
                            let payload: Vec<f32> = (0..64).map(|j| (i + j) as f32).collect();
                            engine.set(path, &payload).unwrap();
                        }
                    }
                }
                let _ = dir; // keep TempDir alive
                tid
            })
        })
        .collect();

    for h in handles {
        h.join().expect("thread panicked");
    }

    println!("  Hits:        {}", HITS.load(Ordering::Relaxed));
    println!("  Misses:      {}", MISSES.load(Ordering::Relaxed));

    // Demonstrate scan_dir on the shared engine.
    let results = engine.scan_dir(
        dir.path(),
        ScanOptions {
            recursive: false,
            ..ScanOptions::default()
        }
        .recursive,
    )?;
    let fresh = results
        .iter()
        .filter(|(_, s)| *s == localcache::CacheStatus::Fresh)
        .count();
    println!("  Cache size:  {} (max 15)", engine.entry_count()?);
    println!("  Fresh files: {}/{}", fresh, results.len());

    // Export snapshot.
    let records = engine.export_entries()?;
    println!("  Exported:    {} records", records.len());

    // Use CacheOptionsExt to create options with TTL.
    use localcache::CacheOptionsExt as _;
    let opts_with_ttl = localcache::CacheOptions::default().with_ttl_mins(30);
    println!("  TTL configured: {:?}", opts_with_ttl.ttl);

    Ok(())
}
