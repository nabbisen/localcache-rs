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
use crate::cache::query::{SortKey, SortOrder};
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

    let first = engine
        .query()
        .order_by(SortKey::Path, SortOrder::Asc)
        .limit(1)
        .run()
        .unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].path, p_ab_dir, "component-wise order must win");

    let all = engine
        .query()
        .order_by(SortKey::Path, SortOrder::Asc)
        .run()
        .unwrap();
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

    let results = engine
        .query()
        .order_by(SortKey::Field("score".into()), SortOrder::Asc)
        .run()
        .unwrap();
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
// 3. Ordering parity — hazard 1 (Mtime compares mtime, not updated_at)
// ---------------------------------------------------------------------------

#[test]
fn order_by_mtime_compares_source_mtime_not_updated_at_column() {
    let dir = TempDir::new().unwrap();
    let engine = engine();

    let p_low_mtime = write_file(&dir, "low_mtime.txt");
    let p_high_mtime = write_file(&dir, "high_mtime.txt");
    engine.set(&p_low_mtime, &json!({"n": 1})).unwrap();
    engine.set(&p_high_mtime, &json!({"n": 2})).unwrap();

    // mtime and updated_at orderings deliberately disagree.
    set_mtime_updated_at(&engine, &p_low_mtime, 100, 9_999);
    set_mtime_updated_at(&engine, &p_high_mtime, 200, 1_111);

    let results = engine
        .query()
        .order_by(SortKey::Mtime, SortOrder::Asc)
        .run()
        .unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(
        results[0].path, p_low_mtime,
        "SortKey::Mtime must sort by metadata.mtime, not the updated_at column"
    );
    assert_eq!(results[1].path, p_high_mtime);
}

// ---------------------------------------------------------------------------
// 3b. RFC 024 R4 — the deprecated bool-taking methods order exactly like
//     `order_by`/`then_by`, ties included
// ---------------------------------------------------------------------------

fn set_last_accessed(engine: &CacheEngine<Value>, path: &std::path::Path, last_accessed_at: i64) {
    engine
        .conn
        .execute(
            "UPDATE files SET last_accessed_at = ?1 WHERE namespace = ?2 AND path = ?3",
            params![
                last_accessed_at,
                engine.namespace,
                path.display().to_string()
            ],
        )
        .unwrap();
}

/// The deprecated spelling of `order_by(key, order)`.
#[allow(deprecated)]
fn old_order_by<'e>(
    query: crate::cache::query::QueryBuilder<'e, Value>,
    key: &SortKey,
    order: SortOrder,
) -> crate::cache::query::QueryBuilder<'e, Value> {
    let ascending = order == SortOrder::Asc;
    match key {
        SortKey::Field(path) => query.order_by_field(path.clone(), ascending),
        SortKey::Mtime => query.order_by_updated_at(ascending),
        SortKey::LastAccessed => query.order_by_last_accessed(ascending),
        SortKey::Path => query.order_by_path(ascending),
    }
}

/// The deprecated spelling of `then_by(key, order)`.
#[allow(deprecated)]
fn old_then_by<'e>(
    query: crate::cache::query::QueryBuilder<'e, Value>,
    key: &SortKey,
    order: SortOrder,
) -> crate::cache::query::QueryBuilder<'e, Value> {
    let ascending = order == SortOrder::Asc;
    match key {
        SortKey::Field(path) => query.then_by_field(path.clone(), ascending),
        SortKey::Mtime => query.then_by_updated_at(ascending),
        SortKey::LastAccessed => query.then_by_last_accessed(ascending),
        SortKey::Path => query.then_by_path(ascending),
    }
}

#[test]
fn deprecated_sort_methods_order_exactly_like_order_by_and_then_by() {
    let dir = TempDir::new().unwrap();
    let engine = engine();

    // Every key has ties: mtime {100,100,200,200,300,100}, last_accessed
    // {5,0,5,0,7,7}, field n {1,1,2,2,3,3}. Paths are distinct.
    let mtimes = [100, 100, 200, 200, 300, 100];
    let accessed = [5, 0, 5, 0, 7, 7];
    let fields = [1, 1, 2, 2, 3, 3];
    for i in 0..6 {
        let path = write_file(&dir, &format!("f{i}.txt"));
        engine.set(&path, &json!({"n": fields[i]})).unwrap();
        // `updated_at` deliberately disagrees with `mtime`.
        set_mtime_updated_at(&engine, &path, mtimes[i], 1_000 - mtimes[i]);
        set_last_accessed(&engine, &path, accessed[i]);
    }

    let keys = [
        SortKey::Field("n".to_owned()),
        SortKey::Mtime,
        SortKey::LastAccessed,
        SortKey::Path,
    ];
    let orders = [SortOrder::Asc, SortOrder::Desc];

    let mut distinct = std::collections::HashSet::new();
    for key in &keys {
        for order in orders {
            let new = paths_of(&engine.query().order_by(key.clone(), order).run().unwrap());
            let old = paths_of(&old_order_by(engine.query(), key, order).run().unwrap());
            assert_eq!(new, old, "order_by({key:?}, {order:?})");
            distinct.insert(new);
        }
    }
    // The comparison is not vacuous: the eight primaries do not all agree.
    assert!(
        distinct.len() > 2,
        "expected differing orders, got {distinct:?}"
    );

    for primary in &keys {
        for secondary in keys.iter().filter(|k| *k != primary) {
            for first in orders {
                for second in orders {
                    let new = paths_of(
                        &engine
                            .query()
                            .order_by(primary.clone(), first)
                            .then_by(secondary.clone(), second)
                            .run()
                            .unwrap(),
                    );
                    let old = paths_of(
                        &old_then_by(
                            old_order_by(engine.query(), primary, first),
                            secondary,
                            second,
                        )
                        .run()
                        .unwrap(),
                    );
                    assert_eq!(
                        new, old,
                        "order_by({primary:?}, {first:?}).then_by({secondary:?}, {second:?})"
                    );
                }
            }
        }
    }
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
    let limited = engine
        .query()
        .order_by(SortKey::Path, SortOrder::Asc)
        .limit(3)
        .run()
        .unwrap();
    assert_eq!(limited.len(), 3, "backfill must reach past both bad rows");
    let mut limited_paths: Vec<_> = limited.iter().map(|e| e.path.clone()).collect();
    limited_paths.sort();
    assert_eq!(limited_paths, expected);
}

// ---------------------------------------------------------------------------
// 9. RFC 022 R2 — `offset` counts only rows that materialize
// ---------------------------------------------------------------------------

enum BadKind {
    /// File row survives, its payload row is deleted.
    Orphan,
    /// Payload row survives, its content is not valid JSON.
    Corrupt,
}

fn make_bad_row(engine: &CacheEngine<Value>, path: &std::path::Path, kind: BadKind) {
    let id = file_id(engine, path);
    match kind {
        BadKind::Orphan => {
            engine
                .conn
                .execute("DELETE FROM payloads WHERE file_id = ?1", params![id])
                .unwrap();
        }
        BadKind::Corrupt => {
            engine
                .conn
                .execute(
                    "UPDATE payloads SET content = ?1 WHERE file_id = ?2",
                    params![b"not valid json".to_vec(), id],
                )
                .unwrap();
        }
    }
}

fn paths_of(entries: &[crate::cache::entry::CacheEntry<Value>]) -> Vec<PathBuf> {
    entries.iter().map(|e| e.path.clone()).collect()
}

/// Candidates in path order `bad, a, b, c, d` (`bad` = corrupt).
/// `order_by_path(true).limit(2)` at offsets 0, 2, 4 must page `[a,b]`,
/// `[c,d]`, `[]` — not `[a,b]`, `[b,c]`, `[c,d]` (the pre-R2 positional
/// defect, which double-counts `b`). Must fail on v0.21.3.
#[test]
fn offset_counts_only_successfully_decoded_rows() {
    let dir = TempDir::new().unwrap();
    let engine = engine();

    let p_bad = write_file(&dir, "0_bad.txt");
    let p_a = write_file(&dir, "1_a.txt");
    let p_b = write_file(&dir, "2_b.txt");
    let p_c = write_file(&dir, "3_c.txt");
    let p_d = write_file(&dir, "4_d.txt");

    for p in [&p_bad, &p_a, &p_b, &p_c, &p_d] {
        engine.set(p, &json!({"ok": true})).unwrap();
    }
    make_bad_row(&engine, &p_bad, BadKind::Corrupt);

    let page0 = engine
        .query()
        .order_by(SortKey::Path, SortOrder::Asc)
        .offset(0)
        .limit(2)
        .run()
        .unwrap();
    let page1 = engine
        .query()
        .order_by(SortKey::Path, SortOrder::Asc)
        .offset(2)
        .limit(2)
        .run()
        .unwrap();
    let page2 = engine
        .query()
        .order_by(SortKey::Path, SortOrder::Asc)
        .offset(4)
        .limit(2)
        .run()
        .unwrap();

    assert_eq!(paths_of(&page0), vec![p_a.clone(), p_b.clone()], "offset 0");
    assert_eq!(paths_of(&page1), vec![p_c.clone(), p_d.clone()], "offset 2");
    assert!(
        page2.is_empty(),
        "offset 4 (== good-row count) must be empty, not error"
    );
}

/// The same data as `offset_counts_only_successfully_decoded_rows`, forced
/// into tier 3 by a non-`json`-encoded row plus a numeric field predicate
/// that matches every good row (same mechanism as
/// `mixed_encoding_namespace_with_field_predicate_matches_all_decode_path`).
/// Tier 3 was never positionally broken — this proves tier 1/2's fix keeps
/// the two tiers' pages identical, as the handoff requires.
#[test]
fn offset_counts_only_successfully_decoded_rows_tier3() {
    let dir = TempDir::new().unwrap();
    let engine = scored_engine();

    let p_bad = write_file(&dir, "0_bad.txt");
    let p_a = write_file(&dir, "1_a.txt");
    let p_b = write_file(&dir, "2_b.txt");
    let p_c = write_file(&dir, "3_c.txt");
    let p_d = write_file(&dir, "4_d.txt");

    for p in [&p_bad, &p_a, &p_b, &p_c, &p_d] {
        engine.set(p, &Scored { score: 5.0 }).unwrap();
    }

    let bad_id = file_id(&engine, &p_bad);
    engine
        .conn
        .execute(
            "UPDATE payloads SET content = ?1 WHERE file_id = ?2",
            params![b"not valid json".to_vec(), bad_id],
        )
        .unwrap();

    // Force tier 3: re-encode one good row as bincode, same as the
    // mixed-encoding test.
    let (bytes, tag) = encode_payload(
        &Scored { score: 5.0 },
        true,
        Codec::Bincode,
        #[cfg(feature = "encryption")]
        None,
    )
    .unwrap();
    assert_ne!(tag, "json", "the whole point is a non-json row");
    let c_id = file_id(&engine, &p_c);
    engine
        .conn
        .execute(
            "UPDATE payloads SET content = ?1, encoding = ?2 WHERE file_id = ?3",
            params![bytes, tag, c_id],
        )
        .unwrap();

    // Matches every good row (all have score == 5.0).
    let query = || {
        engine
            .query()
            .field_gt("score", -1.0)
            .order_by(SortKey::Path, SortOrder::Asc)
    };
    let page0 = query().offset(0).limit(2).run().unwrap();
    let page1 = query().offset(2).limit(2).run().unwrap();
    let page2 = query().offset(4).limit(2).run().unwrap();

    assert_eq!(
        paths_of3(&page0),
        vec![p_a.clone(), p_b.clone()],
        "offset 0, tier 3"
    );
    assert_eq!(
        paths_of3(&page1),
        vec![p_c.clone(), p_d.clone()],
        "offset 2, tier 3"
    );
    assert!(page2.is_empty(), "offset 4, tier 3");
}

fn paths_of3(entries: &[crate::cache::entry::CacheEntry<Scored>]) -> Vec<PathBuf> {
    entries.iter().map(|e| e.path.clone()).collect()
}

/// A bad row at the start, in the middle, at the end, and every row bad,
/// each queried with `offset > 0`.
#[test]
fn offset_skips_bad_rows_at_any_position() {
    let dir = TempDir::new().unwrap();

    check_offset_around_bad_rows(&dir, "start", &[0]);
    check_offset_around_bad_rows(&dir, "middle", &[2]);
    check_offset_around_bad_rows(&dir, "end", &[4]);
    check_offset_around_bad_rows(&dir, "all", &[0, 1, 2, 3, 4]);
}

fn check_offset_around_bad_rows(dir: &TempDir, label: &str, bad_positions: &[usize]) {
    let engine = engine();
    let paths: Vec<_> = (0..5)
        .map(|i| write_file(dir, &format!("{label}_{i}.txt")))
        .collect();
    for p in &paths {
        engine.set(p, &json!({"ok": true})).unwrap();
    }
    for &i in bad_positions {
        make_bad_row(&engine, &paths[i], BadKind::Corrupt);
    }

    let good_paths: Vec<_> = (0..5)
        .filter(|i| !bad_positions.contains(i))
        .map(|i| paths[i].clone())
        .collect();

    let result = engine
        .query()
        .order_by(SortKey::Path, SortOrder::Asc)
        .offset(1)
        .run()
        .unwrap();
    let expected = if good_paths.len() > 1 {
        good_paths[1..].to_vec()
    } else {
        Vec::new()
    };
    assert_eq!(
        paths_of(&result),
        expected,
        "offset=1 with bad rows at {label} ({bad_positions:?})"
    );
}

/// An orphan (no payload row) behaves exactly like a corrupt row for
/// `offset` purposes: neither counts toward it, and it never appears.
#[test]
fn orphan_row_behaves_like_corrupt_row_for_offset() {
    let dir = TempDir::new().unwrap();

    let engine_orphan = engine();
    let paths_orphan: Vec<_> = (0..5)
        .map(|i| write_file(&dir, &format!("orphan_{i}.txt")))
        .collect();
    for p in &paths_orphan {
        engine_orphan.set(p, &json!({"ok": true})).unwrap();
    }
    make_bad_row(&engine_orphan, &paths_orphan[0], BadKind::Orphan);

    let engine_corrupt = engine();
    let paths_corrupt: Vec<_> = (0..5)
        .map(|i| write_file(&dir, &format!("corrupt_{i}.txt")))
        .collect();
    for p in &paths_corrupt {
        engine_corrupt.set(p, &json!({"ok": true})).unwrap();
    }
    make_bad_row(&engine_corrupt, &paths_corrupt[0], BadKind::Corrupt);

    let result_orphan = engine_orphan
        .query()
        .order_by(SortKey::Path, SortOrder::Asc)
        .offset(1)
        .limit(2)
        .run()
        .unwrap();
    let result_corrupt = engine_corrupt
        .query()
        .order_by(SortKey::Path, SortOrder::Asc)
        .offset(1)
        .limit(2)
        .run()
        .unwrap();

    // Both engines have row 0 bad and rows 1..4 good; offset(1) skips the
    // first good row (index 1), so the page is indices 2 and 3.
    assert_eq!(
        paths_of(&result_orphan),
        vec![paths_orphan[2].clone(), paths_orphan[3].clone()],
        "orphan"
    );
    assert_eq!(
        paths_of(&result_corrupt),
        vec![paths_corrupt[2].clone(), paths_corrupt[3].clone()],
        "corrupt"
    );
}

/// `offset` beyond the materializable count returns an empty `Vec`, not
/// `Err`.
#[test]
fn offset_far_beyond_materializable_count_returns_empty_not_err() {
    let dir = TempDir::new().unwrap();
    let engine = engine();

    let paths: Vec<_> = (0..5)
        .map(|i| write_file(&dir, &format!("f{i}.txt")))
        .collect();
    for p in &paths {
        engine.set(p, &json!({"n": 0})).unwrap();
    }
    make_bad_row(&engine, &paths[0], BadKind::Corrupt);

    let result = engine.query().offset(1000).run().unwrap();
    assert!(result.is_empty());
    let result = engine.query().offset(1000).limit(10).run().unwrap();
    assert!(result.is_empty());
}

/// The decode bound for `offset > 0`: `decode_calls <= offset + limit +
/// bad_rows_encountered`, where `bad_rows_encountered` counts only rows
/// that reach an actual decode attempt (a corrupt payload) — an orphan
/// (no payload row at all) never reaches `decode_with` and costs nothing.
/// `decode_count_is_bounded_by_limit_not_namespace_size` (no `offset`, no
/// bad rows) passes unmodified alongside this.
#[test]
fn decode_count_is_bounded_by_offset_plus_limit_plus_bad_rows() {
    let dir = TempDir::new().unwrap();
    let engine = engine();

    let paths: Vec<_> = (0..10)
        .map(|i| write_file(&dir, &format!("f{i:02}.txt")))
        .collect();
    for p in &paths {
        engine.set(p, &json!({"ok": true})).unwrap();
    }
    // Two corrupt rows inside the path-sorted range the query below must
    // scan through to fill offset(2) + limit(3).
    make_bad_row(&engine, &paths[1], BadKind::Corrupt);
    make_bad_row(&engine, &paths[4], BadKind::Corrupt);
    let bad_rows_encountered = 2;

    reset_decode_calls();
    let offset = 2;
    let limit = 3;
    let results = engine
        .query()
        .order_by(SortKey::Path, SortOrder::Asc)
        .offset(offset)
        .limit(limit)
        .run()
        .unwrap();
    assert_eq!(results.len(), limit);
    assert!(
        decode_calls() <= offset + limit + bad_rows_encountered,
        "decode_calls={} exceeds offset({offset}) + limit({limit}) + bad_rows({bad_rows_encountered})",
        decode_calls()
    );
}
