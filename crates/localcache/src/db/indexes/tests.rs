use std::panic::AssertUnwindSafe;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use super::*;
use crate::CacheEngine;
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
         CREATE INDEX temp.lc_user_rollback_drop ON files(path);
         CREATE INDEX temp.lc_user_panic_drop ON files(path);
         ATTACH ':memory:' AS other;
         CREATE TABLE other.files(namespace TEXT, path TEXT, marker TEXT);
         INSERT INTO other.files VALUES ('other-ns', '/other-row', 'ATTACHED');
         CREATE INDEX other.lc_user_rollback_drop ON files(namespace DESC);
         CREATE INDEX other.lc_user_panic_drop ON files(namespace DESC);",
    )
    .unwrap();
}

/// A legacy `lc_user_<suffix>` index, as an earlier release made it, created
/// by raw SQL on a test-owned connection.
fn legacy_index(conn: &Connection, suffix: &str) -> String {
    let full = format!("{PUBLIC_INDEX_PREFIX}{suffix}");
    conn.execute(
        &format!(
            "CREATE INDEX main.{} ON files(namespace, path)",
            quote_identifier(&full).as_sql()
        ),
        [],
    )
    .unwrap();
    full
}

fn main_index_exists(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT count(*) FROM main.sqlite_schema WHERE type = 'index' AND name = ?1",
        [name],
        |row| row.get::<_, i64>(0),
    )
    .unwrap()
        == 1
}

#[test]
fn identifier_equality_and_quoting_are_exact() {
    assert!(identifier_eq("lc_user_Mixed", "LC_USER_mIXED"));
    assert!(!identifier_eq("lc_user_é", "lc_user_É"));
    assert_eq!(quote_identifier("a\"b").as_sql(), "\"a\"\"b\"");
}

#[test]
fn drop_errors_after_ddl_roll_back() {
    let engine = engine();
    install_semantic_fixture(&engine.conn);
    legacy_index(&engine.conn, "rollback_drop");
    let before_drop = semantic_snapshot(&engine.conn);
    let hook = set_hook(|point| {
        if point == TestPoint::DropDdl {
            return Err(LocalFileCacheError::DatabaseState("after drop".into()));
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
    legacy_index(&engine.conn, "panic_drop");
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
    legacy_index(&engine.conn, "nested");
    let outer = engine.conn.unchecked_transaction().unwrap();
    assert!(drop_path_index(&engine.conn, "nested").is_err());
    outer.rollback().unwrap();
    assert_eq!(list_path_indexes(&engine.conn).unwrap(), ["lc_user_nested"]);
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

    assert_eq!(legacy_index(&engine.conn, "collision"), "lc_user_collision");
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
    assert!(
        engine.query().run().unwrap().is_empty(),
        "query read a non-main collision row"
    );
    engine.query().dry_run().unwrap();

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
    assert_eq!(semantic_snapshot(&engine.conn), collisions_without_main);

    assert_eq!(legacy_index(&engine.conn, "collision"), "lc_user_collision");
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
    assert!(
        matches!(
            error,
            LocalFileCacheError::InvalidArgument {
                argument: "index_name",
                ..
            }
        ),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "invalid argument: index_name: not a valid SQLite identifier or not an allowed localcache index"
    );
}

/// What `drop_path_index` will and will not touch: only a structurally owned
/// `lc_user_` index on `files(namespace, path)` is ever dropped.
#[test]
fn drop_rejects_every_non_owned_shape_and_leaves_it_in_place() {
    let engine = engine();
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

    // Owned-prefix names with a wrong shape are an error, never a drop.
    for suffix in [
        "unique",
        "partial",
        "desc",
        "collation",
        "expression",
        "extra",
        "wrong_table",
    ] {
        let error = drop_path_index(&engine.conn, suffix).unwrap_err();
        assert!(
            matches!(
                error,
                LocalFileCacheError::InvalidArgument {
                    argument: "index_name",
                    ..
                }
            ),
            "dropped or mis-reported {suffix}: {error:?}"
        );
        assert!(
            main_index_exists(&engine.conn, &format!("lc_user_{suffix}")),
            "{suffix} was dropped"
        );
    }

    // A name without the prefix is not an `lc_user_` index at all: the suffix
    // form cannot even address it, and the object stays.
    let autoindex: String = engine
        .conn
        .query_row(
            "SELECT name FROM pragma_index_list('files', 'main') WHERE origin = 'u' AND name LIKE 'sqlite_autoindex%' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    for name in [
        "arbitrary_ok_shape",
        "lcXuser_lookalike",
        autoindex.as_str(),
    ] {
        assert!(!drop_path_index(&engine.conn, name).unwrap());
        assert!(main_index_exists(&engine.conn, name), "{name} was dropped");
    }
}

#[test]
fn ascii_case_equivalent_drop_finds_the_catalog_spelling() {
    let engine = engine();
    assert_eq!(legacy_index(&engine.conn, "MixedCase"), "lc_user_MixedCase");
    assert_eq!(
        list_path_indexes(&engine.conn).unwrap(),
        ["lc_user_MixedCase"]
    );
    assert!(drop_path_index(&engine.conn, "mixedcase").unwrap());
    assert!(list_path_indexes(&engine.conn).unwrap().is_empty());
}

/// An out-of-grammar legacy spelling (which the narrowed grammar of v0.21 could
/// no longer create, but an older release could have) is listed and can be
/// dropped.
#[test]
fn an_out_of_grammar_legacy_spelling_can_be_listed_and_dropped() {
    let engine = engine();
    let full = legacy_index(&engine.conn, "éclair$");
    assert_eq!(full, "lc_user_éclair$");
    assert_eq!(list_path_indexes(&engine.conn).unwrap(), [full.as_str()]);
    assert!(drop_path_index(&engine.conn, "éclair$").unwrap());
    assert!(!main_index_exists(&engine.conn, &full));
}

#[test]
fn list_uses_one_generation_and_later_operations_revalidate() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("list-race.sqlite3");
    let engine = CacheEngine::<Vec<f32>>::builder()
        .database(&database)
        .build()
        .unwrap();
    legacy_index(&engine.conn, "list_race");

    let changed = Arc::new(AtomicBool::new(false));
    let changed_in_hook = Arc::clone(&changed);
    let database_in_hook = database.clone();
    let hook = set_hook(move |point| {
        if point == TestPoint::SchemaRead && !changed_in_hook.swap(true, Ordering::SeqCst) {
            let writer = Connection::open(&database_in_hook).db()?;
            writer
                .busy_timeout(std::time::Duration::from_secs(1))
                .db()?;
            writer
                .execute_batch(
                    "DROP INDEX main.lc_user_list_race;
                 CREATE INDEX main.lc_user_list_race ON files(path);",
                )
                .db()?;
        }
        Ok(())
    });
    let listed = list_path_indexes(&engine.conn);
    drop(hook);

    assert!(changed.load(Ordering::SeqCst));
    assert_eq!(listed.unwrap(), ["lc_user_list_race"]);
    // The index was replaced by one of the wrong shape: the next operation
    // revalidates it and refuses.
    assert!(drop_path_index(&engine.conn, "list_race").is_err());
    assert!(main_index_exists(&engine.conn, "lc_user_list_race"));
}
