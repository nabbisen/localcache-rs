//! Integration tests — query.

mod common;
use common::write_file;

use tempfile::TempDir;

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
}

// ====================================================================
// Phase 12 —
