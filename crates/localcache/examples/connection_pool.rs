//! Example: multi-threaded cache with `ConnectionPool`.
//!
//! Shows how `ConnectionPool` lets multiple threads share a single
//! `CacheEngine` without boilerplate `Arc<Mutex<…>>` management.
//!
//! Run with:
//! ```text
//! cargo run --example connection_pool
//! ```

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use tempfile::TempDir;

use localcache::{ConnectionPool, ScanOptions};

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

    // Open a pool pointing at an in-memory database.
    let pool = ConnectionPool::<Vec<f32>>::open(localcache::CacheOptions {
        database_path: ":memory:".into(),
        max_entries: Some(15), // keep only the 15 most recently used
        ..localcache::CacheOptions::default()
    })?;

    // Pre-populate half the entries.
    for p in paths.iter().take(10) {
        let payload: Vec<f32> = (0..64).map(|i| i as f32).collect();
        pool.set(p, &payload)?;
    }

    println!("=== Spawning 8 worker threads ===");
    let pool = Arc::new(pool);
    let paths = Arc::new(paths);

    let handles: Vec<_> = (0..8)
        .map(|tid| {
            let pool = Arc::clone(&pool);
            let paths = Arc::clone(&paths);
            let dir = Arc::clone(&dir);

            thread::spawn(move || {
                for i in 0..20usize {
                    let path = &paths[i % paths.len()];
                    match pool.get_if_fresh(path) {
                        Ok(Some(_)) => {
                            HITS.fetch_add(1, Ordering::Relaxed);
                        }
                        _ => {
                            MISSES.fetch_add(1, Ordering::Relaxed);
                            let payload: Vec<f32> = (0..64).map(|j| (i + j) as f32).collect();
                            pool.set(path, &payload).unwrap();
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

    // Demonstrate scan_dir on the pool.
    let results = pool.scan_dir(
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
    println!("  Cache size:  {} (max 15)", pool.entry_count()?);
    println!("  Fresh files: {}/{}", fresh, results.len());

    // Export snapshot.
    let records = pool.export_entries()?;
    println!("  Exported:    {} records", records.len());

    // Use CacheOptionsExt to create options with TTL.
    use localcache::CacheOptionsExt as _;
    let opts_with_ttl = localcache::CacheOptions::default().with_ttl_mins(30);
    println!("  TTL configured: {:?}", opts_with_ttl.ttl);

    Ok(())
}
