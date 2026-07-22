//! Safe SQLite index identifier and ownership boundary (RFC 011).

use rusqlite::{Connection, TransactionBehavior};

use crate::error::LocalFileCacheError;

pub(crate) const PUBLIC_INDEX_PREFIX: &str = "lc_user_";
const IDENTIFIER_ERROR: &str =
    "SQLite index identifier is invalid or is not an allowed localcache index";

#[derive(Debug, Clone)]
pub(crate) struct IndexListRow {
    pub(crate) name: String,
    pub(crate) unique: bool,
    pub(crate) origin: String,
    pub(crate) partial: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct IndexXinfoRow {
    pub(crate) cid: i64,
    pub(crate) name: Option<String>,
    pub(crate) descending: bool,
    pub(crate) collation: Option<String>,
    pub(crate) key: bool,
}

#[derive(Debug, Clone)]
struct SchemaObject {
    object_type: String,
    name: String,
    table_name: String,
    sql: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct QuotedIdentifier(String);

impl QuotedIdentifier {
    pub(crate) fn as_sql(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestPoint {
    SchemaRead,
    Authorization,
    CreateDdl,
    DropDdl,
}

#[cfg(test)]
type TestHook = Box<dyn FnMut(TestPoint) -> Result<(), LocalFileCacheError>>;

#[cfg(test)]
thread_local! {
    static TEST_HOOK: std::cell::RefCell<Option<TestHook>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
#[inline]
fn test_hook(point: TestPoint) -> Result<(), LocalFileCacheError> {
    TEST_HOOK.with(|slot| {
        let mut hook = slot.borrow_mut();
        if let Some(hook) = hook.as_mut() {
            hook(point)?;
        }
        Ok(())
    })
}

#[cfg(not(test))]
#[inline]
fn test_hook(_point: TestPoint) -> Result<(), LocalFileCacheError> {
    Ok(())
}

pub(crate) fn create_path_index(
    conn: &Connection,
    suffix: &str,
) -> Result<String, LocalFileCacheError> {
    let full = format!("{PUBLIC_INDEX_PREFIX}{suffix}");
    let transaction = rusqlite::Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;

    if let Some(object) = resolve_schema_object(&transaction, &full)? {
        let actual = validate_owned_path_index(&transaction, &object)?;
        transaction.commit()?;
        return Ok(actual);
    }

    validate_new_suffix(suffix)?;
    let quoted = quote_identifier(&full);
    let sql = format!(
        "CREATE INDEX main.{} ON files(namespace, path)",
        quoted.as_sql()
    );
    transaction.execute(&sql, [])?;
    test_hook(TestPoint::CreateDdl)?;

    let object = resolve_schema_object(&transaction, &full)?.ok_or_else(identifier_error)?;
    let actual = validate_owned_path_index(&transaction, &object)?;
    transaction.commit()?;
    Ok(actual)
}

pub(crate) fn drop_path_index(
    conn: &Connection,
    suffix: &str,
) -> Result<bool, LocalFileCacheError> {
    let full = format!("{PUBLIC_INDEX_PREFIX}{suffix}");
    let transaction = rusqlite::Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;

    let Some(object) = resolve_schema_object(&transaction, &full)? else {
        transaction.commit()?;
        return Ok(false);
    };
    validate_owned_path_index(&transaction, &object)?;

    let quoted = quote_identifier(&full);
    let sql = format!("DROP INDEX main.{}", quoted.as_sql());
    transaction.execute(&sql, [])?;
    test_hook(TestPoint::DropDdl)?;
    if resolve_schema_object(&transaction, &full)?.is_some() {
        return Err(identifier_error());
    }
    transaction.commit()?;
    Ok(true)
}

pub(crate) fn list_path_indexes(conn: &Connection) -> Result<Vec<String>, LocalFileCacheError> {
    let transaction = rusqlite::Transaction::new_unchecked(conn, TransactionBehavior::Deferred)?;
    let objects = public_schema_objects(&transaction)?;
    let mut names = Vec::with_capacity(objects.len());
    for object in &objects {
        names.push(validate_owned_path_index(&transaction, object)?);
    }
    names.sort();
    transaction.commit()?;
    Ok(names)
}

#[cfg(test)]
pub(crate) fn authorize_query_index(
    conn: &Connection,
    name: Option<&str>,
) -> Result<Option<QuotedIdentifier>, LocalFileCacheError> {
    let transaction = rusqlite::Transaction::new_unchecked(conn, TransactionBehavior::Deferred)?;
    let result = authorize_query_index_in_snapshot(&transaction, name)?;
    transaction.commit()?;
    Ok(result)
}

pub(crate) fn authorize_query_index_in_snapshot(
    conn: &Connection,
    name: Option<&str>,
) -> Result<Option<QuotedIdentifier>, LocalFileCacheError> {
    let Some(name) = name else {
        return Ok(None);
    };
    let object = resolve_schema_object(conn, name)?.ok_or_else(identifier_error)?;

    match object.name.as_str() {
        "idx_files_namespace_path" => {
            validate_builtin_index(conn, &object, &["namespace", "path"])?;
        }
        "idx_files_lru" => {
            validate_builtin_index(
                conn,
                &object,
                &["namespace", "last_accessed_at", "updated_at"],
            )?;
        }
        _ => {
            validate_owned_path_index(conn, &object)?;
        }
    }

    let quoted = quote_identifier(name);
    test_hook(TestPoint::Authorization)?;
    Ok(Some(quoted))
}

pub(crate) fn main_file_indexes(
    conn: &Connection,
) -> Result<Vec<IndexListRow>, LocalFileCacheError> {
    let mut statement = conn.prepare(
        "SELECT name, \"unique\", origin, partial
         FROM pragma_index_list('files', 'main')
         ORDER BY name",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(IndexListRow {
            name: row.get(0)?,
            unique: row.get::<_, i64>(1)? != 0,
            origin: row.get(2)?,
            partial: row.get::<_, i64>(3)? != 0,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub(crate) fn main_index_xinfo(
    conn: &Connection,
    index_name: &str,
) -> Result<Vec<IndexXinfoRow>, LocalFileCacheError> {
    let mut statement = conn.prepare(
        "SELECT cid, name, \"desc\", coll, key
         FROM pragma_index_xinfo(?1, 'main')
         ORDER BY seqno",
    )?;
    let rows = statement.query_map([index_name], |row| {
        Ok(IndexXinfoRow {
            cid: row.get(0)?,
            name: row.get(1)?,
            descending: row.get::<_, i64>(2)? != 0,
            collation: row.get(3)?,
            key: row.get::<_, i64>(4)? != 0,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub(crate) fn validate_index_terms(
    rows: &[IndexXinfoRow],
    expected: &[&str],
) -> Result<(), &'static str> {
    let key_rows: Vec<&IndexXinfoRow> = rows.iter().filter(|row| row.key).collect();
    if key_rows.len() != expected.len() {
        return Err("index key-term count mismatch");
    }
    for (row, expected_name) in key_rows.into_iter().zip(expected) {
        if row.cid < 0
            || row.name.as_deref() != Some(*expected_name)
            || row.descending
            || !row
                .collation
                .as_deref()
                .is_some_and(|value| value.eq_ignore_ascii_case("BINARY"))
        {
            return Err("index key-term mismatch");
        }
    }
    let auxiliary_rows: Vec<&IndexXinfoRow> = rows.iter().filter(|row| !row.key).collect();
    if auxiliary_rows.len() != 1 {
        return Err("index auxiliary row count mismatch");
    }
    let auxiliary = auxiliary_rows[0];
    if auxiliary.cid != -1
        || auxiliary.name.is_some()
        || auxiliary.descending
        || !auxiliary
            .collation
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case("BINARY"))
    {
        return Err("index auxiliary row mismatch");
    }
    Ok(())
}

fn validate_new_suffix(suffix: &str) -> Result<(), LocalFileCacheError> {
    if suffix.is_empty()
        || suffix.len() > 64
        || !suffix
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(identifier_error());
    }
    Ok(())
}

fn quote_identifier(name: &str) -> QuotedIdentifier {
    QuotedIdentifier(format!("\"{}\"", name.replace('"', "\"\"")))
}

fn identifier_eq(left: &str, right: &str) -> bool {
    left.len() == right.len()
        && left
            .bytes()
            .zip(right.bytes())
            .all(|(left, right)| left.eq_ignore_ascii_case(&right))
}

fn identifier_error() -> LocalFileCacheError {
    LocalFileCacheError::UnsupportedFeature(IDENTIFIER_ERROR.to_owned())
}

fn main_schema_objects(conn: &Connection) -> Result<Vec<SchemaObject>, LocalFileCacheError> {
    let mut statement = conn.prepare(
        "SELECT type, name, tbl_name, sql
         FROM main.sqlite_schema
         WHERE name NOT LIKE 'sqlite\\_%' ESCAPE '\\'
         ORDER BY type, name",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(SchemaObject {
            object_type: row.get(0)?,
            name: row.get(1)?,
            table_name: row.get(2)?,
            sql: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn public_schema_objects(conn: &Connection) -> Result<Vec<SchemaObject>, LocalFileCacheError> {
    let objects = main_schema_objects(conn)?;
    test_hook(TestPoint::SchemaRead)?;
    Ok(objects
        .into_iter()
        .filter(|object| object.name.starts_with(PUBLIC_INDEX_PREFIX))
        .collect())
}

fn resolve_schema_object(
    conn: &Connection,
    requested: &str,
) -> Result<Option<SchemaObject>, LocalFileCacheError> {
    let objects = main_schema_objects(conn)?;
    test_hook(TestPoint::SchemaRead)?;
    let mut matches = objects
        .into_iter()
        .filter(|object| identifier_eq(&object.name, requested));
    let result = matches.next();
    if matches.next().is_some() {
        return Err(identifier_error());
    }
    Ok(result)
}

fn validate_owned_path_index(
    conn: &Connection,
    object: &SchemaObject,
) -> Result<String, LocalFileCacheError> {
    if object.object_type != "index"
        || object.table_name != "files"
        || object.sql.is_none()
        || !object.name.starts_with(PUBLIC_INDEX_PREFIX)
    {
        return Err(identifier_error());
    }
    validate_index_shape(conn, &object.name, &["namespace", "path"])?;
    Ok(object.name.clone())
}

fn validate_builtin_index(
    conn: &Connection,
    object: &SchemaObject,
    expected: &[&str],
) -> Result<(), LocalFileCacheError> {
    if object.object_type != "index" || object.table_name != "files" || object.sql.is_none() {
        return Err(identifier_error());
    }
    validate_index_shape(conn, &object.name, expected)
}

fn validate_index_shape(
    conn: &Connection,
    name: &str,
    expected: &[&str],
) -> Result<(), LocalFileCacheError> {
    let row = main_file_indexes(conn)?
        .into_iter()
        .find(|row| identifier_eq(&row.name, name))
        .ok_or_else(identifier_error)?;
    if row.unique || row.partial || row.origin != "c" {
        return Err(identifier_error());
    }
    let xinfo = main_index_xinfo(conn, &row.name)?;
    validate_index_terms(&xinfo, expected).map_err(|_| identifier_error())
}

#[cfg(test)]
mod tests {
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

    fn set_hook(hook: impl FnMut(TestPoint) -> Result<(), LocalFileCacheError> + 'static) {
        TEST_HOOK.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
    }

    fn clear_hook() {
        TEST_HOOK.with(|slot| *slot.borrow_mut() = None);
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
        set_hook(|point| {
            if point == TestPoint::CreateDdl {
                return Err(LocalFileCacheError::UnsupportedFeature(
                    "after create".into(),
                ));
            }
            Ok(())
        });
        assert!(create_path_index(&engine.conn, "rollback_create").is_err());
        clear_hook();
        assert!(list_path_indexes(&engine.conn).unwrap().is_empty());
        assert_eq!(
            create_path_index(&engine.conn, "rollback_create").unwrap(),
            "lc_user_rollback_create"
        );
        assert!(drop_path_index(&engine.conn, "rollback_create").unwrap());

        create_path_index(&engine.conn, "rollback_drop").unwrap();
        set_hook(|point| {
            if point == TestPoint::DropDdl {
                return Err(LocalFileCacheError::UnsupportedFeature("after drop".into()));
            }
            Ok(())
        });
        assert!(drop_path_index(&engine.conn, "rollback_drop").is_err());
        clear_hook();
        assert_eq!(
            list_path_indexes(&engine.conn).unwrap(),
            ["lc_user_rollback_drop"]
        );
        assert!(drop_path_index(&engine.conn, "rollback_drop").unwrap());
        assert!(list_path_indexes(&engine.conn).unwrap().is_empty());
    }

    #[test]
    fn panic_after_drop_ddl_rolls_back() {
        let engine = engine();
        create_path_index(&engine.conn, "panic_drop").unwrap();
        set_hook(|point| {
            if point == TestPoint::DropDdl {
                panic!("synthetic post-drop panic");
            }
            Ok(())
        });
        let unwind = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = drop_path_index(&engine.conn, "panic_drop");
        }));
        clear_hook();
        assert!(unwind.is_err());
        assert_eq!(
            list_path_indexes(&engine.conn).unwrap(),
            ["lc_user_panic_drop"]
        );
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
                "CREATE TEMP TABLE files(namespace TEXT, path TEXT);
                 INSERT INTO temp.files VALUES ('default', '/temp-only');
                 CREATE INDEX temp.lc_user_collision ON files(namespace, path);
                 ATTACH ':memory:' AS other;
                 CREATE TABLE other.files(namespace TEXT, path TEXT);
                 CREATE INDEX other.lc_user_collision ON files(namespace, path);",
            )
            .unwrap();

        assert_eq!(
            create_path_index(&engine.conn, "collision").unwrap(),
            "lc_user_collision"
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
        set_hook(move |point| {
            if point == TestPoint::SchemaRead && !changed_in_hook.swap(true, Ordering::SeqCst) {
                let writer = Connection::open(&database_in_hook)?;
                writer.execute_batch(
                    "DROP INDEX main.lc_user_snapshot;
                     CREATE INDEX main.lc_user_snapshot ON files(path);",
                )?;
            }
            Ok(())
        });
        let result = authorize_query_index(&engine.conn, Some("lc_user_snapshot"));
        clear_hook();

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
        set_hook(move |point| {
            if point == TestPoint::Authorization && !changed_in_hook.swap(true, Ordering::SeqCst) {
                let writer = Connection::open(&database_in_hook)?;
                writer.execute_batch(
                    "DROP INDEX main.lc_user_race;
                     CREATE INDEX main.lc_user_race ON files(path);",
                )?;
            }
            Ok(())
        });
        let result = repository::keys(
            &engine.conn,
            "default",
            None,
            Some("lc_user_race"),
            None,
            None,
        );
        clear_hook();

        assert!(changed.load(Ordering::SeqCst));
        assert!(result.unwrap().is_empty());
        assert!(authorize_query_index(&engine.conn, Some("lc_user_race")).is_err());
    }
}
