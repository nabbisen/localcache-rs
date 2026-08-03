//! RFC 021 unit tests — one pass, late materialization.
//!
//! Complements the 57 integration tests in `tests/query.rs`, which must
//! keep passing unmodified. These specifically target the three ordering
//! hazards the RFC's amendment exists to avoid, mixed-encoding safety, the
//! decode-count property the RFC exists to create, `IN`-list chunking, and
//! the skip/backfill behaviour at `offset`/`limit` boundaries.

use std::path::PathBuf;

use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tempfile::TempDir;

use super::DECODE_CALLS;
use crate::cache::engine::CacheEngine;
use crate::cache::options::Codec;
use crate::serialization::encode_payload;

fn engine() -> CacheEngine<Value> {
    CacheEngine::builder()
        .database(":memory:")
        .codec(Codec::Json)
        .build()
        .unwrap()
}

/// A concretely-typed payload, used only by the mixed-encoding test:
/// `serde_json::Value` cannot round-trip through bincode (bincode is not a
/// self-describing format, and `Value`'s `Deserialize` impl needs one), so
/// a fixed struct is required to construct a genuinely valid non-`json` row
/// rather than accidentally testing a decode failure instead.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Scored {
    score: f64,
}

fn scored_engine() -> CacheEngine<Scored> {
    CacheEngine::builder()
        .database(":memory:")
        .codec(Codec::Json)
        .build()
        .unwrap()
}

fn write_file(dir: &TempDir, name: &str) -> PathBuf {
    let path = dir.path().join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&path, b"x").unwrap();
    path
}

fn file_id<T>(engine: &CacheEngine<T>, path: &std::path::Path) -> i64 {
    engine
        .conn
        .query_row(
            "SELECT id FROM files WHERE namespace = ?1 AND path = ?2",
            params![engine.namespace, path.display().to_string()],
            |r| r.get(0),
        )
        .unwrap()
}

fn set_mtime_updated_at(
    engine: &CacheEngine<Value>,
    path: &std::path::Path,
    mtime: i64,
    updated_at: i64,
) {
    engine
        .conn
        .execute(
            "UPDATE files SET mtime = ?1, updated_at = ?2 WHERE namespace = ?3 AND path = ?4",
            params![
                mtime,
                updated_at,
                engine.namespace,
                path.display().to_string()
            ],
        )
        .unwrap();
}

fn reset_decode_calls() {
    DECODE_CALLS.with(|c| c.set(0));
}

fn decode_calls() -> usize {
    DECODE_CALLS.with(|c| c.get())
}

// ---------------------------------------------------------------------------
// 1. Ordering parity — hazard 2 (component-wise vs. byte-wise path order)
// ---------------------------------------------------------------------------

#[test]
fn order_by_path_is_component_wise_not_byte_wise() {
    let dir = TempDir::new().unwrap();
    let engine = engine();

    // `.../a/b` (two components) vs. `.../a-b` (one component). Byte-wise
    // (SQL BINARY) collation puts `a-b` first ('-' < '/'); PathBuf's
    // component-wise `Ord` puts `a/b` first ("a" is a strict prefix of
    // "a-b", so the shorter component sorts first). The RFC's whole point
    // is that the comparator never moves to SQL, so the second must win.
    let p_ab_dir = write_file(&dir, "a/b");
    let p_a_dash_b = write_file(&dir, "a-b");

    engine.set(&p_ab_dir, &json!({"n": 1})).unwrap();
    engine.set(&p_a_dash_b, &json!({"n": 2})).unwrap();

    let first = engine.query().order_by_path(true).limit(1).run().unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].path, p_ab_dir, "component-wise order must win");

    let all = engine.query().order_by_path(true).run().unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].path, p_ab_dir);
    assert_eq!(all[1].path, p_a_dash_b);
}

// ---------------------------------------------------------------------------
// 2. Ordering parity — hazard 3 (numeric / string / missing field)
// ---------------------------------------------------------------------------

#[test]
fn order_by_field_numeric_string_missing() {
    let dir = TempDir::new().unwrap();
    let engine = engine();

    // Named so SQL's default `ORDER BY path` (the pre-sort order the stable
    // Rust sort ties against) is deterministic.
    let p_missing = write_file(&dir, "1_missing.txt");
    let p_string = write_file(&dir, "2_string.txt");
    let p_numeric = write_file(&dir, "3_numeric.txt");

    engine.set(&p_missing, &json!({})).unwrap();
    engine.set(&p_string, &json!({"score": "high"})).unwrap();
    engine.set(&p_numeric, &json!({"score": 5.0})).unwrap();

    let results = engine.query().order_by_field("score", true).run().unwrap();
    assert_eq!(results.len(), 3);
    // A string or missing field both map to `None`, which sorts first
    // ascending (SQLite would instead order NULL < REAL < TEXT, sorting the
    // string entry *after* the numeric one — the exact divergence hazard 3
    // documents). The numeric entry must sort last regardless.
    assert_eq!(
        results[2].path, p_numeric,
        "the only numeric entry sorts last"
    );
    assert_eq!(results[0].path, p_missing, "ties preserve SQL path order");
    assert_eq!(results[1].path, p_string, "ties preserve SQL path order");
}

// ---------------------------------------------------------------------------
// 3. Ordering parity — hazard 1 (UpdatedAt compares mtime, not updated_at)
// ---------------------------------------------------------------------------

#[test]
fn order_by_updated_at_compares_mtime_not_updated_at_column() {
    let dir = TempDir::new().unwrap();
    let engine = engine();

    let p_low_mtime = write_file(&dir, "low_mtime.txt");
    let p_high_mtime = write_file(&dir, "high_mtime.txt");
    engine.set(&p_low_mtime, &json!({"n": 1})).unwrap();
    engine.set(&p_high_mtime, &json!({"n": 2})).unwrap();

    // mtime and updated_at orderings deliberately disagree.
    set_mtime_updated_at(&engine, &p_low_mtime, 100, 9_999);
    set_mtime_updated_at(&engine, &p_high_mtime, 200, 1_111);

    let results = engine.query().order_by_updated_at(true).run().unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(
        results[0].path, p_low_mtime,
        "order_by_updated_at must sort by metadata.mtime, not the updated_at column"
    );
    assert_eq!(results[1].path, p_high_mtime);
}

// ---------------------------------------------------------------------------
// 4. Mixed-encoding safety
// ---------------------------------------------------------------------------

#[test]
fn mixed_encoding_namespace_with_field_predicate_matches_all_decode_path() {
    let dir = TempDir::new().unwrap();
    let engine = scored_engine();

    let p_json_low = write_file(&dir, "json_low.txt");
    let p_json_high = write_file(&dir, "json_high.txt");
    let p_other_high = write_file(&dir, "other_encoding_high.txt");

    engine.set(&p_json_low, &Scored { score: 1.0 }).unwrap();
    engine.set(&p_json_high, &Scored { score: 9.0 }).unwrap();
    engine.set(&p_other_high, &Scored { score: 7.0 }).unwrap();

    // Re-encode the third row as bincode (`"raw"`, or `"zstd"` if the
    // `compression` feature happens to be enabled for this test run),
    // bypassing the engine's own json codec — simulating a namespace where
    // the codec or compression setting changed mid-life. The tier-2
    // uniform-`json` precondition must see this and fall through to tier 3
    // rather than silently dropping the row SQL cannot evaluate.
    let value = Scored { score: 7.0 };
    let (bytes, tag) = encode_payload(
        &value,
        true,
        Codec::Bincode,
        #[cfg(feature = "encryption")]
        None,
    )
    .unwrap();
    assert_ne!(tag, "json", "the whole point is a non-json row");
    let id = file_id(&engine, &p_other_high);
    engine
        .conn
        .execute(
            "UPDATE payloads SET content = ?1, encoding = ?2 WHERE file_id = ?3",
            params![bytes, tag, id],
        )
        .unwrap();

    let results = engine.query().field_gt("score", 5.0).run().unwrap();
    let mut paths: Vec<_> = results.iter().map(|e| e.path.clone()).collect();
    paths.sort();
    let mut expected = vec![p_json_high, p_other_high];
    expected.sort();
    assert_eq!(
        paths, expected,
        "mixed-encoding row must not be silently dropped"
    );
}

// ---------------------------------------------------------------------------
// 5. Decode count is bounded by `limit`
// ---------------------------------------------------------------------------

#[test]
fn decode_count_is_bounded_by_limit_not_namespace_size() {
    let dir = TempDir::new().unwrap();
    let engine = engine();

    for i in 0..50 {
        let p = write_file(&dir, &format!("f{i:03}.txt"));
        engine.set(&p, &json!({"n": i})).unwrap();
    }

    reset_decode_calls();
    let results = engine.query().limit(5).run().unwrap();
    assert_eq!(results.len(), 5);
    assert_eq!(
        decode_calls(),
        5,
        "tier 1 must decode exactly the 5 winning payloads, not all 50 candidates"
    );
}

// ---------------------------------------------------------------------------
// 6. `IN`-list chunking above SQLITE_MAX_VARIABLE_NUMBER (999, older builds)
// ---------------------------------------------------------------------------

#[test]
fn in_list_chunking_above_999() {
    let dir = TempDir::new().unwrap();
    let engine = engine();

    const N: usize = 1100;
    for i in 0..N {
        let p = write_file(&dir, &format!("f{i:04}.txt"));
        engine.set(&p, &json!({"n": i})).unwrap();
    }

    let results = engine.query().limit(N).run().unwrap();
    assert_eq!(
        results.len(),
        N,
        "a limit above 999 must not produce a malformed statement"
    );
}

// ---------------------------------------------------------------------------
// 7. `limit`/`offset` boundaries
// ---------------------------------------------------------------------------

#[test]
fn limit_offset_boundaries() {
    let dir = TempDir::new().unwrap();
    let engine = engine();

    for i in 0..5 {
        let p = write_file(&dir, &format!("f{i}.txt"));
        engine.set(&p, &json!({"n": i})).unwrap();
    }

    assert_eq!(engine.query().limit(0).run().unwrap().len(), 0);
    assert_eq!(engine.query().limit(1).run().unwrap().len(), 1);
    assert_eq!(
        engine.query().limit(5).run().unwrap().len(),
        5,
        "exactly the result count"
    );
    assert_eq!(
        engine.query().limit(10).run().unwrap().len(),
        5,
        "beyond the result count returns everything available"
    );
    assert_eq!(engine.query().offset(3).run().unwrap().len(), 2);
    assert_eq!(
        engine.query().offset(10).run().unwrap().len(),
        0,
        "offset beyond the result count returns nothing"
    );
    assert_eq!(engine.query().offset(3).limit(5).run().unwrap().len(), 2);
}

// ---------------------------------------------------------------------------
// 8. Undecodable payload and a file row with no payload row are skipped,
//    with backfill from later candidates so `limit` is still honoured.
// ---------------------------------------------------------------------------

#[test]
fn undecodable_payload_and_missing_payload_row_are_skipped_and_backfilled() {
    let dir = TempDir::new().unwrap();
    let engine = engine();

    // Interspersed by path so the two bad rows land inside the sorted
    // window a `limit` smaller than the candidate count would otherwise
    // return, forcing backfill to reach past them.
    let p_good_1 = write_file(&dir, "1_good.txt");
    let p_orphan = write_file(&dir, "2_orphan.txt");
    let p_good_2 = write_file(&dir, "3_good.txt");
    let p_corrupt = write_file(&dir, "4_corrupt.txt");
    let p_good_3 = write_file(&dir, "5_good.txt");

    for p in [&p_good_1, &p_orphan, &p_good_2, &p_corrupt, &p_good_3] {
        engine.set(p, &json!({"ok": true})).unwrap();
    }

    // Orphan: file row survives, its payload row is deleted.
    let orphan_id = file_id(&engine, &p_orphan);
    engine
        .conn
        .execute(
            "DELETE FROM payloads WHERE file_id = ?1",
            params![orphan_id],
        )
        .unwrap();

    // Corrupt: payload row survives, its content is not valid JSON.
    let corrupt_id = file_id(&engine, &p_corrupt);
    engine
        .conn
        .execute(
            "UPDATE payloads SET content = ?1 WHERE file_id = ?2",
            params![b"not valid json".to_vec(), corrupt_id],
        )
        .unwrap();

    // All 5 candidates, only 3 decodable — must not error, and must return
    // exactly the 3 good ones.
    let all = engine.query().run().unwrap();
    assert_eq!(all.len(), 3);
    let mut paths: Vec<_> = all.iter().map(|e| e.path.clone()).collect();
    paths.sort();
    let mut expected = vec![p_good_1.clone(), p_good_2.clone(), p_good_3.clone()];
    expected.sort();
    assert_eq!(paths, expected);

    // `limit(3)` against 5 candidates, 2 of which are bad: backfill must
    // still deliver all 3 good ones rather than stopping short at the
    // window a naive `limit` would have covered.
    let limited = engine.query().order_by_path(true).limit(3).run().unwrap();
    assert_eq!(limited.len(), 3, "backfill must reach past both bad rows");
    let mut limited_paths: Vec<_> = limited.iter().map(|e| e.path.clone()).collect();
    limited_paths.sort();
    assert_eq!(limited_paths, expected);
}
