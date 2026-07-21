//! Historical fixture generator compiled in an isolated localcache 0.1.0 checkout.

use std::fs;
use std::path::Path;

use localcache::{CacheEngine, CacheOptions, ChangeDetectionMode};

const DATABASE: &str = "/fixture/compat-v0_1.sqlite3";
const INPUT_A: &str = "/fixture/input-a.bin";
const INPUT_GAP: &str = "/fixture/input-gap.bin";
const INPUT_B: &str = "/fixture/input-b.bin";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    for path in [DATABASE, INPUT_A, INPUT_GAP, INPUT_B] {
        if Path::new(path).exists() {
            fs::remove_file(path)?;
        }
    }

    fs::write(INPUT_A, b"synthetic fixture input A\n")?;
    fs::write(INPUT_GAP, b"synthetic fixture input removed for id gap\n")?;
    fs::write(INPUT_B, b"synthetic fixture input B\n")?;

    let engine = CacheEngine::<Vec<f32>>::open(CacheOptions {
        database_path: DATABASE.into(),
        change_detection_mode: ChangeDetectionMode::MetadataOnly,
    })?;
    engine.set(INPUT_A, &vec![1.25, -2.5, 3.75])?;
    engine.set(INPUT_GAP, &vec![99.0])?;
    engine.set(INPUT_B, &vec![8.5, 13.0])?;
    assert!(engine.remove(INPUT_GAP)?);
    drop(engine);

    for path in [INPUT_A, INPUT_GAP, INPUT_B] {
        fs::remove_file(path)?;
    }
    Ok(())
}
