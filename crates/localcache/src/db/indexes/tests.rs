use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use super::*;
use crate::CacheEngine;
use crate::db::repository;
use tempfile::TempDir;

fn engine() -> CacheEngine<Vec<f32>> {
    CacheEngine::builder().database(":memory:").build().unwrap()
}

struct HookGuard;

impl Drop for HookGuard {
    fn drop(&mut self) {
        TEST_HOOK.with(|slot| *slot.borrow_mut() = None);
    }
}

fn set_hook(hook: impl FnMut(TestPoint) -> Result<(), LocalFileCacheError> + 'static) -> HookGuard {
    TEST_HOOK.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
    HookGuard
}

#[derive(Debug, PartialEq, Eq)]
struct SnapshotFileRow {
    id: i64,
    namespace: String,
    path: String,
    mtime: i64,
    file_size: i64,
    hash: Option<String>,
    updated_at: i64,
    payload_version: i64,
    last_accessed_at: i64,
}

#[derive(Debug, PartialEq, Eq)]
struct SemanticSnapshot {
    main_schema: Vec<SchemaObject>,
    main_indexes: Vec<(IndexListRow, Vec<IndexXinfoRow>)>,
    user_version: i64,
    files: Vec<SnapshotFileRow>,
    payloads: Vec<(i64, Vec<u8>, String)>,
    temp_schema: Vec<(String, String, String, Option<String>)>,
    attached_schema: Vec<(String, String, String, Option<String>)>,
    temp_rows: Vec<(String, String, String)>,
    attached_rows: Vec<(String, String, String)>,
}

fn semantic_snapshot(conn: &Connection) -> SemanticSnapshot {
    let main_schema = main_schema_objects(conn).unwrap();
    let main_indexes = main_file_indexes(conn)
        .unwrap()
        .into_iter()
        .map(|index| {
            let terms = main_index_xinfo(conn, &index.name).unwrap();
            (index, terms)
        })
        .collect();
    let user_version = conn
        .query_row("PRAGMA main.user_version", [], |row| row.get(0))
        .unwrap();
    let files = conn
        .prepare(
            "SELECT id, namespace, path, mtime, file_size, hash, updated_at,
                    payload_version, last_accessed_at FROM main.files ORDER BY id",
        )
        .unwrap()
        .query_map([], |row| {
            Ok(SnapshotFileRow {
                id: row.get(0)?,
                namespace: row.get(1)?,
                path: row.get(2)?,
                mtime: row.get(3)?,
                file_size: row.get(4)?,
                hash: row.get(5)?,
                updated_at: row.get(6)?,
                payload_version: row.get(7)?,
                last_accessed_at: row.get(8)?,
            })
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let payloads = conn
        .prepare("SELECT file_id, content, encoding FROM main.payloads ORDER BY file_id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let schema = |name: &str| {
        let sql = format!(
            "SELECT type, name, tbl_name, sql FROM {name}.sqlite_schema ORDER BY type, name"
        );
        conn.prepare(&sql)
            .unwrap()
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    let collision_rows = |name: &str| {
        let sql = format!("SELECT namespace, path, marker FROM {name}.files ORDER BY path");
        conn.prepare(&sql)
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    SemanticSnapshot {
        main_schema,
        main_indexes,
        user_version,
        files,
        payloads,
        temp_schema: schema("temp"),
        attached_schema: schema("other"),
        temp_rows: collision_rows("temp"),
        attached_rows: collision_rows("other"),
    }
}

fn install_semantic_fixture(conn: &Connection) {
    conn.execute_batch(
        "INSERT INTO main.files
            (id, namespace, path, mtime, file_size, hash, updated_at,
             payload_version, last_accessed_at)
         VALUES (7, 'default', '/main-row', 11, 12, 'main-hash', 13, 14, 15);
         INSERT INTO main.payloads(file_id, content, encoding)
         VALUES (7, X'0001FEFF', 'fixture');
         CREATE TEMP TABLE files(namespace TEXT, path TEXT, marker TEXT);
         INSERT INTO temp.files VALUES ('temp-ns', '/temp-row', 'TEMP');
         CREATE INDEX temp.lc_user_rollback_create ON files(path);
         CREATE INDEX temp.lc_user_rollback_drop ON files(path);
         CREATE INDEX temp.lc_user_panic_drop ON files(path);
         ATTACH ':memory:' AS other;
         CREATE TABLE other.files(namespace TEXT, path TEXT, marker TEXT);
         INSERT INTO other.files VALUES ('other-ns', '/other-row', 'ATTACHED');
         CREATE INDEX other.lc_user_rollback_create ON files(namespace DESC);
         CREATE INDEX other.lc_user_rollback_drop ON files(namespace DESC);
         CREATE INDEX other.lc_user_panic_drop ON files(namespace DESC);",
    )
    .unwrap();
}

#[test]
fn grammar_equivalence_and_quoting_are_exact() {
    for valid in ["a", "_", "9", "Mixed_Case_9", "select"] {
        validate_new_suffix(valid).unwrap();
    }
    validate_new_suffix(&"a".repeat(64)).unwrap();
    for invalid in ["", "éclair", "dollar$sign", "has space", "x;--"] {
        assert!(
            validate_new_suffix(invalid).is_err(),
            "accepted {invalid:?}"
        );
    }
    assert!(validate_new_suffix(&"a".repeat(65)).is_err());
    assert!(identifier_eq("lc_user_Mixed", "LC_USER_mIXED"));
    assert!(!identifier_eq("lc_user_é", "lc_user_É"));
    assert_eq!(quote_identifier("a\"b").as_sql(), "\"a\"\"b\"");
}

#[test]
fn create_and_drop_errors_after_ddl_roll_back() {
    let engine = engine();
    install_semantic_fixture(&engine.conn);
    let before_create = semantic_snapshot(&engine.conn);
    let hook = set_hook(|point| {
        if point == TestPoint::CreateDdl {
            return Err(LocalFileCacheError::UnsupportedFeature(
                "after create".into(),
            ));
        }
        Ok(())
    });
    assert!(create_path_index(&engine.conn, "rollback_create").is_err());
    drop(hook);
    assert_eq!(semantic_snapshot(&engine.conn), before_create);
    assert_eq!(
        create_path_index(&engine.conn, "rollback_create").unwrap(),
        "lc_user_rollback_create"
    );
    assert!(drop_path_index(&engine.conn, "rollback_create").unwrap());

    create_path_index(&engine.conn, "rollback_drop").unwrap();
    let before_drop = semantic_snapshot(&engine.conn);
    let hook = set_hook(|point| {
        if point == TestPoint::DropDdl {
            return Err(LocalFileCacheError::UnsupportedFeature("after drop".into()));
        }
        Ok(())
    });
    assert!(drop_path_index(&engine.conn, "rollback_drop").is_err());
    drop(hook);
    assert_eq!(semantic_snapshot(&engine.conn), before_drop);
    assert!(drop_path_index(&engine.conn, "rollback_drop").unwrap());
}

#[test]
fn panic_after_drop_ddl_rolls_back() {
    let engine = engine();
    install_semantic_fixture(&engine.conn);
    create_path_index(&engine.conn, "panic_drop").unwrap();
    let before = semantic_snapshot(&engine.conn);
    let hook = set_hook(|point| {
        if point == TestPoint::DropDdl {
            panic!("synthetic post-drop panic");
        }
        Ok(())
    });
    let unwind = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let _ = drop_path_index(&engine.conn, "panic_drop");
    }));
    drop(hook);
    assert!(unwind.is_err());
    assert_eq!(semantic_snapshot(&engine.conn), before);
    assert!(drop_path_index(&engine.conn, "panic_drop").unwrap());
}

#[test]
fn nested_transaction_fails_before_mutation() {
    let engine = engine();
    let outer = engine.conn.unchecked_transaction().unwrap();
    assert!(create_path_index(&engine.conn, "nested").is_err());
    outer.rollback().unwrap();
    assert!(list_path_indexes(&engine.conn).unwrap().is_empty());
}

#[test]
fn main_schema_wins_over_temp_and_attached_collisions() {
    let engine = engine();
    engine
        .conn
        .execute_batch(
            "CREATE TEMP TABLE files(namespace TEXT, path TEXT, marker TEXT);
             INSERT INTO temp.files VALUES ('default', '/temp-only', 'TEMP');
             CREATE INDEX temp.lc_user_collision ON files(path);
             ATTACH ':memory:' AS other;
             CREATE TABLE other.files(namespace TEXT, path TEXT, marker TEXT);
             INSERT INTO other.files VALUES ('default', '/attached-only', 'ATTACHED');
             CREATE INDEX other.lc_user_collision ON files(namespace DESC);",
        )
        .unwrap();
    let collisions_without_main = semantic_snapshot(&engine.conn);

    assert_eq!(
        create_path_index(&engine.conn, "collision").unwrap(),
        "lc_user_collision"
    );
    assert_eq!(
        list_path_indexes(&engine.conn).unwrap(),
        ["lc_user_collision"]
    );
    let main_row = main_file_indexes(&engine.conn)
        .unwrap()
        .into_iter()
        .find(|row| row.name == "lc_user_collision")
        .unwrap();
    assert_eq!(
        main_row,
        IndexListRow {
            name: "lc_user_collision".into(),
            unique: false,
            origin: "c".into(),
            partial: false,
        }
    );
    validate_index_terms(
        &main_index_xinfo(&engine.conn, "lc_user_collision").unwrap(),
        &["namespace", "path"],
    )
    .unwrap();
    let temp_term: (String, i64) = engine
        .conn
        .query_row(
            "SELECT name, \"desc\" FROM pragma_index_xinfo('lc_user_collision', 'temp')
             WHERE key = 1 ORDER BY seqno LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let attached_term: (String, i64) = engine
        .conn
        .query_row(
            "SELECT name, \"desc\" FROM pragma_index_xinfo('lc_user_collision', 'other')
             WHERE key = 1 ORDER BY seqno LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(temp_term, ("path".into(), 0));
    assert_eq!(attached_term, ("namespace".into(), 1));
    assert_eq!(
        engine
            .conn
            .query_row("SELECT path, marker FROM temp.files", [], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap(),
        ("/temp-only".into(), "TEMP".into())
    );
    assert_eq!(
        engine
            .conn
            .query_row("SELECT path, marker FROM other.files", [], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap(),
        ("/attached-only".into(), "ATTACHED".into())
    );
    assert!(authorize_query_index(&engine.conn, Some("lc_user_collision")).is_ok());
    assert!(
        repository::keys(
            &engine.conn,
            "default",
            None,
            Some("lc_user_collision"),
            None,
            None,
        )
        .unwrap()
        .is_empty()
    );
    for hint in [
        None,
        Some("lc_user_collision"),
        Some("idx_files_namespace_path"),
    ] {
        let query = engine.query();
        let result = match hint {
            Some(name) => query.index_hint(name).run(),
            None => query.run(),
        }
        .unwrap();
        assert!(result.is_empty(), "query read a non-main collision row");

        let query = engine.query();
        let plan = match hint {
            Some(name) => query.index_hint(name).dry_run(),
            None => query.dry_run(),
        }
        .unwrap();
        if let Some(name) = hint {
            assert!(plan.contains(name), "plan did not require {name}: {plan}");
        }
    }

    assert!(drop_path_index(&engine.conn, "collision").unwrap());
    for schema in ["temp", "other"] {
        let sql = format!(
            "SELECT count(*) FROM {schema}.sqlite_schema WHERE type = 'index' AND name = 'lc_user_collision'"
        );
        assert_eq!(
            engine
                .conn
                .query_row(&sql, [], |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
    assert!(!drop_path_index(&engine.conn, "collision").unwrap());
    assert!(authorize_query_index(&engine.conn, Some("lc_user_collision")).is_err());
    assert!(
        engine
            .query()
            .index_hint("lc_user_collision")
            .run()
            .is_err()
    );
    assert!(
        engine
            .query()
            .index_hint("lc_user_collision")
            .dry_run()
            .is_err()
    );
    assert_eq!(semantic_snapshot(&engine.conn), collisions_without_main);

    assert_eq!(
        create_path_index(&engine.conn, "collision").unwrap(),
        "lc_user_collision"
    );
    assert_eq!(
        list_path_indexes(&engine.conn).unwrap(),
        ["lc_user_collision"]
    );
}

#[test]
fn list_fails_closed_on_an_invalid_owned_prefix_candidate() {
    let engine = engine();
    engine
        .conn
        .execute("CREATE INDEX lc_user_wrong ON files(path)", [])
        .unwrap();
    let error = list_path_indexes(&engine.conn).unwrap_err();
    assert_eq!(
        error.to_string(),
        "unsupported feature: SQLite index identifier is invalid or is not an allowed localcache index"
    );
}

#[test]
fn query_allowlist_rejects_every_non_owned_shape() {
    let engine = engine();
    assert!(authorize_query_index(&engine.conn, Some("idx_files_namespace_path")).is_ok());
    assert!(authorize_query_index(&engine.conn, Some("idx_files_lru")).is_ok());

    engine
        .conn
        .execute_batch(
            "CREATE INDEX arbitrary_ok_shape ON files(namespace, path);
             CREATE UNIQUE INDEX lc_user_unique ON files(namespace, path);
             CREATE INDEX lc_user_partial ON files(namespace, path) WHERE path <> '';
             CREATE INDEX lc_user_desc ON files(namespace DESC, path);
             CREATE INDEX lc_user_collation ON files(namespace COLLATE NOCASE, path);
             CREATE INDEX lc_user_expression ON files(lower(namespace), path);
             CREATE INDEX lc_user_extra ON files(namespace, path, mtime);
             CREATE INDEX lcXuser_lookalike ON files(namespace, path);
             CREATE INDEX lc_user_wrong_table ON payloads(file_id);",
        )
        .unwrap();

    let autoindex: String = engine
        .conn
        .query_row(
            "SELECT name FROM pragma_index_list('files', 'main') WHERE origin = 'u' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    for name in [
        "arbitrary_ok_shape",
        "lc_user_unique",
        "lc_user_partial",
        "lc_user_desc",
        "lc_user_collation",
        "lc_user_expression",
        "lc_user_extra",
        "lcXuser_lookalike",
        "lc_user_wrong_table",
        autoindex.as_str(),
    ] {
        assert!(
            authorize_query_index(&engine.conn, Some(name)).is_err(),
            "authorized {name}"
        );
    }
}

#[test]
fn ascii_case_equivalent_create_returns_catalog_spelling() {
    let engine = engine();
    assert_eq!(
        create_path_index(&engine.conn, "MixedCase").unwrap(),
        "lc_user_MixedCase"
    );
    assert_eq!(
        create_path_index(&engine.conn, "mixedcase").unwrap(),
        "lc_user_MixedCase"
    );
    assert_eq!(
        list_path_indexes(&engine.conn).unwrap(),
        ["lc_user_MixedCase"]
    );
}

#[test]
fn authorization_metadata_is_read_from_one_snapshot() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("snapshot.sqlite3");
    let engine = CacheEngine::<Vec<f32>>::builder()
        .database(&database)
        .build()
        .unwrap();
    create_path_index(&engine.conn, "snapshot").unwrap();

    let changed = Arc::new(AtomicBool::new(false));
    let changed_in_hook = Arc::clone(&changed);
    let database_in_hook: PathBuf = database.clone();
    let hook = set_hook(move |point| {
        if point == TestPoint::SchemaRead && !changed_in_hook.swap(true, Ordering::SeqCst) {
            let writer = Connection::open(&database_in_hook)?;
            writer.busy_timeout(std::time::Duration::from_secs(1))?;
            writer.execute_batch(
                "DROP INDEX main.lc_user_snapshot;
                 CREATE INDEX main.lc_user_snapshot ON files(path);",
            )?;
        }
        Ok(())
    });
    let result = authorize_query_index(&engine.conn, Some("lc_user_snapshot"));
    drop(hook);

    assert!(changed.load(Ordering::SeqCst));
    assert!(
        result.is_ok(),
        "mixed schema generations were observed: {result:?}"
    );
    assert!(authorize_query_index(&engine.conn, Some("lc_user_snapshot")).is_err());
}

#[test]
fn query_execution_stays_in_the_authorization_snapshot() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("query-race.sqlite3");
    let engine = CacheEngine::<Vec<f32>>::builder()
        .database(&database)
        .build()
        .unwrap();
    create_path_index(&engine.conn, "race").unwrap();

    let changed = Arc::new(AtomicBool::new(false));
    let changed_in_hook = Arc::clone(&changed);
    let database_in_hook = database.clone();
    let hook = set_hook(move |point| {
        if point == TestPoint::Authorization && !changed_in_hook.swap(true, Ordering::SeqCst) {
            let writer = Connection::open(&database_in_hook)?;
            writer.busy_timeout(std::time::Duration::from_secs(1))?;
            writer.execute_batch(
                "DROP INDEX main.lc_user_race;
                 CREATE INDEX main.lc_user_race ON files(path);",
            )?;
        }
        Ok(())
    });
    let result = engine.query().index_hint("lc_user_race").run();
    drop(hook);

    assert!(changed.load(Ordering::SeqCst));
    assert!(result.unwrap().is_empty());
    assert!(authorize_query_index(&engine.conn, Some("lc_user_race")).is_err());
}

#[test]
fn list_uses_one_generation_and_later_operations_revalidate() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("list-race.sqlite3");
    let engine = CacheEngine::<Vec<f32>>::builder()
        .database(&database)
        .build()
        .unwrap();
    create_path_index(&engine.conn, "list_race").unwrap();

    let changed = Arc::new(AtomicBool::new(false));
    let changed_in_hook = Arc::clone(&changed);
    let database_in_hook = database.clone();
    let hook = set_hook(move |point| {
        if point == TestPoint::SchemaRead && !changed_in_hook.swap(true, Ordering::SeqCst) {
            let writer = Connection::open(&database_in_hook)?;
            writer.busy_timeout(std::time::Duration::from_secs(1))?;
            writer.execute_batch(
                "DROP INDEX main.lc_user_list_race;
                 CREATE INDEX main.lc_user_list_race ON files(path);",
            )?;
        }
        Ok(())
    });
    let listed = list_path_indexes(&engine.conn);
    drop(hook);

    assert!(changed.load(Ordering::SeqCst));
    assert_eq!(listed.unwrap(), ["lc_user_list_race"]);
    assert!(authorize_query_index(&engine.conn, Some("lc_user_list_race")).is_err());
}

#[test]
fn dry_run_stays_in_the_authorization_snapshot() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("dry-run-race.sqlite3");
    let engine = CacheEngine::<Vec<f32>>::builder()
        .database(&database)
        .build()
        .unwrap();
    create_path_index(&engine.conn, "dry_race").unwrap();

    let changed = Arc::new(AtomicBool::new(false));
    let changed_in_hook = Arc::clone(&changed);
    let database_in_hook = database.clone();
    let hook = set_hook(move |point| {
        if point == TestPoint::Authorization && !changed_in_hook.swap(true, Ordering::SeqCst) {
            let writer = Connection::open(&database_in_hook)?;
            writer.busy_timeout(std::time::Duration::from_secs(1))?;
            writer.execute_batch(
                "DROP INDEX main.lc_user_dry_race;
                 CREATE INDEX main.lc_user_dry_race ON files(path);",
            )?;
        }
        Ok(())
    });
    let result = engine.query().index_hint("lc_user_dry_race").dry_run();
    drop(hook);

    assert!(changed.load(Ordering::SeqCst));
    let plan = result.unwrap();
    assert!(plan.contains("lc_user_dry_race"));
    assert!(authorize_query_index(&engine.conn, Some("lc_user_dry_race")).is_err());
}
