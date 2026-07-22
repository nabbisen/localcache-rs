//! Integration tests — query.

mod common;
use common::write_file;

use tempfile::TempDir;

#[allow(unused_imports)]
use localcache::{CacheEngine, CacheOptions};

#[cfg(feature = "async")]
// ====================================================================
#[test]
fn contains_returns_true_for_cached_entry() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();

    let path = write_file(&dir, "exist.txt", b"data");
    assert!(!engine.contains(&path).unwrap(), "not yet cached");

    engine.set(&path, &vec![1.0_f32]).unwrap();
    assert!(engine.contains(&path).unwrap(), "should be cached");
}

#[test]
fn contains_returns_false_when_missing() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();
    let path = write_file(&dir, "ghost.txt", b"x");
    assert!(!engine.contains(&path).unwrap());
}

#[test]
fn keys_returns_all_paths() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();

    let p1 = write_file(&dir, "k1.txt", b"a");
    let p2 = write_file(&dir, "k2.txt", b"b");
    let p3 = write_file(&dir, "k3.txt", b"c");

    engine.set(&p1, &vec![1.0_f32]).unwrap();
    engine.set(&p2, &vec![2.0_f32]).unwrap();
    engine.set(&p3, &vec![3.0_f32]).unwrap();

    let keys = engine.keys(None).unwrap();
    assert_eq!(keys.len(), 3);
}

#[test]
fn keys_with_like_filter() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();

    let p1 = write_file(&dir, "alpha.txt", b"a");
    let p2 = write_file(&dir, "beta.txt", b"b");

    engine.set(&p1, &vec![1.0_f32]).unwrap();
    engine.set(&p2, &vec![2.0_f32]).unwrap();

    // Filter paths ending in "alpha.txt" using LIKE
    let keys = engine.keys(Some("%alpha.txt")).unwrap();
    assert_eq!(keys.len(), 1);
    assert!(keys[0].to_string_lossy().ends_with("alpha.txt"));
}

#[test]
fn keys_empty_namespace() {
    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();
    let keys = engine.keys(None).unwrap();
    assert!(keys.is_empty());
}

// ====================================================================
// Phase 10 — QueryBuilder
// ====================================================================

#[cfg(feature = "json")]
mod query_tests {
    use super::*;
    use localcache::Codec;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct Article {
        title: String,
        score: f64,
        tags: Vec<String>,
    }

    fn make_query_engine(_dir: &TempDir) -> CacheEngine<Article> {
        CacheEngine::builder()
            .database(":memory:")
            .codec(Codec::Json)
            .build()
            .unwrap()
    }

    #[test]
    fn query_field_gt_filters_correctly() {
        let dir = TempDir::new().unwrap();
        let engine = make_query_engine(&dir);

        let p1 = write_file(&dir, "high.txt", b"x");
        let p2 = write_file(&dir, "low.txt", b"y");

        engine
            .set(
                &p1,
                &Article {
                    title: "High".into(),
                    score: 0.95,
                    tags: vec![],
                },
            )
            .unwrap();
        engine
            .set(
                &p2,
                &Article {
                    title: "Low".into(),
                    score: 0.3,
                    tags: vec![],
                },
            )
            .unwrap();

        let results = engine.query().field_gt("score", 0.5).run().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].payload.title, "High");
    }

    #[test]
    fn query_field_lt_filters_correctly() {
        let dir = TempDir::new().unwrap();
        let engine = make_query_engine(&dir);

        let p1 = write_file(&dir, "a.txt", b"x");
        let p2 = write_file(&dir, "b.txt", b"y");

        engine
            .set(
                &p1,
                &Article {
                    title: "A".into(),
                    score: 0.1,
                    tags: vec![],
                },
            )
            .unwrap();
        engine
            .set(
                &p2,
                &Article {
                    title: "B".into(),
                    score: 0.9,
                    tags: vec![],
                },
            )
            .unwrap();

        let results = engine.query().field_lt("score", 0.5).run().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].payload.title, "A");
    }

    #[test]
    fn query_field_eq_string() {
        let dir = TempDir::new().unwrap();
        let engine = make_query_engine(&dir);

        let p1 = write_file(&dir, "eq1.txt", b"x");
        let p2 = write_file(&dir, "eq2.txt", b"y");

        engine
            .set(
                &p1,
                &Article {
                    title: "Rust".into(),
                    score: 0.8,
                    tags: vec![],
                },
            )
            .unwrap();
        engine
            .set(
                &p2,
                &Article {
                    title: "Go".into(),
                    score: 0.7,
                    tags: vec![],
                },
            )
            .unwrap();

        let results = engine
            .query()
            .field_eq("title", serde_json::json!("Rust"))
            .run()
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].payload.title, "Rust");
    }

    #[test]
    fn query_field_contains_substring() {
        let dir = TempDir::new().unwrap();
        let engine = make_query_engine(&dir);

        let p1 = write_file(&dir, "sub1.txt", b"x");
        let p2 = write_file(&dir, "sub2.txt", b"y");

        engine
            .set(
                &p1,
                &Article {
                    title: "Hello World".into(),
                    score: 0.5,
                    tags: vec![],
                },
            )
            .unwrap();
        engine
            .set(
                &p2,
                &Article {
                    title: "Goodbye".into(),
                    score: 0.5,
                    tags: vec![],
                },
            )
            .unwrap();

        let results = engine
            .query()
            .field_contains("title", "World")
            .run()
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].payload.title, "Hello World");
    }

    #[test]
    fn query_payload_contains() {
        let dir = TempDir::new().unwrap();
        let engine = make_query_engine(&dir);

        let p1 = write_file(&dir, "pc1.txt", b"x");
        let p2 = write_file(&dir, "pc2.txt", b"y");

        engine
            .set(
                &p1,
                &Article {
                    title: "Rust".into(),
                    score: 0.9,
                    tags: vec!["systems".into()],
                },
            )
            .unwrap();
        engine
            .set(
                &p2,
                &Article {
                    title: "Python".into(),
                    score: 0.8,
                    tags: vec!["scripting".into()],
                },
            )
            .unwrap();

        let results = engine.query().payload_contains("systems").run().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].payload.title, "Rust");
    }

    #[test]
    fn query_limit() {
        let dir = TempDir::new().unwrap();
        let engine = make_query_engine(&dir);

        for i in 0..5u32 {
            let p = write_file(&dir, &format!("lim{i}.txt"), b"x");
            engine
                .set(
                    &p,
                    &Article {
                        title: format!("Item {i}"),
                        score: 0.5,
                        tags: vec![],
                    },
                )
                .unwrap();
        }

        let results = engine.query().limit(2).run().unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn query_combined_predicates() {
        let dir = TempDir::new().unwrap();
        let engine = make_query_engine(&dir);

        for i in 0..6u32 {
            let p = write_file(&dir, &format!("comb{i}.txt"), b"x");
            engine
                .set(
                    &p,
                    &Article {
                        title: if i % 2 == 0 {
                            "Even".into()
                        } else {
                            "Odd".into()
                        },
                        score: i as f64 * 0.1,
                        tags: vec![],
                    },
                )
                .unwrap();
        }

        // title == "Even" AND score > 0.3
        let results = engine
            .query()
            .field_eq("title", serde_json::json!("Even"))
            .field_gt("score", 0.3)
            .run()
            .unwrap();

        // Scores for "Even": 0.0, 0.2, 0.4 → only 0.4 > 0.3
        assert_eq!(results.len(), 1);
        assert!((results[0].payload.score - 0.4).abs() < 1e-10);
    }

    #[test]
    fn query_path_like_filter() {
        let dir = TempDir::new().unwrap();
        let engine = make_query_engine(&dir);

        let p1 = write_file(&dir, "group_a.txt", b"x");
        let p2 = write_file(&dir, "group_b.txt", b"y");
        let p3 = write_file(&dir, "other.txt", b"z");

        for p in [&p1, &p2, &p3] {
            engine
                .set(
                    p,
                    &Article {
                        title: "T".into(),
                        score: 1.0,
                        tags: vec![],
                    },
                )
                .unwrap();
        }

        let results = engine.query().path_like("%group_%.txt").run().unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn query_no_matches_returns_empty() {
        let dir = TempDir::new().unwrap();
        let engine = make_query_engine(&dir);

        let p = write_file(&dir, "nm.txt", b"x");
        engine
            .set(
                &p,
                &Article {
                    title: "Test".into(),
                    score: 0.5,
                    tags: vec![],
                },
            )
            .unwrap();

        let results = engine.query().field_gt("score", 0.99).run().unwrap();
        assert!(results.is_empty());
    }
}

// ====================================================================
// Phase 11 — QueryBuilder order_by + offset + limit
// ====================================================================

#[cfg(feature = "json")]
mod query_ordering_tests {
    use super::*;
    use localcache::Codec;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize, Clone)]
    struct Item {
        name: String,
        rank: f64,
    }

    fn ordered_engine(dir: &TempDir) -> CacheEngine<Item> {
        let engine: CacheEngine<Item> = CacheEngine::builder()
            .database(":memory:")
            .codec(Codec::Json)
            .build()
            .unwrap();
        // Insert 5 items with distinct ranks.
        for i in 0..5u32 {
            let p = write_file(dir, &format!("ord{i}.txt"), b"x");
            engine
                .set(
                    &p,
                    &Item {
                        name: format!("item{i}"),
                        rank: i as f64,
                    },
                )
                .unwrap();
        }
        engine
    }

    #[test]
    fn order_by_field_ascending() {
        let dir = TempDir::new().unwrap();
        let engine = ordered_engine(&dir);
        let results = engine.query().order_by_field("rank", true).run().unwrap();
        assert_eq!(results.len(), 5);
        for w in results.windows(2) {
            assert!(
                w[0].payload.rank <= w[1].payload.rank,
                "should be ascending"
            );
        }
    }

    #[test]
    fn order_by_field_descending() {
        let dir = TempDir::new().unwrap();
        let engine = ordered_engine(&dir);
        let results = engine.query().order_by_field("rank", false).run().unwrap();
        assert_eq!(results.len(), 5);
        for w in results.windows(2) {
            assert!(
                w[0].payload.rank >= w[1].payload.rank,
                "should be descending"
            );
        }
    }

    #[test]
    fn order_by_path_ascending() {
        let dir = TempDir::new().unwrap();
        let engine = ordered_engine(&dir);
        let results = engine.query().order_by_path(true).run().unwrap();
        assert_eq!(results.len(), 5);
        for w in results.windows(2) {
            assert!(w[0].path <= w[1].path, "should be path-ascending");
        }
    }

    #[test]
    fn offset_skips_first_n() {
        let dir = TempDir::new().unwrap();
        let engine = ordered_engine(&dir);
        let all = engine.query().order_by_field("rank", true).run().unwrap();
        let skipped = engine
            .query()
            .order_by_field("rank", true)
            .offset(2)
            .run()
            .unwrap();
        assert_eq!(skipped.len(), 3);
        assert!((skipped[0].payload.rank - all[2].payload.rank).abs() < 1e-10);
    }

    #[test]
    fn offset_plus_limit_paging() {
        let dir = TempDir::new().unwrap();
        let engine = ordered_engine(&dir);

        // Page 1: items 0-1
        let p1 = engine
            .query()
            .order_by_field("rank", true)
            .limit(2)
            .offset(0)
            .run()
            .unwrap();
        // Page 2: items 2-3
        let p2 = engine
            .query()
            .order_by_field("rank", true)
            .limit(2)
            .offset(2)
            .run()
            .unwrap();
        // Page 3: item 4
        let p3 = engine
            .query()
            .order_by_field("rank", true)
            .limit(2)
            .offset(4)
            .run()
            .unwrap();

        assert_eq!(p1.len(), 2);
        assert_eq!(p2.len(), 2);
        assert_eq!(p3.len(), 1);
        // Pages should not overlap.
        assert!((p1[1].payload.rank - p2[0].payload.rank).abs() > 0.5);
    }

    #[test]
    fn offset_beyond_results_is_empty() {
        let dir = TempDir::new().unwrap();
        let engine = ordered_engine(&dir);
        let results = engine.query().offset(100).run().unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn order_then_filter_combined() {
        let dir = TempDir::new().unwrap();
        let engine = ordered_engine(&dir);
        let results = engine
            .query()
            .field_lt("rank", 3.0)
            .order_by_field("rank", false) // descending within the matches
            .run()
            .unwrap();
        // Items 0, 1, 2 match rank < 3.0
        assert_eq!(results.len(), 3);
        // First result should have highest rank among matches
        assert!((results[0].payload.rank - 2.0).abs() < 1e-10);
    }
}

// ====================================================================
// Phase 11 — touch()
// ====================================================================

#[test]
fn touch_updates_last_accessed_at() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();

    let path = write_file(&dir, "touch.txt", b"content");
    engine.set(&path, &vec![1.0_f32]).unwrap();

    let before = engine.list_entries().unwrap()[0].last_accessed_at;
    assert_eq!(before, 0, "should be 0 before any access");

    let updated = engine.touch(&path).unwrap();
    assert!(updated, "touch should return true for existing entry");

    let after = engine.list_entries().unwrap()[0].last_accessed_at;
    assert!(after > 0, "last_accessed_at should be set after touch");
}

#[test]
fn touch_returns_false_for_missing_entry() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();

    let path = write_file(&dir, "no_entry.txt", b"x");
    // Never set — touch should return false.
    assert!(!engine.touch(&path).unwrap());
}

#[test]
fn touch_protects_from_lru_eviction() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(":memory:")
        .max_entries(2)
        .build()
        .unwrap();

    let p1 = write_file(&dir, "t1.txt", b"a");
    let p2 = write_file(&dir, "t2.txt", b"b");
    engine.set(&p1, &vec![1.0_f32]).unwrap();
    engine.set(&p2, &vec![2.0_f32]).unwrap();

    // Warm p1 — it should now have a higher last_accessed_at than p2.
    engine.touch(&p1).unwrap();

    // Adding p3 should evict p2 (less recently accessed) not p1.
    let p3 = write_file(&dir, "t3.txt", b"c");
    engine.set(&p3, &vec![3.0_f32]).unwrap();

    assert_eq!(engine.entry_count().unwrap(), 2);
    assert!(
        engine.get(&p1).unwrap().is_some(),
        "p1 should survive (was touched)"
    );
    assert!(engine.get(&p3).unwrap().is_some(), "p3 is newest write");
}

// ====================================================================
// Phase 11 — Persistent index management
// ====================================================================

#[test]
fn create_and_list_and_drop_index() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(dir.path().join("idx.sqlite3"))
        .build()
        .unwrap();

    // Start with no user indexes.
    let before = engine.list_path_indexes().unwrap();
    let initial_count = before.len();

    // Create two indexes.
    let name_a = engine.create_path_index("by_prefix").unwrap();
    let name_b = engine.create_path_index("by_suffix").unwrap();
    assert_eq!(name_a, "lc_user_by_prefix");
    assert_eq!(name_b, "lc_user_by_suffix");

    let listed = engine.list_path_indexes().unwrap();
    assert_eq!(listed.len(), initial_count + 2);
    assert!(listed.contains(&name_a));
    assert!(listed.contains(&name_b));

    // Drop one.
    assert!(engine.drop_path_index("by_prefix").unwrap());
    let after = engine.list_path_indexes().unwrap();
    assert_eq!(after.len(), initial_count + 1);
    assert!(!after.contains(&name_a));

    // Dropping non-existent index returns false.
    assert!(!engine.drop_path_index("by_prefix").unwrap());
}

#[test]
fn create_index_is_idempotent() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(dir.path().join("idem.sqlite3"))
        .build()
        .unwrap();

    engine.create_path_index("myidx").unwrap();
    // Second call should not error.
    engine.create_path_index("myidx").unwrap();

    let indexes = engine.list_path_indexes().unwrap();
    assert_eq!(
        indexes
            .iter()
            .filter(|n| n.as_str() == "lc_user_myidx")
            .count(),
        1
    );
}

#[test]
fn hostile_index_identifiers_fail_closed_without_mutation() {
    use localcache::LocalFileCacheError;
    use rusqlite::Connection;

    const SAFETY_ERROR: &str =
        "SQLite index identifier is invalid or is not an allowed localcache index";
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("hostile-index.sqlite3");
    let engine: CacheEngine<Vec<f32>> = CacheEngine::builder().database(&database).build().unwrap();
    let cached_path = write_file(&directory, "hostile-survivor.txt", b"safe");
    engine.set(&cached_path, &vec![1.25_f32, -2.5]).unwrap();
    let snapshot = || {
        let observer = Connection::open(&database).unwrap();
        let user_version = observer
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap();
        let journal_mode = observer
            .query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
            .unwrap();
        let schema = observer
            .query_row(
                "SELECT group_concat(type || ':' || name || ':' || coalesce(sql, ''), '|')
                 FROM (SELECT type, name, sql FROM main.sqlite_schema ORDER BY type, name)",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap();
        let file_count = observer
            .query_row("SELECT count(*) FROM main.files", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap();
        let payloads = observer
            .prepare("SELECT content FROM main.payloads ORDER BY file_id")
            .unwrap()
            .query_map([], |row| row.get::<_, Vec<u8>>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        (user_version, journal_mode, schema, file_count, payloads)
    };
    let before = snapshot();

    for suffix in [
        "",
        "has space",
        "line\nbreak",
        "tab\tname",
        "control\u{1f}",
        "dollar$sign",
        "éclair",
        "schema.name",
        "bracket[name]",
        "back`tick",
        "single'quote",
        "double\"quote",
        "slash/*comment*/",
        "x ON files(namespace,path); DROP TABLE files; --",
        "x\"; DROP TABLE files;--",
        "x'); PRAGMA user_version=99;--",
        &"a".repeat(65),
    ] {
        let error = engine.create_path_index(suffix).unwrap_err();
        assert!(matches!(
            &error,
            LocalFileCacheError::UnsupportedFeature(message) if message == SAFETY_ERROR
        ));
        if !suffix.is_empty() {
            assert!(!error.to_string().contains(suffix));
        }
        assert!(!engine.drop_path_index(suffix).unwrap());

        let full = format!("lc_user_{suffix}");
        for error in [
            engine.query().index_hint(&full).run().unwrap_err(),
            engine.query().index_hint(&full).dry_run().unwrap_err(),
        ] {
            assert!(matches!(
                &error,
                LocalFileCacheError::UnsupportedFeature(message) if message == SAFETY_ERROR
            ));
            if !suffix.is_empty() {
                assert!(!error.to_string().contains(suffix));
            }
        }
        assert_eq!(snapshot(), before, "mutation for hostile suffix {suffix:?}");
    }

    assert!(engine.list_path_indexes().unwrap().is_empty());
    assert_eq!(
        engine.get(&cached_path).unwrap().unwrap().payload,
        vec![1.25_f32, -2.5]
    );
}

#[test]
fn index_creation_grammar_accepts_boundaries_and_reserved_words() {
    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();
    for suffix in ["a", "_", "9", "Mixed_Case_9", "select"] {
        assert_eq!(
            engine.create_path_index(suffix).unwrap(),
            format!("lc_user_{suffix}")
        );
    }
    let boundary = "a".repeat(64);
    assert_eq!(
        engine.create_path_index(&boundary).unwrap(),
        format!("lc_user_{boundary}")
    );
}

#[test]
fn read_only_precedes_identifier_validation_for_index_mutations() {
    use localcache::LocalFileCacheError;

    let directory = TempDir::new().unwrap();
    let database = directory.path().join("read-only-index.sqlite3");
    drop(
        CacheEngine::<Vec<f32>>::builder()
            .database(&database)
            .build()
            .unwrap(),
    );
    let engine = CacheEngine::<Vec<f32>>::builder()
        .database(&database)
        .read_only()
        .build()
        .unwrap();
    assert!(matches!(
        engine.create_path_index("x\";--"),
        Err(LocalFileCacheError::ReadOnly)
    ));
    assert!(matches!(
        engine.drop_path_index("x\";--"),
        Err(LocalFileCacheError::ReadOnly)
    ));
}

// ====================================================================
// Phase 11 — Async touch / keys / index
// ====================================================================

#[cfg(feature = "async")]
mod async_phase11_tests {
    use super::*;
    use localcache::AsyncCacheEngine;

    #[tokio::test]
    async fn async_touch() {
        let dir = TempDir::new().unwrap();
        let engine: AsyncCacheEngine<Vec<f32>> = AsyncCacheEngine::open(CacheOptions {
            database_path: ":memory:".into(),
            ..CacheOptions::default()
        })
        .await
        .unwrap();

        let path = write_file(&dir, "at.txt", b"x");
        engine.set(path.clone(), vec![1.0_f32]).await.unwrap();

        let updated = engine.touch(path).await.unwrap();
        assert!(updated);
    }

    #[tokio::test]
    async fn async_keys() {
        let dir = TempDir::new().unwrap();
        let engine: AsyncCacheEngine<Vec<f32>> = AsyncCacheEngine::open(CacheOptions {
            database_path: ":memory:".into(),
            ..CacheOptions::default()
        })
        .await
        .unwrap();

        let p = write_file(&dir, "ak.txt", b"x");
        engine.set(p, vec![1.0_f32]).await.unwrap();

        let keys = engine.keys(None).await.unwrap();
        assert_eq!(keys.len(), 1);
    }

    #[tokio::test]
    async fn async_contains() {
        let dir = TempDir::new().unwrap();
        let engine: AsyncCacheEngine<Vec<f32>> = AsyncCacheEngine::open(CacheOptions {
            database_path: ":memory:".into(),
            ..CacheOptions::default()
        })
        .await
        .unwrap();

        let p = write_file(&dir, "ac.txt", b"x");
        assert!(!engine.contains(p.clone()).await.unwrap());
        engine.set(p.clone(), vec![1.0_f32]).await.unwrap();
        assert!(engine.contains(p).await.unwrap());
    }

    #[tokio::test]
    async fn async_index_lifecycle() {
        let dir = TempDir::new().unwrap();
        let engine: AsyncCacheEngine<Vec<f32>> = AsyncCacheEngine::open(CacheOptions {
            database_path: dir.path().join("aidx.sqlite3"),
            ..CacheOptions::default()
        })
        .await
        .unwrap();

        let name = engine
            .create_path_index("asyncidx".to_owned())
            .await
            .unwrap();
        assert_eq!(name, "lc_user_asyncidx");

        let indexes = engine.list_path_indexes().await.unwrap();
        assert!(indexes.contains(&name));

        let dropped = engine.drop_path_index("asyncidx".to_owned()).await.unwrap();
        assert!(dropped);
    }

    #[tokio::test]
    async fn async_index_identifier_rejection_matches_sync_contract() {
        use localcache::LocalFileCacheError;

        let engine: AsyncCacheEngine<Vec<f32>> = AsyncCacheEngine::open(CacheOptions {
            database_path: ":memory:".into(),
            ..CacheOptions::default()
        })
        .await
        .unwrap();
        let error = engine
            .create_path_index("x\"; DROP TABLE files;--".to_owned())
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            LocalFileCacheError::UnsupportedFeature(ref message)
                if message == "SQLite index identifier is invalid or is not an allowed localcache index"
        ));
    }
}

#[cfg(all(not(feature = "async"), feature = "async-std"))]
#[test]
fn async_std_index_identifier_rejection_matches_sync_contract() {
    async_std::task::block_on(async_index_identifier_rejection());
}

#[cfg(all(not(feature = "async"), not(feature = "async-std"), feature = "smol"))]
#[test]
fn smol_index_identifier_rejection_matches_sync_contract() {
    smol::block_on(async_index_identifier_rejection());
}

#[cfg(any(
    all(not(feature = "async"), feature = "async-std"),
    all(not(feature = "async"), not(feature = "async-std"), feature = "smol")
))]
async fn async_index_identifier_rejection() {
    use localcache::{AsyncCacheEngine, CacheOptions, LocalFileCacheError};

    let engine: AsyncCacheEngine<Vec<f32>> = AsyncCacheEngine::open(CacheOptions {
        database_path: ":memory:".into(),
        ..CacheOptions::default()
    })
    .await
    .unwrap();
    let error = engine
        .create_path_index("x\"; DROP TABLE files;--".to_owned())
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        LocalFileCacheError::UnsupportedFeature(ref message)
            if message == "SQLite index identifier is invalid or is not an allowed localcache index"
    ));
}

// ====================================================================
// Phase 12 —

// ============================================================
// RFC 002 — Query Index Hints and Explain Plan
// ============================================================

mod rfc002_index_hints {
    use tempfile::TempDir;

    use localcache::CacheEngine;

    fn populated_engine(dir: &TempDir) -> CacheEngine<Vec<f32>> {
        let engine = CacheEngine::<Vec<f32>>::builder()
            .database(":memory:")
            .build()
            .unwrap();
        for i in 0..10u32 {
            let path = dir.path().join(format!("doc{i:02}.txt"));
            std::fs::write(&path, format!("content {i}")).unwrap();
            engine.set(&path, &vec![i as f32]).unwrap();
        }
        engine
    }

    // ------------------------------------------------------------------
    // dry_run returns a non-empty plan string
    // ------------------------------------------------------------------
    #[test]
    fn dry_run_returns_plan() {
        let dir = TempDir::new().unwrap();
        let engine = populated_engine(&dir);

        let plan = engine.query().dry_run().unwrap();
        assert!(!plan.is_empty(), "dry_run must return a non-empty plan");
        // SQLite's EXPLAIN QUERY PLAN output always mentions the table.
        let plan_lower = plan.to_lowercase();
        assert!(
            plan_lower.contains("scan") || plan_lower.contains("search"),
            "plan should contain SCAN or SEARCH: {plan}"
        );
    }

    // ------------------------------------------------------------------
    // dry_run with path_like
    // ------------------------------------------------------------------
    #[test]
    fn dry_run_with_path_like() {
        let dir = TempDir::new().unwrap();
        let engine = populated_engine(&dir);

        let plan = engine.query().path_like("%.txt").dry_run().unwrap();
        assert!(!plan.is_empty());
    }

    // ------------------------------------------------------------------
    // dry_run does not load any entries
    // ------------------------------------------------------------------
    #[test]
    fn dry_run_does_not_load_payloads() {
        let dir = TempDir::new().unwrap();
        let engine = populated_engine(&dir);

        let count_before = engine.entry_count().unwrap();
        let _plan = engine.query().path_like("%.txt").dry_run().unwrap();
        let count_after = engine.entry_count().unwrap();

        assert_eq!(
            count_before, count_after,
            "dry_run must not modify the cache"
        );
    }

    // ------------------------------------------------------------------
    // index_hint with valid index — query returns correct results
    // ------------------------------------------------------------------
    #[test]
    fn index_hint_valid_index_returns_results() {
        let dir = TempDir::new().unwrap();
        let engine = populated_engine(&dir);

        let idx_full = engine.create_path_index("rfc002test").unwrap();
        assert_eq!(idx_full, "lc_user_rfc002test");

        let results = engine
            .query()
            .path_like("%.txt")
            .index_hint(&idx_full)
            .run()
            .unwrap();

        assert_eq!(results.len(), 10, "should return all 10 entries with hint");
        engine.drop_path_index("rfc002test").unwrap();
    }

    // ------------------------------------------------------------------
    // index_hint with invalid index — run() returns a Database error
    // ------------------------------------------------------------------
    #[test]
    fn index_hint_invalid_index_returns_error() {
        let dir = TempDir::new().unwrap();
        let engine = populated_engine(&dir);

        let result = engine.query().index_hint("nonexistent_index_xyz").run();

        assert!(
            result.is_err(),
            "expected error for invalid index hint: {result:?}"
        );
    }

    // ------------------------------------------------------------------
    // dry_run with index_hint — plan mentions the index
    // ------------------------------------------------------------------
    #[test]
    fn dry_run_with_index_hint_mentions_index() {
        let dir = TempDir::new().unwrap();
        let engine = populated_engine(&dir);

        let idx_full = engine.create_path_index("dryrunidx").unwrap();
        let plan = engine
            .query()
            .path_like("%.txt")
            .index_hint(&idx_full)
            .dry_run()
            .unwrap();

        assert!(
            plan.contains(&idx_full),
            "dry_run plan should mention the hinted index; got: {plan}"
        );
        engine.drop_path_index("dryrunidx").unwrap();
    }

    // ------------------------------------------------------------------
    // async query_dry_run wrapper
    // ------------------------------------------------------------------
    #[cfg(feature = "async")]
    #[tokio::test]
    async fn async_query_dry_run() {
        use localcache::{AsyncCacheEngine, CacheOptions};

        let engine = AsyncCacheEngine::<Vec<f32>>::open(CacheOptions {
            database_path: ":memory:".into(),
            ..CacheOptions::default()
        })
        .await
        .unwrap();

        let plan = engine
            .query_dry_run(|q| q.path_like("%.txt"))
            .await
            .unwrap();

        assert!(!plan.is_empty(), "async dry_run must return a plan");
    }
}

// ============================================================
// RFC 006 — Directory-scoped Query Predicates
// ============================================================

mod rfc006_dir_predicates {
    use std::fs;

    use tempfile::TempDir;

    use localcache::CacheEngine;

    // ── Fixture ────────────────────────────────────────────────────────────
    //
    //   root/
    //     a.txt   (cached)
    //     b.txt   (cached)
    //     sub/
    //       c.txt   (cached)
    //       sub2/
    //         d.txt   (cached)
    //     other/
    //       e.txt   (cached)

    struct Fixture {
        _dir: TempDir,
        root: std::path::PathBuf,
        a: std::path::PathBuf,
        b: std::path::PathBuf,
        c: std::path::PathBuf,
        d: std::path::PathBuf,
        e: std::path::PathBuf,
    }

    fn make_fixture() -> (Fixture, CacheEngine<Vec<f32>>) {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("root");
        let sub = root.join("sub");
        let sub2 = sub.join("sub2");
        let other = root.join("other");
        for d in [&root, &sub, &sub2, &other] {
            fs::create_dir(d).unwrap();
        }

        let a = write_file_at(&root, "a.txt", b"a");
        let b = write_file_at(&root, "b.txt", b"b");
        let c = write_file_at(&sub, "c.txt", b"c");
        let d = write_file_at(&sub2, "d.txt", b"d");
        let e = write_file_at(&other, "e.txt", b"e");

        let engine: CacheEngine<Vec<f32>> = CacheEngine::builder()
            .database(dir.path().join("rfc006.sqlite3"))
            .build()
            .unwrap();
        for (path, val) in [(&a, 1.0), (&b, 2.0), (&c, 3.0), (&d, 4.0), (&e, 5.0)] {
            engine.set(path, &vec![val]).unwrap();
        }

        let fix = Fixture {
            _dir: dir,
            root,
            a,
            b,
            c,
            d,
            e,
        };
        (fix, engine)
    }

    fn write_file_at(dir: &std::path::Path, name: &str, content: &[u8]) -> std::path::PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, content).unwrap();
        p
    }

    // Sort helper for stable comparison.
    fn sorted_paths(entries: Vec<localcache::CacheEntry<Vec<f32>>>) -> Vec<std::path::PathBuf> {
        let mut v: Vec<_> = entries.into_iter().map(|e| e.path).collect();
        v.sort();
        v
    }

    // ── path_in_dir, non-recursive ──────────────────────────────────────────

    #[test]
    fn path_in_dir_non_recursive_returns_direct_children_only() {
        let (fix, engine) = make_fixture();
        let results = engine.query().path_in_dir(&fix.root, false).run().unwrap();
        let paths = sorted_paths(results);
        // Only a.txt and b.txt are direct children of root.
        assert_eq!(paths, vec![fix.a.clone(), fix.b.clone()]);
    }

    #[test]
    fn path_in_dir_non_recursive_subdir_returns_only_its_direct_children() {
        let (fix, engine) = make_fixture();
        let sub = fix.root.join("sub");
        let results = engine.query().path_in_dir(&sub, false).run().unwrap();
        let paths = sorted_paths(results);
        // c.txt is a direct child of sub; d.txt is in sub/sub2 — excluded.
        assert_eq!(paths, vec![fix.c.clone()]);
    }

    #[test]
    fn path_in_dir_non_recursive_excludes_subdirectory_entries() {
        let (fix, engine) = make_fixture();
        let results = engine.query().path_in_dir(&fix.root, false).run().unwrap();
        let paths = sorted_paths(results);
        // c, d, e must not appear.
        assert!(!paths.contains(&fix.c));
        assert!(!paths.contains(&fix.d));
        assert!(!paths.contains(&fix.e));
    }

    // ── path_in_dir, recursive ─────────────────────────────────────────────

    #[test]
    fn path_in_dir_recursive_returns_full_subtree() {
        let (fix, engine) = make_fixture();
        let results = engine.query().path_in_dir(&fix.root, true).run().unwrap();
        let paths = sorted_paths(results);
        // All five entries are under root.
        assert_eq!(paths.len(), 5);
        for p in [&fix.a, &fix.b, &fix.c, &fix.d, &fix.e] {
            assert!(paths.contains(p), "expected {}", p.display());
        }
    }

    #[test]
    fn path_in_dir_recursive_sub_returns_sub_and_deeper() {
        let (fix, engine) = make_fixture();
        let sub = fix.root.join("sub");
        let results = engine.query().path_in_dir(&sub, true).run().unwrap();
        let paths = sorted_paths(results);
        // c.txt (in sub) and d.txt (in sub/sub2) included; a, b, e excluded.
        assert_eq!(paths.len(), 2);
        assert!(paths.contains(&fix.c));
        assert!(paths.contains(&fix.d));
        assert!(!paths.contains(&fix.a));
    }

    // ── path_in_dir, special characters ───────────────────────────────────

    #[test]
    fn path_in_dir_handles_percent_in_directory_name() {
        let dir = TempDir::new().unwrap();
        let special = dir.path().join("100%_done");
        fs::create_dir(&special).unwrap();
        let f = write_file_at(&special, "x.txt", b"x");

        let engine: CacheEngine<Vec<f32>> = CacheEngine::builder()
            .database(dir.path().join("pct.sqlite3"))
            .build()
            .unwrap();
        // Also cache a file whose path does NOT contain "100%_done".
        let other = write_file_at(dir.path(), "other.txt", b"o");
        engine.set(&f, &vec![1.0]).unwrap();
        engine.set(&other, &vec![2.0]).unwrap();

        let results = engine.query().path_in_dir(&special, true).run().unwrap();
        let paths = sorted_paths(results);
        // The % and _ must be matched literally, not as LIKE wildcards.
        assert_eq!(paths, vec![f]);
    }

    // ── path_in_dir, nonexistent directory ────────────────────────────────

    #[test]
    fn path_in_dir_nonexistent_returns_empty() {
        let dir = TempDir::new().unwrap();
        let engine: CacheEngine<Vec<f32>> =
            CacheEngine::builder().database(":memory:").build().unwrap();
        // Engine is empty; the dir doesn't exist either.
        let ghost = dir.path().join("does_not_exist");
        let results = engine.query().path_in_dir(&ghost, true).run().unwrap();
        assert!(results.is_empty());
    }

    // ── path_glob ─────────────────────────────────────────────────────────

    #[test]
    fn path_glob_star_matches_all_txt() {
        let (_fix, engine) = make_fixture();
        let results = engine.query().path_glob("*.txt").run().unwrap();
        assert_eq!(results.len(), 5, "all five .txt files should match");
    }

    #[test]
    fn path_glob_question_mark_matches_single_char() {
        let (_fix, engine) = make_fixture();
        // "?.txt" matches a single-char stem — a.txt and b.txt (and c, d, e too
        // since they also have single-char stems).
        let results = engine.query().path_glob("*/?.txt").run().unwrap();
        assert_eq!(results.len(), 5);
    }

    #[test]
    fn path_glob_brace_alternation() {
        let (fix, engine) = make_fixture();
        // Match only a.txt and b.txt via brace alternation.
        let root_str = fix.root.to_string_lossy();
        let pattern = format!("{root_str}/{{a,b}}.txt");
        let results = engine.query().path_glob(&pattern).run().unwrap();
        let paths = sorted_paths(results);
        assert_eq!(paths, vec![fix.a.clone(), fix.b.clone()]);
    }

    #[test]
    fn path_glob_nested_brace_expansion() {
        let (_fix, engine) = make_fixture();
        // {a,{b,c}} expands to a, b, c — should match a.txt, b.txt, c.txt.
        let pattern = "*/{a,{b,c}}.txt".to_string();
        let results = engine.query().path_glob(&pattern).run().unwrap();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn path_glob_literal_bracket_matches_file_with_bracket_in_name() {
        let dir = TempDir::new().unwrap();
        let f = write_file_at(dir.path(), "[special].txt", b"s");
        let engine: CacheEngine<Vec<f32>> =
            CacheEngine::builder().database(":memory:").build().unwrap();
        engine.set(&f, &vec![9.0]).unwrap();

        // User writes the pattern with a literal `[`; our translation converts
        // `[` → `[[]` so SQLite GLOB treats it as a character class matching `[`.
        // Pattern "*/[special].txt" → after translation → "*/[[]special].txt"
        // which matches the file named "[special].txt".
        let sep = std::path::MAIN_SEPARATOR;
        let pattern = format!("*{sep}[special].txt");
        let results = engine.query().path_glob(&pattern).run().unwrap();
        assert_eq!(results.len(), 1, "literal [ should match the file");
    }

    // ── Combination: path_in_dir + path_glob ──────────────────────────────

    #[test]
    fn path_in_dir_and_path_glob_combine() {
        let (fix, engine) = make_fixture();
        // Within root/sub (recursive) AND glob matching only c.txt.
        let sub = fix.root.join("sub");
        let results = engine
            .query()
            .path_in_dir(&sub, true)
            .path_glob("*/c.txt")
            .run()
            .unwrap();
        let paths = sorted_paths(results);
        assert_eq!(paths, vec![fix.c.clone()]);
    }

    // ── dry_run reflects new predicates ───────────────────────────────────

    #[test]
    fn dry_run_with_path_in_dir() {
        let (fix, engine) = make_fixture();
        let plan = engine
            .query()
            .path_in_dir(&fix.root, false)
            .dry_run()
            .unwrap();
        // Plan must be non-empty — the LIKE clause shows up in the scan.
        assert!(!plan.is_empty(), "dry_run must return a plan");
    }

    #[test]
    fn dry_run_with_path_glob() {
        let (_fix, engine) = make_fixture();
        let plan = engine.query().path_glob("*.txt").dry_run().unwrap();
        assert!(!plan.is_empty());
    }

    // ── Equivalence test: path_in_dir(dir, false) vs LIKE + parent filter ─

    #[test]
    fn path_in_dir_non_recursive_equivalent_to_like_plus_parent_filter() {
        let (fix, engine) = make_fixture();

        // Reference: path_like prefix + Rust-side parent() equality.
        let root_str = fix.root.to_string_lossy();
        let pattern = format!("{}{}%", root_str, std::path::MAIN_SEPARATOR);
        let all_under: Vec<_> = engine
            .query()
            .path_like(&pattern)
            .run()
            .unwrap()
            .into_iter()
            .filter(|e| e.path.parent() == Some(&fix.root))
            .map(|e| e.path)
            .collect();

        // RFC 006 path: SQL-native, no post-filter.
        let native: Vec<_> =
            sorted_paths(engine.query().path_in_dir(&fix.root, false).run().unwrap());

        let mut reference = all_under;
        reference.sort();
        assert_eq!(
            native, reference,
            "SQL-native and post-filter results must agree"
        );
    }
}
