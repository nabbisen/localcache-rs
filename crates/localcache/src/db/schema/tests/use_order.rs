//! RFC 026 R3: the migration to schema v6 (the use order replaces
//! `last_accessed_at`, and the duplicate path index goes).
//!
//! The expected values are computed here by the recurrence itself, written out
//! independently of the window statement the migration uses.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use rusqlite::{Connection, params};
use tempfile::TempDir;

use crate::LocalFileCacheError;

use super::super::{
    CURRENT_VERSION,
    classifier::{self, SchemaState},
    initialize, initialize_with_hook,
    migration::MigrationPoint,
};

const HISTORICAL_FILES: [&str; 4] = [
    // v2
    "CREATE TABLE files (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        namespace TEXT NOT NULL DEFAULT 'default',
        path TEXT NOT NULL, mtime INTEGER NOT NULL, file_size INTEGER NOT NULL,
        hash TEXT, updated_at INTEGER NOT NULL,
        UNIQUE(namespace, path));",
    // v3
    "CREATE TABLE files (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        namespace TEXT NOT NULL DEFAULT 'default',
        path TEXT NOT NULL, mtime INTEGER NOT NULL, file_size INTEGER NOT NULL,
        hash TEXT, updated_at INTEGER NOT NULL,
        payload_version INTEGER NOT NULL DEFAULT 0,
        UNIQUE(namespace, path));",
    // v4
    "CREATE TABLE files (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        namespace TEXT NOT NULL DEFAULT 'default',
        path TEXT NOT NULL, mtime INTEGER NOT NULL, file_size INTEGER NOT NULL,
        hash TEXT, updated_at INTEGER NOT NULL,
        payload_version INTEGER NOT NULL DEFAULT 0,
        last_accessed_at INTEGER NOT NULL DEFAULT 0,
        UNIQUE(namespace, path));",
    // v5 (the same shape as v4; mtime is nanoseconds)
    "CREATE TABLE files (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        namespace TEXT NOT NULL DEFAULT 'default',
        path TEXT NOT NULL, mtime INTEGER NOT NULL, file_size INTEGER NOT NULL,
        hash TEXT, updated_at INTEGER NOT NULL,
        payload_version INTEGER NOT NULL DEFAULT 0,
        last_accessed_at INTEGER NOT NULL DEFAULT 0,
        UNIQUE(namespace, path));",
];

fn create_historical(conn: &Connection, version: u8) {
    conn.execute_batch(HISTORICAL_FILES[usize::from(version) - 2])
        .unwrap();
    conn.execute_batch(if version >= 3 {
        "CREATE TABLE payloads (
            file_id INTEGER PRIMARY KEY, content BLOB NOT NULL,
            encoding TEXT NOT NULL DEFAULT 'raw',
            FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE);"
    } else {
        "CREATE TABLE payloads (
            file_id INTEGER PRIMARY KEY, content BLOB NOT NULL,
            FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE);"
    })
    .unwrap();
    conn.execute_batch("CREATE INDEX idx_files_namespace_path ON files(namespace, path);")
        .unwrap();
    if version >= 4 {
        conn.execute_batch(
            "CREATE INDEX idx_files_lru ON files(namespace, last_accessed_at, updated_at);",
        )
        .unwrap();
    }
    conn.execute_batch(&format!("PRAGMA user_version = {version};"))
        .unwrap();
}

/// A few rows, two namespaces, with ties in `last_accessed_at` and `updated_at`.
fn small_rows(conn: &Connection, version: u8) {
    let columns = match version {
        2 => "namespace, path, mtime, file_size, updated_at",
        3 => "namespace, path, mtime, file_size, updated_at",
        _ => "namespace, path, mtime, file_size, updated_at, last_accessed_at",
    };
    let rows: [(&str, &str, i64, i64); 7] = [
        ("a", "/f/1", 100, 0),
        ("a", "/f/2", 100, 500),
        ("a", "/f/3", 90, 500),
        ("a", "/f/4", 300, 0),
        ("b", "/f/5", 100, 0),
        ("b", "/f/6", 100, 100),
        ("b", "/f/7", 50, 0),
    ];
    for (namespace, path, updated_at, accessed) in rows {
        let sql = if version >= 4 {
            format!("INSERT INTO files ({columns}) VALUES (?1, ?2, 7, 8, ?3, ?4)")
        } else {
            format!("INSERT INTO files ({columns}) VALUES (?1, ?2, 7, 8, ?3)")
        };
        if version >= 4 {
            conn.execute(&sql, params![namespace, path, updated_at, accessed])
        } else {
            conn.execute(&sql, params![namespace, path, updated_at])
        }
        .unwrap();
    }
    conn.execute_batch(
        "INSERT INTO payloads (file_id, content) SELECT id, CAST(id AS BLOB) FROM files;",
    )
    .unwrap();
}

#[derive(Debug, Clone)]
struct Start {
    id: i64,
    namespace: String,
    updated_at: i64,
    last_accessed_at: i64,
}

fn starting_rows(conn: &Connection) -> Vec<Start> {
    let has_accessed = conn
        .prepare("SELECT 1 FROM pragma_table_info('files') WHERE name = 'last_accessed_at'")
        .unwrap()
        .exists([])
        .unwrap();
    let sql = if has_accessed {
        "SELECT id, namespace, updated_at, last_accessed_at FROM files"
    } else {
        "SELECT id, namespace, updated_at, 0 FROM files"
    };
    conn.prepare(sql)
        .unwrap()
        .query_map([], |row| {
            Ok(Start {
                id: row.get(0)?,
                namespace: row.get(1)?,
                updated_at: row.get(2)?,
                last_accessed_at: row.get(3)?,
            })
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// R3 step 1, written as the plain recurrence: per namespace, in order of
/// `(s, updated_at, id)`, `v1 = s * 10^6` and `v_i = max(s_i * 10^6, v_(i-1) + 1)`,
/// where `s` is `last_accessed_at` if above 0 and `updated_at` otherwise.
fn expected_values(rows: &[Start]) -> HashMap<i64, i64> {
    let mut by_namespace: BTreeMap<&str, Vec<&Start>> = BTreeMap::new();
    for row in rows {
        by_namespace.entry(&row.namespace).or_default().push(row);
    }
    let mut expected = HashMap::new();
    for (_, mut group) in by_namespace {
        let seconds = |row: &Start| {
            if row.last_accessed_at > 0 {
                row.last_accessed_at
            } else {
                row.updated_at
            }
        };
        group.sort_by_key(|row| (seconds(row), row.updated_at, row.id));
        let mut previous: Option<i64> = None;
        for row in group {
            let floor = seconds(row) * 1_000_000;
            let value = previous.map_or(floor, |last| floor.max(last + 1));
            expected.insert(row.id, value);
            previous = Some(value);
        }
    }
    expected
}

fn stored_values(conn: &Connection) -> HashMap<i64, i64> {
    conn.prepare("SELECT id, last_used_us FROM files")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn assert_current_and_exact(conn: &Connection) {
    assert!(matches!(
        classifier::classify(conn, classifier::read_user_version(conn).unwrap()).unwrap(),
        SchemaState::Version {
            version: CURRENT_VERSION,
            ..
        }
    ));
}

fn bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

#[test]
fn every_historical_version_reaches_v6_with_the_rule_applied_and_nothing_lost() {
    for version in 2..=5_u8 {
        let mut conn = Connection::open_in_memory().unwrap();
        create_historical(&conn, version);
        small_rows(&conn, version);
        // A public user index must survive untouched.
        if version >= 4 {
            conn.execute_batch("CREATE INDEX lc_user_keep ON files(namespace, path);")
                .unwrap();
        }
        let start = starting_rows(&conn);
        let payloads_before: Vec<(i64, Vec<u8>)> = conn
            .prepare("SELECT file_id, content FROM payloads ORDER BY file_id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();

        let outcome = initialize(&mut conn, true).unwrap();
        assert!(outcome.schema_migration_committed, "v{version}");

        assert_current_and_exact(&conn);
        assert_eq!(stored_values(&conn), expected_values(&start), "v{version}");
        let payloads_after: Vec<(i64, Vec<u8>)> = conn
            .prepare("SELECT file_id, content FROM payloads ORDER BY file_id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(payloads_after, payloads_before, "v{version}");
        let objects: Vec<String> = conn
            .prepare(
                "SELECT name FROM sqlite_schema
                 WHERE type IN ('table', 'index') AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\'
                 ORDER BY name",
            )
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let mut expected_objects = vec!["files", "idx_files_use", "payloads"];
        if version >= 4 {
            expected_objects.insert(2, "lc_user_keep");
        }
        assert_eq!(objects, expected_objects, "v{version}");
    }
}

#[test]
fn the_rule_on_a_known_namespace() {
    // Namespace "a" in order of (s, updated_at, id): /f/4 has s = 300 (never
    // read, written at 300), /f/1 and /f/3 .. worked out by hand below.
    let mut conn = Connection::open_in_memory().unwrap();
    create_historical(&conn, 5);
    small_rows(&conn, 5);
    initialize(&mut conn, true).unwrap();
    let value = |path: &str| -> i64 {
        conn.query_row(
            "SELECT last_used_us FROM files WHERE path = ?1",
            [path],
            |row| row.get(0),
        )
        .unwrap()
    };
    // a: /f/1 s=100 (never read, updated 100); /f/3 s=500; /f/2 s=500
    //    (updated 100, then 90: /f/3 sorts first by updated_at 90); /f/4 s=300.
    //    Order: /f/1 (100), /f/4 (300), /f/3 (500, updated 90), /f/2 (500, updated 100).
    assert_eq!(value("/f/1"), 100_000_000);
    assert_eq!(value("/f/4"), 300_000_000);
    assert_eq!(value("/f/3"), 500_000_000);
    assert_eq!(value("/f/2"), 500_000_001);
    // b: /f/7 s=50, /f/5 s=100 (updated 100), /f/6 s=100 (read at 100, updated 100).
    assert_eq!(value("/f/7"), 50_000_000);
    assert_eq!(value("/f/5"), 100_000_000);
    assert_eq!(value("/f/6"), 100_000_001);
}

fn v5_with_bulk_rows(path: &Path, rows: u32, shared_second: Option<i64>) -> Vec<Start> {
    let conn = Connection::open(path).unwrap();
    create_historical(&conn, 5);
    let (updated, accessed) = match shared_second {
        // every row shares one second, half as a read and half as a write
        Some(second) => (
            format!("{second}"),
            format!("CASE WHEN i % 2 = 0 THEN {second} ELSE 0 END"),
        ),
        None => (
            "1700000000 + (i * 7919) % 5000".to_owned(),
            "CASE WHEN i % 3 = 0 THEN 0 ELSE 1700000000 + (i * 104729) % 9000 END".to_owned(),
        ),
    };
    conn.execute_batch(&format!(
        "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < {rows})
         INSERT INTO files (namespace, path, mtime, file_size, updated_at, last_accessed_at)
         SELECT 'ns' || (i % 3), '/bulk/' || i, i, 1, {updated}, {accessed} FROM n;
         INSERT INTO payloads (file_id, content) SELECT id, CAST(id AS BLOB) FROM files;"
    ))
    .unwrap();
    starting_rows(&conn)
}

#[test]
fn a_hundred_thousand_rows_equal_the_independent_recurrence() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("bulk.sqlite3");
    let start = v5_with_bulk_rows(&path, 100_000, None);
    let mut conn = Connection::open(&path).unwrap();
    assert!(
        initialize(&mut conn, false)
            .unwrap()
            .schema_migration_committed
    );
    assert_current_and_exact(&conn);
    assert_eq!(stored_values(&conn), expected_values(&start));
}

#[test]
fn two_hundred_thousand_rows_in_one_second_stay_strictly_ordered() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("one-second.sqlite3");
    let start = v5_with_bulk_rows(&path, 200_000, Some(1_700_000_000));
    let mut conn = Connection::open(&path).unwrap();
    initialize(&mut conn, false).unwrap();
    let stored = stored_values(&conn);
    assert_eq!(stored, expected_values(&start));
    // More than 10^6 rows in one second cannot collide; 200,000 certainly not.
    let duplicates: i64 = conn
        .query_row(
            "SELECT count(*) FROM (SELECT namespace, last_used_us FROM files
                                   GROUP BY 1, 2 HAVING count(*) > 1)",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(duplicates, 0);
}

#[test]
fn a_migrated_database_and_a_fresh_one_have_the_same_structure() {
    let directory = TempDir::new().unwrap();
    let mut fresh = Connection::open(directory.path().join("fresh.sqlite3")).unwrap();
    initialize(&mut fresh, false).unwrap();
    let mut migrated = Connection::open_in_memory().unwrap();
    create_historical(&migrated, 5);
    small_rows(&migrated, 5);
    initialize(&mut migrated, true).unwrap();

    type Column = (String, String, i64, Option<String>, i64, i64);
    type Index = (String, i64, String, Vec<(i64, Option<String>)>);
    let structure = |conn: &Connection| {
        let columns: Vec<Column> = conn
            .prepare(
                "SELECT name, upper(type), \"notnull\", dflt_value, pk, hidden
                 FROM pragma_table_xinfo('files') ORDER BY cid",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let payload_columns: Vec<Column> = conn
            .prepare(
                "SELECT name, upper(type), \"notnull\", dflt_value, pk, hidden
                 FROM pragma_table_xinfo('payloads') ORDER BY cid",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let indexes: Vec<Index> = conn
            .prepare("SELECT name, \"unique\", origin FROM pragma_index_list('files') ORDER BY name")
            .unwrap()
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .into_iter()
            .map(|(name, unique, origin)| {
                let terms = conn
                    .prepare(&format!(
                        "SELECT seqno, name FROM pragma_index_xinfo('{name}') WHERE key = 1 ORDER BY seqno"
                    ))
                    .unwrap()
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                    .unwrap()
                    .collect::<Result<_, _>>()
                    .unwrap();
                (name, unique, origin, terms)
            })
            .collect();
        (columns, payload_columns, indexes)
    };
    assert_eq!(structure(&fresh), structure(&migrated));
    // …and that structure is the documented one.
    let (columns, _, indexes) = structure(&fresh);
    let names: Vec<&str> = columns.iter().map(|column| column.0.as_str()).collect();
    assert_eq!(
        names,
        [
            "id",
            "namespace",
            "path",
            "mtime",
            "file_size",
            "hash",
            "updated_at",
            "payload_version",
            "last_used_us"
        ]
    );
    let index_names: Vec<&str> = indexes.iter().map(|index| index.0.as_str()).collect();
    assert_eq!(index_names, ["idx_files_use", "sqlite_autoindex_files_1"]);
}

#[test]
fn a_second_open_is_a_no_op() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("twice.sqlite3");
    v5_with_bulk_rows(&path, 50, None);
    {
        let mut conn = Connection::open(&path).unwrap();
        assert!(
            initialize(&mut conn, false)
                .unwrap()
                .schema_migration_committed
        );
    }
    let once = bytes(&path);
    let mut conn = Connection::open(&path).unwrap();
    let changes = conn.total_changes();
    let outcome = initialize(&mut conn, false).unwrap();
    assert!(!outcome.schema_migration_committed);
    assert_eq!(conn.total_changes(), changes);
    drop(conn);
    assert_eq!(bytes(&path), once, "a second open must not write the file");
}

const V5_FAILPOINTS: [MigrationPoint; 9] = [
    MigrationPoint::AfterAuthoritativeClassification,
    MigrationPoint::AfterV6ColumnAdded,
    MigrationPoint::AfterV6Backfill,
    MigrationPoint::AfterV6IndexesDropped,
    MigrationPoint::AfterV6ColumnDropped,
    MigrationPoint::AfterV6UseIndexCreated,
    MigrationPoint::AfterFinalUserVersionWrite,
    MigrationPoint::AfterFinalPostconditions,
    MigrationPoint::ImmediatelyBeforeCommit,
];

#[test]
fn every_failpoint_of_the_v5_migration_leaves_the_file_byte_identical() {
    for failpoint in V5_FAILPOINTS {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("failpoint.sqlite3");
        let start = v5_with_bulk_rows(&path, 300, None);
        let before = bytes(&path);

        let mut conn = Connection::open(&path).unwrap();
        let error = initialize_with_hook(&mut conn, false, &mut |point| {
            if point == failpoint {
                Err(LocalFileCacheError::DatabaseState(format!(
                    "synthetic failure at {point:?}"
                )))
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        assert!(error.to_string().contains("synthetic failure"), "{error}");
        drop(conn);
        assert_eq!(
            bytes(&path),
            before,
            "not byte-identical after {failpoint:?}"
        );

        // The retry reaches the same state a clean migration reaches.
        let mut conn = Connection::open(&path).unwrap();
        assert!(
            initialize(&mut conn, false)
                .unwrap()
                .schema_migration_committed
        );
        assert_eq!(stored_values(&conn), expected_values(&start));
    }
}

#[test]
fn a_panic_after_the_backfill_rolls_back_to_the_same_bytes() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("panic.sqlite3");
    v5_with_bulk_rows(&path, 300, None);
    let before = bytes(&path);
    let mut conn = Connection::open(&path).unwrap();
    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = initialize_with_hook(&mut conn, false, &mut |point| {
            if point == MigrationPoint::AfterV6ColumnDropped {
                panic!("synthetic panic in the middle of the v5 migration");
            }
            Ok(())
        });
    }));
    assert!(unwind.is_err());
    drop(conn);
    assert_eq!(bytes(&path), before);
}

/// F1: a damaged or hostile v5 file whose recency cannot be held by the use
/// order fails the migration and leaves the file exactly as it was.
#[test]
fn a_hostile_v5_recency_fails_the_migration_and_leaves_the_file_unchanged() {
    for (column, hostile) in [
        ("last_accessed_at", i64::MAX),
        // s * 10^6 is just over 2^62: an integer, but not below the bound
        ("last_accessed_at", 4_611_686_018_428),
        // s * 10^6 overflows i64 and SQLite turns it into a float
        ("last_accessed_at", 9_223_372_036_855),
        // a never-read row is placed by `updated_at`
        ("updated_at", i64::MAX),
        ("updated_at", i64::MIN),
        // RFC 026 R3: the bottom of the range is refused too
        ("updated_at", -1),
    ] {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("hostile.sqlite3");
        v5_with_bulk_rows(&path, 40, None);
        let set = if column == "updated_at" {
            // the rule reads `updated_at` only when `last_accessed_at` is 0
            "UPDATE files SET last_accessed_at = 0, updated_at = ?1 WHERE id = 7"
        } else {
            "UPDATE files SET last_accessed_at = ?1 WHERE id = 7"
        };
        Connection::open(&path)
            .unwrap()
            .execute(set, [hostile])
            .unwrap();
        let before = bytes(&path);

        let mut conn = Connection::open(&path).unwrap();
        let error = initialize(&mut conn, false).unwrap_err();
        let message = error.to_string();
        assert!(
            matches!(error, LocalFileCacheError::DatabaseState(_)),
            "{column} = {hostile}: {message}"
        );
        assert!(
            message.contains("outside the range"),
            "{column} = {hostile}: {message}"
        );
        drop(conn);
        assert_eq!(bytes(&path), before, "{column} = {hostile}");
        assert_eq!(
            Connection::open(&path)
                .unwrap()
                .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            5
        );
    }
}

/// A negative `last_accessed_at` is not a use time at all: the rule treats the
/// row as never read and places it by `updated_at`, so it migrates.
#[test]
fn a_negative_last_accessed_at_is_treated_as_never_read() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("negative.sqlite3");
    v5_with_bulk_rows(&path, 40, None);
    Connection::open(&path)
        .unwrap()
        .execute("UPDATE files SET last_accessed_at = -5 WHERE id = 7", [])
        .unwrap();
    let mut conn = Connection::open(&path).unwrap();
    initialize(&mut conn, false).unwrap();
    let updated_at: i64 = conn
        .query_row("SELECT updated_at FROM files WHERE id = 7", [], |row| {
            row.get(0)
        })
        .unwrap();
    let stored: i64 = conn
        .query_row("SELECT last_used_us FROM files WHERE id = 7", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(stored >= updated_at * 1_000_000);
}

/// RFC 026: a read-only open cannot migrate, so a v5 database reports
/// `SchemaNotCurrent` and is left exactly as it was.
#[test]
fn a_read_only_open_of_a_v5_database_reports_schema_not_current_and_changes_nothing() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("v5-readonly.sqlite3");
    v5_with_bulk_rows(&path, 20, None);
    let before = bytes(&path);

    let mut reader =
        Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let error = super::super::validate_read_only(&mut reader).unwrap_err();
    assert!(
        matches!(error, LocalFileCacheError::SchemaNotCurrent),
        "{error:?}"
    );
    drop(reader);
    assert_eq!(bytes(&path), before);
}

/// RFC 026 R3: one range, `[0, 2^62)`, for a stored use value and an imported
/// one. A row placed below 0 refuses the migration and leaves the file as it
/// was; fixing the row lets the retry succeed.
#[test]
fn a_negative_recency_refuses_the_migration_until_the_row_is_fixed() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("negative-updated-at.sqlite3");
    v5_with_bulk_rows(&path, 40, None);
    Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE files SET last_accessed_at = 0, updated_at = -5 WHERE id = 7",
            [],
        )
        .unwrap();
    let before = bytes(&path);

    let mut conn = Connection::open(&path).unwrap();
    let error = initialize(&mut conn, false).unwrap_err();
    assert!(
        matches!(&error, LocalFileCacheError::DatabaseState(m) if m.contains("outside the range")),
        "{error:?}"
    );
    drop(conn);
    assert_eq!(bytes(&path), before);

    Connection::open(&path)
        .unwrap()
        .execute("UPDATE files SET updated_at = 100 WHERE id = 7", [])
        .unwrap();
    let start = {
        let conn = Connection::open(&path).unwrap();
        starting_rows(&conn)
    };
    let mut conn = Connection::open(&path).unwrap();
    assert!(
        initialize(&mut conn, false)
            .unwrap()
            .schema_migration_committed
    );
    assert_eq!(stored_values(&conn), expected_values(&start));
    let lowest: i64 = conn
        .query_row("SELECT min(last_used_us) FROM files", [], |row| row.get(0))
        .unwrap();
    assert!(lowest >= 0);
}

/// The top of the range is accepted exactly: the largest second whose
/// microseconds are below 2^62.
#[test]
fn the_largest_representable_recency_migrates() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("edge.sqlite3");
    v5_with_bulk_rows(&path, 10, None);
    let largest = (1_i64 << 62) / 1_000_000;
    Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE files SET last_accessed_at = ?1 WHERE id = 3",
            [largest],
        )
        .unwrap();
    let mut conn = Connection::open(&path).unwrap();
    initialize(&mut conn, false).unwrap();
    let stored: i64 = conn
        .query_row("SELECT last_used_us FROM files WHERE id = 3", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(stored, largest * 1_000_000);
}
