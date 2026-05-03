//! Example: document analysis pipeline with versioned cache.
//!
//! Shows how `payload_version` lets you invalidate a whole cache when your
//! processing logic changes, and how `batch_set` speeds up bulk ingestion.
//!
//! Run with:
//! ```text
//! cargo run --example document_pipeline
//! ```

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tempfile::TempDir;

use localcache::{CacheEngine, CacheOptions, CacheStatus, ChangeDetectionMode, Codec};

// ---------------------------------------------------------------------------
// Payload type — a rich document analysis result
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Analysis {
    word_count: usize,
    char_count: usize,
    lines: usize,
    language: String,
    keywords: Vec<String>,
    /// Schema version — bump when the analysis logic changes.
    schema_version: u32,
}

const CURRENT_SCHEMA: u32 = 2;

fn analyse(content: &str) -> Analysis {
    let words: Vec<&str> = content.split_whitespace().collect();
    // Very naive keyword extraction: longest unique words.
    let mut unique: Vec<String> = {
        let mut seen = std::collections::HashSet::new();
        words
            .iter()
            .filter(|w| seen.insert(**w))
            .map(|w| w.to_string())
            .collect()
    };
    unique.sort_by_key(|s| std::cmp::Reverse(s.len()));
    unique.truncate(5);

    Analysis {
        word_count: words.len(),
        char_count: content.len(),
        lines: content.lines().count(),
        language: "en".into(),
        keywords: unique,
        schema_version: CURRENT_SCHEMA,
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;

    // Open engine with JSON codec so payloads are human-readable on disk.
    let engine = CacheEngine::<Analysis>::open(CacheOptions {
        database_path: dir.path().join("documents.sqlite3"),
        change_detection_mode: ChangeDetectionMode::MetadataOnly,
        payload_version: CURRENT_SCHEMA,
        namespace: "analysis".into(),
        codec: Codec::Json,
        ..CacheOptions::default()
    })?;

    // Create sample documents.
    let docs: Vec<(PathBuf, String)> = vec![
        (
            dir.path().join("intro.txt"),
            "The quick brown fox jumps over the lazy dog. Rust is fast.".into(),
        ),
        (
            dir.path().join("chapter1.txt"),
            "Memory safety without garbage collection is a core Rust feature.".into(),
        ),
        (
            dir.path().join("chapter2.txt"),
            "Ownership borrow checker lifetime are unique to Rust language design.".into(),
        ),
        (
            dir.path().join("summary.txt"),
            "Rust combines performance safety and expressiveness remarkably well.".into(),
        ),
    ];

    for (path, content) in &docs {
        std::fs::write(path, content)?;
    }

    // -----------------------------------------------------------------------
    // Phase 1: bulk ingest via batch_set
    // -----------------------------------------------------------------------
    println!("=== Phase 1: Bulk ingest ===");
    let items: Vec<(PathBuf, Analysis)> =
        docs.iter().map(|(p, c)| (p.clone(), analyse(c))).collect();

    let report = engine.batch_set(&items)?;
    println!("  Stored:  {}", report.succeeded);
    println!("  Errors:  {}", report.failed.len());

    // -----------------------------------------------------------------------
    // Phase 2: selective re-analysis (only stale)
    // -----------------------------------------------------------------------
    println!("\n=== Phase 2: Check freshness ===");
    // Modify one file to make it stale.
    std::fs::write(
        &docs[1].0,
        "Updated chapter 1: now with more Rust ownership details.",
    )?;

    let stale: Vec<&PathBuf> = docs
        .iter()
        .filter(|(p, _)| {
            matches!(
                engine.check_status(p),
                Ok(CacheStatus::Stale) | Ok(CacheStatus::Missing)
            )
        })
        .map(|(p, _)| p)
        .collect();

    println!("  Stale files: {}", stale.len());
    for p in &stale {
        let content = std::fs::read_to_string(p)?;
        engine.set(*p, &analyse(&content))?;
        println!("  Re-analysed: {}", p.display());
    }

    // -----------------------------------------------------------------------
    // Phase 3: query with payload predicates (json feature)
    // -----------------------------------------------------------------------
    #[cfg(feature = "json")]
    {
        println!("\n=== Phase 3: Query (word_count > 8) ===");
        let results = engine
            .query()
            .field_gt("word_count", 8.0)
            .order_by_field("word_count", false)
            .run()?;

        for entry in &results {
            println!(
                "  {} — {} words, {} keywords",
                entry.path.file_name().unwrap().to_str().unwrap(),
                entry.payload.word_count,
                entry.payload.keywords.len(),
            );
        }
    }

    // -----------------------------------------------------------------------
    // Phase 4: version migration simulation
    // -----------------------------------------------------------------------
    println!("\n=== Phase 4: Version check ===");
    let stats = engine.cache_stats()?;
    println!("  Total entries:   {}", stats.total_entries);
    println!("  By version:      {:?}", stats.entries_by_payload_version);
    println!("  Encoding:        {:?}", stats.entries_by_encoding);

    Ok(())
}
