//! Read-only, fail-closed schema classification for RFC 010.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, OptionalExtension, types::Value};

use crate::db::indexes::{main_file_indexes, main_index_xinfo, validate_index_terms};
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

/// Validate an intermediate schema shape inside RFC 010's outer migration
/// transaction without changing `user_version` between steps.
pub(super) fn validate_effective_version(
    conn: &Connection,
    version: u8,
) -> Result<SchemaState, LocalFileCacheError> {
    let objects = application_objects(conn)?;
    let files_high_water = validate_version(conn, i64::from(version), version, &objects)?;
    Ok(SchemaState::Version {
        version,
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
    let indexes = main_file_indexes(conn)?;

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
        let xinfo = main_index_xinfo(conn, &index.name)?;
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
        "SELECT count(*) FROM pragma_index_list('payloads', 'main')",
        [],
        |row| row.get(0),
    )?;
    if payload_index_count != 0 {
        return Err(unrecognized(physical_version, "unexpected payload index"));
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
#[path = "classifier/tests.rs"]
mod tests;
