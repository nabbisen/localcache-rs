//! Historical fixture generator compiled in an isolated localcache 0.19.0 checkout.

use std::fs;
use std::path::Path;

use localcache::{CacheEngine, JournalMode};

const DATABASE: &str = "/fixture/compat-v0_19-user-index.sqlite3";
const INPUT: &str = "/fixture/input-user-index.bin";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    for path in [DATABASE, INPUT] {
        if Path::new(path).exists() {
            fs::remove_file(path)?;
        }
    }

    fs::write(INPUT, b"synthetic released-v4 user-index fixture input\n")?;
    let engine = CacheEngine::<Vec<f32>>::builder()
        .database(DATABASE)
        .journal_mode(JournalMode::Delete)
        .build()?;
    engine.set(INPUT, &vec![21.0, 34.0])?;
    assert_eq!(engine.create_path_index("rfc010" )?, "lc_user_rfc010");
    drop(engine);
    fs::remove_file(INPUT)?;
    Ok(())
}
