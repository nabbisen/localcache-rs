//! Example: vector embedding cache.
//!
//! Demonstrates how to use `localcache` to cache the result of an expensive
//! embedding computation (here simulated with a cheap function) so that files
//! are only processed when their content changes.
//!
//! Run with:
//! ```text
//! cargo run --example embedding_cache
//! ```

use std::io::Write;
use std::time::Instant;

use tempfile::TempDir;

use localcache::{CacheEngine, CacheStatus, ChangeDetectionMode};

// ---------------------------------------------------------------------------
// Simulated embedding — in production this would call a model API.
// ---------------------------------------------------------------------------

fn embed_file(path: &std::path::Path) -> Vec<f32> {
    // Simulate a slow computation (50 µs for demo purposes).
    let bytes = std::fs::read(path).unwrap_or_default();
    let sum: u64 = bytes.iter().map(|&b| b as u64).sum();
    (0..384).map(|i| (sum as f32 + i as f32) * 0.001).collect()
}

// ---------------------------------------------------------------------------
// Cache-aware embedding function
// ---------------------------------------------------------------------------

fn get_embedding(engine: &CacheEngine<Vec<f32>>, path: &std::path::Path) -> Vec<f32> {
    match engine.get_if_fresh(path) {
        Ok(Some(entry)) => {
            println!("  cache HIT  — {}", path.display());
            entry.payload
        }
        _ => {
            println!("  cache MISS — {}", path.display());
            let embedding = embed_file(path);
            engine.set(path, &embedding).unwrap();
            embedding
        }
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let db_path = dir.path().join("embeddings.sqlite3");

    // Engine configured to detect changes by metadata + full hash.
    let engine = CacheEngine::<Vec<f32>>::builder()
        .database(&db_path)
        .change_detection(ChangeDetectionMode::MetadataThenFullHash)
        .namespace("embeddings")
        .build()?;

    // Create some sample files.
    let files: Vec<_> = (0..5)
        .map(|i| {
            let path = dir.path().join(format!("doc_{i}.txt"));
            std::fs::write(
                &path,
                format!("Document {i}: hello world {}", "x".repeat(i * 10)),
            )
            .unwrap();
            path
        })
        .collect();

    println!("=== First pass (cold cache) ===");
    let t0 = Instant::now();
    for f in &files {
        let _ = get_embedding(&engine, f);
    }
    println!("  Time: {:.1}ms\n", t0.elapsed().as_secs_f64() * 1000.0);

    println!("=== Second pass (warm cache, no changes) ===");
    let t1 = Instant::now();
    for f in &files {
        let _ = get_embedding(&engine, f);
    }
    println!("  Time: {:.1}ms\n", t1.elapsed().as_secs_f64() * 1000.0);

    // Modify one file.
    println!("=== Modifying doc_2.txt ===");
    let mut f = std::fs::OpenOptions::new().append(true).open(&files[2])?;
    writeln!(f, "\n[updated]")?;
    drop(f);

    println!("=== Third pass (one file changed) ===");
    let t2 = Instant::now();
    for file in &files {
        let status = engine.check_status(file)?;
        if status == CacheStatus::Fresh {
            println!("  SKIP (fresh) — {}", file.display());
        } else {
            let embedding = embed_file(file);
            engine.set(file, &embedding)?;
            println!("  RE-EMBED    — {}", file.display());
        }
    }
    println!("  Time: {:.1}ms\n", t2.elapsed().as_secs_f64() * 1000.0);

    let stats = engine.cache_stats()?;
    println!("=== Cache statistics ===");
    println!("  Entries:       {}", stats.total_entries);
    println!("  Payload bytes: {}", stats.total_payload_bytes);

    Ok(())
}
