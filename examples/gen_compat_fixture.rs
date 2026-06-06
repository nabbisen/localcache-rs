//! Generator for the golden compatibility fixture database.
//!
//! This is a **one-off, manually-run** tool.  It writes the file
//! `tests/fixtures/compat-v0_18.sqlite3` that `tests/compat.rs` loads in
//! CI to verify wire-format stability across releases.
//!
//! **Do not run this routinely.**  Regenerating the fixture is the loud,
//! intentional act that signals a wire-format change and triggers CI failures
//! on old fixture decodes until the fixture is updated.
//!
//! # How to run
//!
//! From the repository root:
//!
//! ```sh
//! cargo run --example gen_compat_fixture --features compression
//! ```
//!
//! Commit the resulting `tests/fixtures/compat-v0_18.sqlite3` together with
//! any test changes.  The fixture is intentionally small (< 8 KiB).

use std::path::Path;

use localcache::{CacheEngine, JournalMode};

// Fixture payloads — must match the expected values in tests/compat.rs exactly.
const PLAIN_A: [f32; 3] = [1.0, 2.0, 3.0];
const PLAIN_B: [f32; 3] = [4.0, 5.0, 6.0];
#[cfg(feature = "compression")]
const COMPRESSED_C: [f32; 3] = [7.0, 8.0, 9.0];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fixture_path = Path::new("tests/fixtures/compat-v0_18.sqlite3");

    // Remove any existing fixture so we start clean.
    if fixture_path.exists() {
        std::fs::remove_file(fixture_path)?;
    }

    // Namespace "plain" — two uncompressed bincode entries.
    // We use fake but deterministic path strings: the compat test reads via
    // engine.query() which does not require files to exist on disk.
    {
        let engine = CacheEngine::<Vec<f32>>::builder()
            .database(fixture_path)
            .namespace("plain")
            // Use Delete journal mode so no -wal/-shm sidecars are produced
            // alongside the committed fixture file.
            .journal_mode(JournalMode::Delete)
            .build()?;

        // Write entries using fake file paths.  engine.set() requires the
        // file to exist, so we create temporary stubs in the OS temp dir.
        let tmp = std::env::temp_dir();
        let stub_a = tmp.join("localcache_compat_a.bin");
        let stub_b = tmp.join("localcache_compat_b.bin");
        std::fs::write(&stub_a, b"stub")?;
        std::fs::write(&stub_b, b"stub")?;

        engine.set(&stub_a, &PLAIN_A.to_vec())?;
        engine.set(&stub_b, &PLAIN_B.to_vec())?;

        println!("wrote plain entries: {:?} {:?}", stub_a, stub_b);

        // Clean up stubs (the canonical paths are already stored in DB).
        let _ = std::fs::remove_file(&stub_a);
        let _ = std::fs::remove_file(&stub_b);
    }

    // Namespace "compressed" — one zstd-compressed entry.
    #[cfg(feature = "compression")]
    {
        let engine = CacheEngine::<Vec<f32>>::builder()
            .database(fixture_path)
            .namespace("compressed")
            .journal_mode(JournalMode::Delete)
            .compress()
            .build()?;

        let tmp = std::env::temp_dir();
        let stub_c = tmp.join("localcache_compat_c.bin");
        std::fs::write(&stub_c, b"stub")?;
        engine.set(&stub_c, &COMPRESSED_C.to_vec())?;
        println!("wrote compressed entry: {:?}", stub_c);
        let _ = std::fs::remove_file(&stub_c);
    }

    println!(
        "\nFixture written to {}\nFile size: {} bytes",
        fixture_path.display(),
        std::fs::metadata(fixture_path)?.len()
    );
    println!("\nDo NOT regenerate this file routinely.");
    println!("Commit it alongside any test/doc changes.");

    Ok(())
}
