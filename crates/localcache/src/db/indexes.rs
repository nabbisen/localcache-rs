//! Safe SQLite index identifier and ownership boundary (RFC 011).

use rusqlite::{Connection, TransactionBehavior};

use crate::error::{ArgumentName, LocalFileCacheError, SqliteResultExt, invalid_argument};

pub(crate) const PUBLIC_INDEX_PREFIX: &str = "lc_user_";
const IDENTIFIER_ERROR: &str = "not a valid SQLite identifier or not an allowed localcache index";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexListRow {
    pub(crate) name: String,
    pub(crate) unique: bool,
    pub(crate) origin: String,
    pub(crate) partial: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexXinfoRow {
    pub(crate) cid: i64,
    pub(crate) name: Option<String>,
    pub(crate) descending: bool,
    pub(crate) collation: Option<String>,
    pub(crate) key: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestPoint {
    SchemaRead,
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

pub(crate) fn drop_path_index(
    conn: &Connection,
    suffix: &str,
) -> Result<bool, LocalFileCacheError> {
    let full = format!("{PUBLIC_INDEX_PREFIX}{suffix}");
    let transaction =
        rusqlite::Transaction::new_unchecked(conn, TransactionBehavior::Immediate).db()?;

    let Some(object) = resolve_schema_object(&transaction, &full)? else {
        transaction.commit().db()?;
        return Ok(false);
    };
    validate_owned_path_index(&transaction, &object)?;

    // RFC 011 N-01: quote the catalog's own spelling (`object.name`, already
    // resolved and validated above), not the caller's `full` -- both are
    // guaranteed equal by `identifier_eq`, so this changes nothing about
    // which index gets dropped, but it makes that guarantee locally obvious
    // instead of resting on `identifier_eq`'s exact semantics one function
    // away.
    let quoted = quote_identifier(&object.name);
    let sql = format!("DROP INDEX main.{}", quoted.as_sql());
    transaction.execute(&sql, []).db()?;
    #[cfg(test)]
    test_hook(TestPoint::DropDdl)?;
    if resolve_schema_object(&transaction, &full)?.is_some() {
        return Err(identifier_error());
    }
    transaction.commit().db()?;
    Ok(true)
}

pub(crate) fn list_path_indexes(conn: &Connection) -> Result<Vec<String>, LocalFileCacheError> {
    let transaction =
        rusqlite::Transaction::new_unchecked(conn, TransactionBehavior::Deferred).db()?;
    let objects = public_schema_objects(&transaction)?;
    let mut names = Vec::with_capacity(objects.len());
    for object in &objects {
        names.push(validate_owned_path_index(&transaction, object)?);
    }
    names.sort();
    transaction.commit().db()?;
    Ok(names)
}

pub(crate) fn main_file_indexes(
    conn: &Connection,
) -> Result<Vec<IndexListRow>, LocalFileCacheError> {
    let mut statement = conn
        .prepare(
            "SELECT name, \"unique\", origin, partial
         FROM pragma_index_list('files', 'main')
         ORDER BY name",
        )
        .db()?;
    let rows = statement
        .query_map([], |row| {
            Ok(IndexListRow {
                name: row.get(0)?,
                unique: row.get::<_, i64>(1)? != 0,
                origin: row.get(2)?,
                partial: row.get::<_, i64>(3)? != 0,
            })
        })
        .db()?;
    rows.collect::<Result<Vec<_>, _>>().db()
}

pub(crate) fn main_index_xinfo(
    conn: &Connection,
    index_name: &str,
) -> Result<Vec<IndexXinfoRow>, LocalFileCacheError> {
    let mut statement = conn
        .prepare(
            "SELECT cid, name, \"desc\", coll, key
         FROM pragma_index_xinfo(?1, 'main')
         ORDER BY seqno",
        )
        .db()?;
    let rows = statement
        .query_map([index_name], |row| {
            Ok(IndexXinfoRow {
                cid: row.get(0)?,
                name: row.get(1)?,
                descending: row.get::<_, i64>(2)? != 0,
                collation: row.get(3)?,
                key: row.get::<_, i64>(4)? != 0,
            })
        })
        .db()?;
    rows.collect::<Result<Vec<_>, _>>().db()
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

fn quote_identifier(name: &str) -> QuotedIdentifier {
    QuotedIdentifier(format!("\"{}\"", name.replace('"', "\"\"")))
}

/// RFC 011 N-02: this ASCII-only case folding is deliberate, matching
/// SQLite's own identifier comparison, which is also ASCII-only. Widening it
/// to Unicode-aware casefolding would change which identifiers this crate
/// considers equal to which the database considers equal, reopening exactly
/// the mismatch this function exists to prevent. Do not "fix" this to use
/// `str::eq_ignore_ascii_case` on the whole string or a Unicode-aware
/// comparison without re-deriving that SQLite's own behavior changed too.
fn identifier_eq(left: &str, right: &str) -> bool {
    left.len() == right.len()
        && left
            .bytes()
            .zip(right.bytes())
            .all(|(left, right)| left.eq_ignore_ascii_case(&right))
}

fn identifier_error() -> LocalFileCacheError {
    invalid_argument(ArgumentName::IndexName, IDENTIFIER_ERROR)
}

fn main_schema_objects(conn: &Connection) -> Result<Vec<SchemaObject>, LocalFileCacheError> {
    let mut statement = conn
        .prepare(
            "SELECT type, name, tbl_name, sql
         FROM main.sqlite_schema
         WHERE name NOT LIKE 'sqlite\\_%' ESCAPE '\\'
         ORDER BY type, name",
        )
        .db()?;
    let rows = statement
        .query_map([], |row| {
            Ok(SchemaObject {
                object_type: row.get(0)?,
                name: row.get(1)?,
                table_name: row.get(2)?,
                sql: row.get(3)?,
            })
        })
        .db()?;
    rows.collect::<Result<Vec<_>, _>>().db()
}

fn public_schema_objects(conn: &Connection) -> Result<Vec<SchemaObject>, LocalFileCacheError> {
    let objects = main_schema_objects(conn)?;
    #[cfg(test)]
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
    #[cfg(test)]
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
mod tests;
