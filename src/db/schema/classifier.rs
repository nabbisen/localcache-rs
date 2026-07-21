//! Read-only, fail-closed schema classification for RFC 010.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, OptionalExtension, types::Value};

use crate::error::LocalFileCacheError;

const CURRENT_VERSION: i64 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SchemaState {
    Fresh,
    Version { version: u8, files_high_water: i64 },
}

#[derive(Debug)]
struct SchemaObject {
    object_type: String,
    name: String,
    table_name: String,
    sql: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
struct Column {
    name: String,
    declared_type: String,
    not_null: bool,
    default: Option<String>,
    primary_key_position: i64,
    hidden: i64,
}

#[derive(Debug)]
struct IndexListRow {
    name: String,
    unique: bool,
    origin: String,
    partial: bool,
}

#[derive(Debug, Clone)]
struct IndexXinfoRow {
    cid: i64,
    name: Option<String>,
    descending: bool,
    collation: Option<String>,
    key: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DdlToken {
    Word(String),
    StringLiteral(String),
    Symbol(char),
}

pub(super) fn read_user_version(conn: &Connection) -> Result<i64, LocalFileCacheError> {
    Ok(conn.query_row("PRAGMA user_version", [], |row| row.get(0))?)
}

pub(super) fn classify(
    conn: &Connection,
    physical_version: i64,
) -> Result<SchemaState, LocalFileCacheError> {
    if !(0..=CURRENT_VERSION).contains(&physical_version) {
        return Err(unrecognized(physical_version, "unsupported schema version"));
    }

    let objects = application_objects(conn)?;
    if physical_version == 0 && objects.is_empty() {
        return Ok(SchemaState::Fresh);
    }

    let effective_version = if physical_version == 0 {
        1
    } else {
        physical_version as u8
    };
    let files_high_water = validate_version(conn, physical_version, effective_version, &objects)?;
    Ok(SchemaState::Version {
        version: effective_version,
        files_high_water,
    })
}

fn validate_version(
    conn: &Connection,
    physical_version: i64,
    version: u8,
    objects: &[SchemaObject],
) -> Result<i64, LocalFileCacheError> {
    validate_object_policy(physical_version, version, objects)?;
    validate_table(conn, physical_version, "files", version, objects)?;
    validate_table(conn, physical_version, "payloads", version, objects)?;
    validate_foreign_key(conn, physical_version)?;
    validate_indexes(conn, physical_version, version)?;
    let files_high_water = validate_sequence(conn, physical_version)?;
    validate_foreign_key_integrity(conn, physical_version)?;
    Ok(files_high_water)
}

fn application_objects(conn: &Connection) -> Result<Vec<SchemaObject>, LocalFileCacheError> {
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

fn validate_object_policy(
    physical_version: i64,
    version: u8,
    objects: &[SchemaObject],
) -> Result<(), LocalFileCacheError> {
    let required_indexes: BTreeSet<&str> = match version {
        1 => ["idx_files_path"].into_iter().collect(),
        2 | 3 => ["idx_files_namespace_path"].into_iter().collect(),
        4 | 5 => ["idx_files_lru", "idx_files_namespace_path"]
            .into_iter()
            .collect(),
        _ => unreachable!(),
    };
    let mut tables = BTreeSet::new();
    let mut indexes = BTreeSet::new();

    for object in objects {
        match object.object_type.as_str() {
            "table" if object.name == "files" || object.name == "payloads" => {
                if object.table_name != object.name || object.sql.is_none() {
                    return Err(unrecognized(physical_version, "invalid table metadata"));
                }
                tables.insert(object.name.as_str());
            }
            "index" if required_indexes.contains(object.name.as_str()) => {
                if object.table_name != "files" || object.sql.is_none() {
                    return Err(unrecognized(
                        physical_version,
                        "invalid built-in index metadata",
                    ));
                }
                indexes.insert(object.name.as_str());
            }
            "index" if version >= 4 && object.name.starts_with("lc_user_") => {
                if object.table_name != "files" || object.sql.is_none() {
                    return Err(unrecognized(
                        physical_version,
                        "invalid public user index metadata",
                    ));
                }
                indexes.insert(object.name.as_str());
            }
            _ => {
                return Err(unrecognized(
                    physical_version,
                    "unexpected application object",
                ));
            }
        }
    }

    if tables != BTreeSet::from(["files", "payloads"])
        || !required_indexes.iter().all(|name| indexes.contains(name))
    {
        return Err(unrecognized(
            physical_version,
            "missing required table or index",
        ));
    }
    Ok(())
}

fn validate_table(
    conn: &Connection,
    physical_version: i64,
    table_name: &str,
    version: u8,
    objects: &[SchemaObject],
) -> Result<(), LocalFileCacheError> {
    let table_kind: Option<(String, i64, i64)> = conn
        .query_row(
            "SELECT type, wr, strict
             FROM pragma_table_list
             WHERE schema = 'main' AND name = ?1",
            [table_name],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    if table_kind != Some(("table".to_owned(), 0, 0)) {
        return Err(unrecognized(
            physical_version,
            "table is not an ordinary non-STRICT rowid table",
        ));
    }

    let actual = table_columns(conn, table_name)?;
    let expected = expected_columns(table_name, version);
    if actual != expected {
        return Err(unrecognized(physical_version, "column contract mismatch"));
    }

    let sql = objects
        .iter()
        .find(|object| object.object_type == "table" && object.name == table_name)
        .and_then(|object| object.sql.as_deref())
        .ok_or_else(|| unrecognized(physical_version, "missing table DDL"))?;
    validate_table_ddl(sql, table_name, physical_version)?;
    Ok(())
}

fn table_columns(conn: &Connection, table_name: &str) -> Result<Vec<Column>, LocalFileCacheError> {
    let mut statement = conn.prepare(
        "SELECT name, type, \"notnull\", dflt_value, pk, hidden
         FROM pragma_table_xinfo(?1)
         ORDER BY cid",
    )?;
    let rows = statement.query_map([table_name], |row| {
        Ok(Column {
            name: row.get(0)?,
            declared_type: row.get::<_, String>(1)?.to_ascii_uppercase(),
            not_null: row.get::<_, i64>(2)? != 0,
            default: row.get(3)?,
            primary_key_position: row.get(4)?,
            hidden: row.get(5)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn expected_columns(table_name: &str, version: u8) -> Vec<Column> {
    let column = |name: &str,
                  declared_type: &str,
                  not_null: bool,
                  default: Option<&str>,
                  primary_key_position: i64| Column {
        name: name.to_owned(),
        declared_type: declared_type.to_owned(),
        not_null,
        default: default.map(str::to_owned),
        primary_key_position,
        hidden: 0,
    };

    if table_name == "files" {
        let mut columns = vec![
            column("id", "INTEGER", false, None, 1),
            column("path", "TEXT", true, None, 0),
            column("mtime", "INTEGER", true, None, 0),
            column("file_size", "INTEGER", true, None, 0),
            column("hash", "TEXT", false, None, 0),
            column("updated_at", "INTEGER", true, None, 0),
        ];
        if version >= 2 {
            columns.insert(1, column("namespace", "TEXT", true, Some("'default'"), 0));
        }
        if version >= 3 {
            columns.push(column("payload_version", "INTEGER", true, Some("0"), 0));
        }
        if version >= 4 {
            columns.push(column("last_accessed_at", "INTEGER", true, Some("0"), 0));
        }
        columns
    } else {
        let mut columns = vec![
            column("file_id", "INTEGER", false, None, 1),
            column("content", "BLOB", true, None, 0),
        ];
        if version >= 3 {
            columns.push(column("encoding", "TEXT", true, Some("'raw'"), 0));
        }
        columns
    }
}

fn validate_table_ddl(
    sql: &str,
    table_name: &str,
    physical_version: i64,
) -> Result<(), LocalFileCacheError> {
    let tokens = tokenize(sql).map_err(|reason| unrecognized(physical_version, reason))?;
    let words: Vec<&str> = tokens
        .iter()
        .filter_map(|token| match token {
            DdlToken::Word(word) => Some(word.as_str()),
            _ => None,
        })
        .collect();
    if words.get(0..3) != Some(&["create", "table", table_name][..]) {
        return Err(unrecognized(
            physical_version,
            "unsupported table DDL prefix",
        ));
    }
    for forbidden in [
        "as",
        "check",
        "collate",
        "constraint",
        "deferrable",
        "generated",
        "match",
        "strict",
        "virtual",
        "without",
    ] {
        if words.contains(&forbidden) {
            return Err(unrecognized(
                physical_version,
                "unsupported table DDL grammar",
            ));
        }
    }
    if contains_word_sequence(&words, &["on", "conflict"]) {
        return Err(unrecognized(
            physical_version,
            "non-released constraint conflict policy",
        ));
    }

    let count = |word: &str| words.iter().filter(|candidate| **candidate == word).count();
    let id_contract = if table_name == "files" {
        ["id", "integer", "primary", "key", "autoincrement"]
    } else {
        ["file_id", "integer", "primary", "key", ""]
    };
    if !contains_word_sequence(&words, &id_contract[..4])
        || (table_name == "files" && !contains_word_sequence(&words, &id_contract))
        || count("primary") != 1
        || count("key") != if table_name == "files" { 1 } else { 2 }
        || count("autoincrement") != usize::from(table_name == "files")
        || count("unique") != usize::from(table_name == "files")
        || count("foreign") != usize::from(table_name == "payloads")
        || count("references") != usize::from(table_name == "payloads")
    {
        return Err(unrecognized(
            physical_version,
            "table DDL constraint mismatch",
        ));
    }
    if table_name == "payloads"
        && (!contains_word_sequence(&words, &["foreign", "key", "file_id"])
            || !contains_word_sequence(&words, &["references", "files", "id"])
            || !contains_word_sequence(&words, &["on", "delete", "cascade"]))
    {
        return Err(unrecognized(
            physical_version,
            "payload foreign-key DDL mismatch",
        ));
    }
    Ok(())
}

fn tokenize(sql: &str) -> Result<Vec<DdlToken>, &'static str> {
    let chars: Vec<char> = sql.chars().collect();
    let mut tokens = Vec::new();
    let mut cursor = 0;
    while cursor < chars.len() {
        let ch = chars[cursor];
        if ch.is_whitespace() {
            cursor += 1;
        } else if ch.is_ascii_alphabetic() || ch == '_' {
            let start = cursor;
            cursor += 1;
            while cursor < chars.len()
                && (chars[cursor].is_ascii_alphanumeric() || chars[cursor] == '_')
            {
                cursor += 1;
            }
            tokens.push(DdlToken::Word(
                chars[start..cursor]
                    .iter()
                    .collect::<String>()
                    .to_ascii_lowercase(),
            ));
        } else if ch == '"' || ch == '`' || ch == '[' {
            let closing = if ch == '[' { ']' } else { ch };
            cursor += 1;
            let mut identifier = String::new();
            loop {
                let Some(&next) = chars.get(cursor) else {
                    return Err("unterminated quoted identifier");
                };
                cursor += 1;
                if next == closing {
                    if closing != ']' && chars.get(cursor) == Some(&closing) {
                        identifier.push(closing);
                        cursor += 1;
                    } else {
                        break;
                    }
                } else {
                    identifier.push(next);
                }
            }
            if identifier.is_empty()
                || !identifier
                    .chars()
                    .all(|value| value.is_ascii_alphanumeric() || value == '_')
            {
                return Err("unsupported quoted identifier");
            }
            tokens.push(DdlToken::Word(identifier.to_ascii_lowercase()));
        } else if ch == '\'' {
            cursor += 1;
            let mut value = String::new();
            loop {
                let Some(&next) = chars.get(cursor) else {
                    return Err("unterminated string literal");
                };
                cursor += 1;
                if next == '\'' {
                    if chars.get(cursor) == Some(&'\'') {
                        value.push('\'');
                        cursor += 1;
                    } else {
                        break;
                    }
                } else {
                    value.push(next);
                }
            }
            tokens.push(DdlToken::StringLiteral(value));
        } else if matches!(ch, '(' | ')' | ',' | ';') || ch.is_ascii_digit() {
            tokens.push(DdlToken::Symbol(ch));
            cursor += 1;
        } else {
            return Err("unsupported token in table DDL");
        }
    }
    Ok(tokens)
}

fn contains_word_sequence(words: &[&str], expected: &[&str]) -> bool {
    words
        .windows(expected.len())
        .any(|window| window == expected)
}

fn validate_foreign_key(
    conn: &Connection,
    physical_version: i64,
) -> Result<(), LocalFileCacheError> {
    let mut statement = conn.prepare(
        "SELECT id, seq, \"table\", \"from\", \"to\", on_update, on_delete, match
         FROM pragma_foreign_key_list('payloads')
         ORDER BY id, seq",
    )?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let expected = vec![(
        0,
        0,
        "files".to_owned(),
        "file_id".to_owned(),
        "id".to_owned(),
        "NO ACTION".to_owned(),
        "CASCADE".to_owned(),
        "NONE".to_owned(),
    )];
    if rows != expected {
        return Err(unrecognized(
            physical_version,
            "foreign-key contract mismatch",
        ));
    }
    Ok(())
}

fn validate_indexes(
    conn: &Connection,
    physical_version: i64,
    version: u8,
) -> Result<(), LocalFileCacheError> {
    let mut statement = conn.prepare(
        "SELECT name, \"unique\", origin, partial
         FROM pragma_index_list('files')
         ORDER BY name",
    )?;
    let indexes = statement
        .query_map([], |row| {
            Ok(IndexListRow {
                name: row.get(0)?,
                unique: row.get::<_, i64>(1)? != 0,
                origin: row.get(2)?,
                partial: row.get::<_, i64>(3)? != 0,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let expected_builtins: BTreeMap<&str, &[&str]> = match version {
        1 => BTreeMap::from([("idx_files_path", &["path"][..])]),
        2 | 3 => BTreeMap::from([("idx_files_namespace_path", &["namespace", "path"][..])]),
        4 | 5 => BTreeMap::from([
            (
                "idx_files_lru",
                &["namespace", "last_accessed_at", "updated_at"][..],
            ),
            ("idx_files_namespace_path", &["namespace", "path"][..]),
        ]),
        _ => unreachable!(),
    };
    let unique_columns: &[&str] = if version == 1 {
        &["path"]
    } else {
        &["namespace", "path"]
    };
    let mut unique_count = 0;
    let mut seen_builtins = BTreeSet::new();

    for index in &indexes {
        let xinfo = index_xinfo(conn, &index.name)?;
        if index.origin == "u" {
            unique_count += 1;
            if !index.unique || index.partial {
                return Err(unrecognized(physical_version, "UNIQUE autoindex mismatch"));
            }
            validate_index_terms(&xinfo, unique_columns)
                .map_err(|reason| unrecognized(physical_version, reason))?;
        } else if let Some(expected) = expected_builtins.get(index.name.as_str()) {
            if index.unique || index.partial || index.origin != "c" {
                return Err(unrecognized(
                    physical_version,
                    "built-in index flags mismatch",
                ));
            }
            validate_index_terms(&xinfo, expected)
                .map_err(|reason| unrecognized(physical_version, reason))?;
            seen_builtins.insert(index.name.as_str());
        } else if version >= 4 && index.name.starts_with("lc_user_") {
            if index.unique || index.partial || index.origin != "c" {
                return Err(unrecognized(
                    physical_version,
                    "public user index flags mismatch",
                ));
            }
            let expected = &["namespace", "path"];
            validate_index_terms(&xinfo, expected)
                .map_err(|reason| unrecognized(physical_version, reason))?;
        } else {
            return Err(unrecognized(physical_version, "unexpected index"));
        }
    }

    if unique_count != 1 || seen_builtins.len() != expected_builtins.len() {
        return Err(unrecognized(
            physical_version,
            "required index contract mismatch",
        ));
    }

    let payload_index_count: i64 = conn.query_row(
        "SELECT count(*) FROM pragma_index_list('payloads')",
        [],
        |row| row.get(0),
    )?;
    if payload_index_count != 0 {
        return Err(unrecognized(physical_version, "unexpected payload index"));
    }
    Ok(())
}

fn index_xinfo(
    conn: &Connection,
    index_name: &str,
) -> Result<Vec<IndexXinfoRow>, LocalFileCacheError> {
    let mut statement = conn.prepare(
        "SELECT cid, name, \"desc\", coll, key
         FROM pragma_index_xinfo(?1)
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

fn validate_index_terms(rows: &[IndexXinfoRow], expected: &[&str]) -> Result<(), &'static str> {
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

    // SQLite reports the ordinary table row locator as a non-key auxiliary
    // row (usually cid=-1/name=NULL). It is implementation-owned, not an
    // expression or an additional user key term, so it must be tolerated.
    if !rows.iter().any(|row| !row.key) {
        return Err("index is missing SQLite auxiliary row locator");
    }
    Ok(())
}

fn validate_sequence(conn: &Connection, physical_version: i64) -> Result<i64, LocalFileCacheError> {
    let mut statement = conn.prepare("SELECT name, seq, typeof(seq) FROM sqlite_sequence")?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Value>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if rows.len() > 1 || rows.iter().any(|(name, _, _)| name != "files") {
        return Err(unrecognized(
            physical_version,
            "sqlite_sequence contract mismatch",
        ));
    }

    let (live_count, max_positive): (i64, Option<i64>) = conn.query_row(
        "SELECT count(*), max(CASE WHEN id > 0 THEN id END) FROM files",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    match rows.as_slice() {
        [] if live_count == 0 => Ok(0),
        [(name, Value::Integer(sequence), kind)]
            if name == "files" && kind == "integer" && *sequence >= 0 =>
        {
            Ok(max_positive.map_or(*sequence, |maximum| maximum.max(*sequence)))
        }
        _ => Err(unrecognized(
            physical_version,
            "invalid AUTOINCREMENT sequence state",
        )),
    }
}

fn validate_foreign_key_integrity(
    conn: &Connection,
    physical_version: i64,
) -> Result<(), LocalFileCacheError> {
    let violation_count: i64 =
        conn.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })?;
    if violation_count != 0 {
        return Err(unrecognized(physical_version, "foreign-key violation"));
    }
    Ok(())
}

fn unrecognized(physical_version: i64, reason: &str) -> LocalFileCacheError {
    LocalFileCacheError::UnsupportedFeature(format!(
        "unrecognized database schema at physical version {physical_version}: {reason}; database was not modified"
    ))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use sha2::{Digest, Sha256};

    use super::*;

    const V1_SCHEMA: &str = "
        CREATE TABLE files (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            path TEXT NOT NULL UNIQUE,
            mtime INTEGER NOT NULL,
            file_size INTEGER NOT NULL,
            hash TEXT,
            updated_at INTEGER NOT NULL
        );
        CREATE TABLE payloads (
            file_id INTEGER PRIMARY KEY,
            content BLOB NOT NULL,
            FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
        );
        CREATE INDEX idx_files_path ON files(path);
        PRAGMA user_version = 1;
    ";

    const V5_SCHEMA: &str = "
        CREATE TABLE files (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            namespace TEXT NOT NULL DEFAULT 'default',
            path TEXT NOT NULL,
            mtime INTEGER NOT NULL,
            file_size INTEGER NOT NULL,
            hash TEXT,
            updated_at INTEGER NOT NULL,
            payload_version INTEGER NOT NULL DEFAULT 0,
            last_accessed_at INTEGER NOT NULL DEFAULT 0,
            UNIQUE(namespace, path)
        );
        CREATE TABLE payloads (
            file_id INTEGER PRIMARY KEY,
            content BLOB NOT NULL,
            encoding TEXT NOT NULL DEFAULT 'raw',
            FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
        );
        CREATE INDEX idx_files_namespace_path ON files(namespace, path);
        CREATE INDEX idx_files_lru ON files(namespace, last_accessed_at, updated_at);
        PRAGMA user_version = 5;
    ";

    fn v5() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(V5_SCHEMA).unwrap();
        conn
    }

    fn version_state(version: u8, files_high_water: i64) -> SchemaState {
        SchemaState::Version {
            version,
            files_high_water,
        }
    }

    fn assert_fixture_sha256(path: &str, expected: &str) {
        let bytes = fs::read(path).unwrap();
        assert_eq!(format!("{:x}", Sha256::digest(bytes)), expected);
    }

    fn assert_rejected_unchanged(sql: &str) {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(sql).unwrap();
        let before: String = conn
            .query_row(
                "SELECT group_concat(type || ':' || name || ':' || coalesce(sql, ''), '|')
                 FROM (SELECT type, name, sql FROM sqlite_schema ORDER BY type, name)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let before_changes = conn.total_changes();
        let error = match classify(&conn, 5) {
            Err(error) => error.to_string(),
            Ok(state) => panic!("schema unexpectedly accepted as {state:?}:\n{sql}"),
        };
        let after: String = conn
            .query_row(
                "SELECT group_concat(type || ':' || name || ':' || coalesce(sql, ''), '|')
                 FROM (SELECT type, name, sql FROM sqlite_schema ORDER BY type, name)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(error.contains("database was not modified"), "{error}");
        assert_eq!(conn.total_changes(), before_changes);
        assert_eq!(after, before);
    }

    #[test]
    fn fresh_and_exact_v5_classify() {
        let fresh = Connection::open_in_memory().unwrap();
        assert_eq!(classify(&fresh, 0).unwrap(), SchemaState::Fresh);
        assert_eq!(classify(&v5(), 5).unwrap(), version_state(5, 0));
    }

    #[test]
    fn current_validation_is_query_only_and_has_zero_row_changes() {
        let conn = v5();
        conn.execute_batch(
            "INSERT INTO files
                (namespace, path, mtime, file_size, hash, updated_at, payload_version, last_accessed_at)
             VALUES ('default', '/fixture/current.bin', 123, 7, NULL, 456, 0, 789);
             INSERT INTO payloads (file_id, content, encoding)
             VALUES (1, X'0001FF', 'raw');",
        )
        .unwrap();
        let before_schema: String = conn
            .query_row(
                "SELECT group_concat(type || ':' || name || ':' || coalesce(sql, ''), '|')
                 FROM (SELECT type, name, sql FROM sqlite_schema ORDER BY type, name)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let before_data: (i64, String, i64, Vec<u8>, i64) = conn
            .query_row(
                "SELECT files.id, files.path, files.mtime, payloads.content,
                        (SELECT seq FROM sqlite_sequence WHERE name = 'files')
                 FROM files JOIN payloads ON payloads.file_id = files.id",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        let before_changes = conn.total_changes();
        conn.execute_batch("PRAGMA query_only = ON;").unwrap();

        super::super::initialize(&conn).unwrap();

        let after_schema: String = conn
            .query_row(
                "SELECT group_concat(type || ':' || name || ':' || coalesce(sql, ''), '|')
                 FROM (SELECT type, name, sql FROM sqlite_schema ORDER BY type, name)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(conn.total_changes(), before_changes);
        assert_eq!(after_schema, before_schema);
        let after_data: (i64, String, i64, Vec<u8>, i64) = conn
            .query_row(
                "SELECT files.id, files.path, files.mtime, payloads.content,
                        (SELECT seq FROM sqlite_sequence WHERE name = 'files')
                 FROM files JOIN payloads ON payloads.file_id = files.id",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(after_data, before_data);
        assert_eq!(read_user_version(&conn).unwrap(), 5);
    }

    #[test]
    fn exact_user_index_is_allowed_only_for_v4_and_v5() {
        let conn = v5();
        conn.execute_batch("CREATE INDEX lc_user_test ON files(namespace, path);")
            .unwrap();
        assert_eq!(classify(&conn, 5).unwrap(), version_state(5, 0));
    }

    #[test]
    fn exact_historical_fixtures_classify_without_writes() {
        assert_fixture_sha256(
            "tests/fixtures/compat-v0_1.sqlite3",
            "bd0bb9ffb9e07abafebde2c8a492618bf23ba8cf0e8c29cd8a9a76a4f5153aac",
        );
        assert_fixture_sha256(
            "tests/fixtures/compat-v0_19-user-index.sqlite3",
            "585ea037ad94ef77696b3bb3c6d13d9778975057e2bdd7bdc5b01b299cfc86df",
        );
        for (path, version, expected) in [
            ("tests/fixtures/compat-v0_1.sqlite3", 0, version_state(1, 3)),
            (
                "tests/fixtures/compat-v0_19-user-index.sqlite3",
                4,
                version_state(4, 1),
            ),
        ] {
            let conn = Connection::open_with_flags(
                Path::new(path),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap();
            let before_changes = conn.total_changes();
            assert_eq!(classify(&conn, version).unwrap(), expected);
            assert_eq!(conn.total_changes(), before_changes);
        }
    }

    #[test]
    fn explicit_v1_without_payload_carries_sequence_above_live_id() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(V1_SCHEMA).unwrap();
        conn.execute(
            "INSERT INTO files (id, path, mtime, file_size, updated_at)
             VALUES (3, 'without-payload', 0, 0, 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE sqlite_sequence SET seq = 17 WHERE name = 'files'",
            [],
        )
        .unwrap();
        assert_eq!(classify(&conn, 1).unwrap(), version_state(1, 17));
        assert_eq!(
            conn.query_row("SELECT count(*) FROM payloads", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn sqlite_auxiliary_index_rows_are_not_user_key_terms() {
        let rows = vec![
            IndexXinfoRow {
                cid: 1,
                name: Some("namespace".into()),
                descending: false,
                collation: Some("BINARY".into()),
                key: true,
            },
            IndexXinfoRow {
                cid: 2,
                name: Some("path".into()),
                descending: false,
                collation: Some("BINARY".into()),
                key: true,
            },
            IndexXinfoRow {
                cid: -1,
                name: None,
                descending: false,
                collation: Some("BINARY".into()),
                key: false,
            },
        ];
        validate_index_terms(&rows, &["namespace", "path"]).unwrap();

        let mut extra_key = rows;
        extra_key.push(IndexXinfoRow {
            cid: 3,
            name: Some("mtime".into()),
            descending: false,
            collation: Some("BINARY".into()),
            key: true,
        });
        assert!(validate_index_terms(&extra_key, &["namespace", "path"]).is_err());
    }

    #[test]
    fn sqlite_reports_auxiliary_rows_for_ordinary_and_unique_indexes() {
        let conn = v5();
        let ordinary = index_xinfo(&conn, "idx_files_namespace_path").unwrap();
        validate_index_terms(&ordinary, &["namespace", "path"]).unwrap();

        let unique_name: String = conn
            .query_row(
                "SELECT name FROM pragma_index_list('files') WHERE origin = 'u'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let unique = index_xinfo(&conn, &unique_name).unwrap();
        validate_index_terms(&unique, &["namespace", "path"]).unwrap();
        assert!(ordinary.iter().any(|row| !row.key));
        assert!(unique.iter().any(|row| !row.key));
    }

    #[test]
    fn malformed_near_matches_fail_closed() {
        let cases = [
            "CREATE VIEW unrelated AS SELECT 1",
            "CREATE TRIGGER unexpected AFTER INSERT ON files BEGIN SELECT 1; END",
            "CREATE INDEX lc_user_wrong ON files(path)",
            "CREATE INDEX extra ON files(namespace, path)",
        ];
        for sql in cases {
            let conn = v5();
            conn.execute_batch(sql).unwrap();
            let error = classify(&conn, 5).unwrap_err().to_string();
            assert!(error.contains("database was not modified"), "{error}");
        }
    }

    #[test]
    fn table_and_index_contract_near_matches_fail_unchanged() {
        let variants = [
            V5_SCHEMA.replace("namespace TEXT", "namespace BLOB"),
            V5_SCHEMA.replace("path TEXT NOT NULL", "path TEXT"),
            V5_SCHEMA.replace("DEFAULT 'default'", "DEFAULT 'other'"),
            V5_SCHEMA.replace(
                "namespace TEXT NOT NULL DEFAULT 'default',\n            path TEXT NOT NULL,",
                "path TEXT NOT NULL,\n            namespace TEXT NOT NULL DEFAULT 'default',",
            ),
            V5_SCHEMA.replace(
                "last_accessed_at INTEGER NOT NULL DEFAULT 0,",
                "last_accessed_at INTEGER NOT NULL DEFAULT 0,\n            extra INTEGER,",
            ),
            V5_SCHEMA.replace(
                "last_accessed_at INTEGER NOT NULL DEFAULT 0,",
                "last_accessed_at INTEGER NOT NULL DEFAULT 0,\n            generated INTEGER GENERATED ALWAYS AS (mtime) VIRTUAL,",
            ),
            V5_SCHEMA.replace("UNIQUE(namespace, path)", "UNIQUE(path, namespace)"),
            V5_SCHEMA.replace(
                "UNIQUE(namespace, path)",
                "UNIQUE(namespace, path) ON CONFLICT IGNORE",
            ),
            V5_SCHEMA.replace(
                "UNIQUE(namespace, path)",
                "UNIQUE(namespace, path) ON CONFLICT REPLACE",
            ),
            V5_SCHEMA.replace(
                "id INTEGER PRIMARY KEY AUTOINCREMENT",
                "id INTEGER PRIMARY KEY ON CONFLICT FAIL AUTOINCREMENT",
            ),
            V5_SCHEMA.replace(
                "path TEXT NOT NULL,",
                "path TEXT NOT NULL ON CONFLICT IGNORE,",
            ),
            V5_SCHEMA.replace(
                "id INTEGER PRIMARY KEY AUTOINCREMENT",
                "id INTEGER UNIQUE",
            ),
            V5_SCHEMA.replace("hash TEXT,", "hash TEXT CHECK(hash IS NULL),"),
            V5_SCHEMA.replace("ON DELETE CASCADE", "ON DELETE RESTRICT"),
            V5_SCHEMA.replace(
                "path TEXT NOT NULL,",
                "path TEXT COLLATE NOCASE NOT NULL,",
            ),
            V5_SCHEMA.replace(
                "UNIQUE(namespace, path)\n        );",
                "UNIQUE(namespace, path)\n        ) STRICT;",
            ),
            V5_SCHEMA.replace(
                "FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE\n        );",
                "FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE\n        ) WITHOUT ROWID;",
            ),
            V5_SCHEMA.to_owned()
                + "CREATE INDEX lc_user_partial ON files(namespace, path) WHERE mtime > 0;",
            V5_SCHEMA.replace(
                "CREATE INDEX idx_files_namespace_path ON files(namespace, path);",
                "CREATE INDEX idx_files_namespace_path ON files(namespace, path DESC);",
            ),
            V5_SCHEMA.replace(
                "CREATE INDEX idx_files_lru ON files(namespace, last_accessed_at, updated_at);",
                "CREATE INDEX idx_files_lru ON files(namespace COLLATE NOCASE, last_accessed_at, updated_at);",
            ),
            V5_SCHEMA.to_owned() + "CREATE UNIQUE INDEX lc_user_unique ON files(namespace, path);",
            V5_SCHEMA.to_owned() + "CREATE INDEX lc_user_expression ON files(lower(path));",
        ];
        for sql in variants {
            assert_rejected_unchanged(&sql);
        }
    }

    #[test]
    fn invalid_sequence_types_and_values_fail_closed() {
        for replacement in ["NULL", "1.5", "'3'", "X'03'", "-1"] {
            let conn = v5();
            conn.execute(
                "INSERT INTO files (path, mtime, file_size, updated_at) VALUES ('p', 0, 0, 0)",
                [],
            )
            .unwrap();
            conn.execute(
                "UPDATE sqlite_sequence SET seq = 3 WHERE name = 'files'",
                [],
            )
            .unwrap();
            conn.execute_batch(&format!(
                "UPDATE sqlite_sequence SET seq = {replacement} WHERE name = 'files'"
            ))
            .unwrap();
            assert!(classify(&conn, 5).is_err(), "accepted {replacement}");
        }
    }

    #[test]
    fn sequence_effective_high_water_matrix_matches_rfc() {
        let empty = v5();
        assert_eq!(classify(&empty, 5).unwrap(), version_state(5, 0));

        let absent_nonempty = v5();
        absent_nonempty
            .execute(
                "INSERT INTO files (id, path, mtime, file_size, updated_at)
                 VALUES (7, 'p', 0, 0, 0)",
                [],
            )
            .unwrap();
        absent_nonempty
            .execute("DELETE FROM sqlite_sequence WHERE name = 'files'", [])
            .unwrap();
        assert!(classify(&absent_nonempty, 5).is_err());

        let duplicate = v5();
        duplicate
            .execute(
                "INSERT INTO files (path, mtime, file_size, updated_at)
                 VALUES ('p', 0, 0, 0)",
                [],
            )
            .unwrap();
        duplicate
            .execute(
                "INSERT INTO sqlite_sequence (name, seq) VALUES ('files', 2)",
                [],
            )
            .unwrap();
        assert!(classify(&duplicate, 5).is_err());

        let below_live = v5();
        below_live
            .execute(
                "INSERT INTO files (id, path, mtime, file_size, updated_at)
                 VALUES (7, 'p', 0, 0, 0)",
                [],
            )
            .unwrap();
        below_live
            .execute(
                "UPDATE sqlite_sequence SET seq = 3 WHERE name = 'files'",
                [],
            )
            .unwrap();
        assert_eq!(classify(&below_live, 5).unwrap(), version_state(5, 7));

        let above_live = v5();
        above_live
            .execute(
                "INSERT INTO files (id, path, mtime, file_size, updated_at)
                 VALUES (7, 'p', 0, 0, 0)",
                [],
            )
            .unwrap();
        above_live
            .execute(
                "UPDATE sqlite_sequence SET seq = 11 WHERE name = 'files'",
                [],
            )
            .unwrap();
        assert_eq!(classify(&above_live, 5).unwrap(), version_state(5, 11));

        let maximum = v5();
        maximum
            .execute(
                "INSERT INTO files (id, path, mtime, file_size, updated_at)
                 VALUES (?1, 'p', 0, 0, 0)",
                [i64::MAX],
            )
            .unwrap();
        assert_eq!(classify(&maximum, 5).unwrap(), version_state(5, i64::MAX));
    }

    #[test]
    fn unsupported_versions_and_each_version_zero_object_kind_fail_unchanged() {
        let conn = v5();
        for version in [-1, 6, i64::MAX] {
            let error = classify(&conn, version).unwrap_err().to_string();
            assert!(error.contains("database was not modified"), "{error}");
        }

        for sql in [
            "CREATE TABLE unrelated (id INTEGER)",
            "CREATE TABLE unrelated (id INTEGER); CREATE INDEX unrelated_idx ON unrelated(id)",
            "CREATE VIEW unrelated AS SELECT 1",
            "CREATE TABLE unrelated (id INTEGER); \
             CREATE TRIGGER unrelated_trigger AFTER INSERT ON unrelated BEGIN SELECT 1; END",
        ] {
            let candidate = Connection::open_in_memory().unwrap();
            candidate.execute_batch(sql).unwrap();
            let before_changes = candidate.total_changes();
            let error = classify(&candidate, 0).unwrap_err().to_string();
            assert!(error.contains("database was not modified"), "{error}");
            assert_eq!(candidate.total_changes(), before_changes);
        }
    }

    #[test]
    fn orphan_payload_is_rejected_even_when_foreign_keys_were_disabled() {
        let conn = v5();
        conn.execute_batch("PRAGMA foreign_keys = OFF;").unwrap();
        conn.execute(
            "INSERT INTO payloads (file_id, content) VALUES (99, X'00')",
            [],
        )
        .unwrap();
        assert!(classify(&conn, 5).is_err());
    }
}
