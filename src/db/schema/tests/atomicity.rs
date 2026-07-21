use std::{panic::AssertUnwindSafe, time::Duration};

use rusqlite::{Connection, ErrorCode, limits::Limit};
use tempfile::TempDir;

use crate::LocalFileCacheError;

use super::super::{
    classifier::{self, SchemaState},
    initialize, initialize_with_hook,
    migration::MigrationPoint,
};
use super::helpers::{assert_no_migration_objects, open_v1, snapshot_v1};

const REQUIRED_FAILPOINTS: [MigrationPoint; 19] = [
    MigrationPoint::AfterAuthoritativeClassification,
    MigrationPoint::AfterShadowFilesCreation,
    MigrationPoint::AfterShadowPayloadsCreation,
    MigrationPoint::AfterParentCopy,
    MigrationPoint::AfterPayloadCopy,
    MigrationPoint::AfterBidirectionalEquivalence,
    MigrationPoint::AfterOldPayloadsDrop,
    MigrationPoint::AfterOldFilesDrop,
    MigrationPoint::AfterNewFilesRename,
    MigrationPoint::AfterNewPayloadsRename,
    MigrationPoint::AfterSequenceRestoration,
    MigrationPoint::AfterV2ShapeValidation,
    MigrationPoint::AfterV2ToV3,
    MigrationPoint::AfterV3ToV4,
    MigrationPoint::AfterV4NumericSnapshot,
    MigrationPoint::AfterV4ConversionEquivalence,
    MigrationPoint::AfterFinalUserVersionWrite,
    MigrationPoint::AfterFinalPostconditions,
    MigrationPoint::ImmediatelyBeforeCommit,
];

#[test]
fn every_required_failpoint_rolls_back_and_retry_succeeds() {
    for failpoint in REQUIRED_FAILPOINTS {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("failpoint.sqlite3");
        let mut conn = open_v1(&path);
        let before = snapshot_v1(&conn);

        let error = initialize_with_hook(&mut conn, false, &mut |point| {
            if point == failpoint {
                Err(LocalFileCacheError::UnsupportedFeature(format!(
                    "synthetic RFC 010 failure at {point:?}"
                )))
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        assert!(error.to_string().contains("synthetic RFC 010 failure"));
        assert_eq!(
            snapshot_v1(&conn),
            before,
            "rollback failed at {failpoint:?}"
        );
        assert_no_migration_objects(&conn);

        let outcome = initialize(&mut conn, false).unwrap();
        assert!(outcome.schema_migration_committed);
        assert!(matches!(
            classifier::classify(&conn, classifier::read_user_version(&conn).unwrap()).unwrap(),
            SchemaState::Version { version: 5, .. }
        ));
        assert_eq!(
            conn.query_row(
                "SELECT seq FROM sqlite_sequence WHERE name = 'files'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            17
        );
        let payloads = conn
            .prepare("SELECT file_id, content FROM payloads ORDER BY file_id")
            .unwrap()
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(payloads, [(1, vec![0, 1, 255]), (3, vec![])]);
        assert_no_migration_objects(&conn);
    }
}

#[test]
fn panic_after_old_parent_drop_rolls_back_semantically() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("panic.sqlite3");
    let mut conn = open_v1(&path);
    let before = snapshot_v1(&conn);

    let unwind = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let _ = initialize_with_hook(&mut conn, false, &mut |point| {
            if point == MigrationPoint::AfterOldFilesDrop {
                panic!("synthetic destructive migration panic");
            }
            Ok(())
        });
    }));
    assert!(unwind.is_err());
    assert_eq!(snapshot_v1(&conn), before);
    assert_no_migration_objects(&conn);
    assert!(
        initialize(&mut conn, false)
            .unwrap()
            .schema_migration_committed
    );
}

#[test]
fn competing_immediate_writer_returns_busy_without_retry_or_mutation() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("busy.sqlite3");
    let mut opener = open_v1(&path);
    opener.busy_timeout(Duration::ZERO).unwrap();
    let before = snapshot_v1(&opener);

    let blocker = Connection::open(&path).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE;").unwrap();
    let error = initialize(&mut opener, false).unwrap_err();
    match error {
        LocalFileCacheError::Database(rusqlite::Error::SqliteFailure(code, _)) => {
            assert_eq!(code.code, ErrorCode::DatabaseBusy);
        }
        other => panic!("expected SQLITE_BUSY, got {other}"),
    }
    blocker.execute_batch("ROLLBACK;").unwrap();
    assert_eq!(snapshot_v1(&opener), before);
    assert_no_migration_objects(&opener);
}

#[test]
fn authoritative_read_supersedes_stale_preliminary_version() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("stale.sqlite3");
    let mut first = open_v1(&path);
    let first_changes = first.total_changes();
    let mut second_migrated = false;

    let outcome = initialize_with_hook(&mut first, false, &mut |point| {
        if point == MigrationPoint::AfterPreliminaryVersionRead && !second_migrated {
            let mut second = Connection::open(&path)?;
            let second_outcome = initialize(&mut second, false)?;
            assert!(second_outcome.schema_migration_committed);
            second_migrated = true;
        }
        Ok(())
    })
    .unwrap();

    assert!(second_migrated);
    assert!(!outcome.schema_migration_committed);
    assert_eq!(first.total_changes(), first_changes);
    assert_eq!(
        classifier::read_user_version(&first).unwrap(),
        5,
        "the authoritative transaction must observe the concurrent migration"
    );
    assert_no_migration_objects(&first);
}

#[test]
fn representative_larger_v1_migration_preserves_all_relations() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("larger.sqlite3");
    let mut conn = open_v1(&path);
    conn.execute_batch(
        "WITH RECURSIVE ids(id) AS (
            VALUES(100) UNION ALL SELECT id + 1 FROM ids WHERE id < 1099
         )
         INSERT INTO files(id, path, mtime, file_size, hash, updated_at)
         SELECT id, '/fixture/large-' || id, id, id * 2, NULL, id * 3 FROM ids;
         INSERT INTO payloads(file_id, content)
         SELECT id, CAST('payload-' || id AS BLOB) FROM files
         WHERE id >= 100 AND id % 2 = 0;
         UPDATE sqlite_sequence SET seq = 1500 WHERE name = 'files';",
    )
    .unwrap();
    let expected_payload_bytes: i64 = conn
        .query_row("SELECT sum(length(content)) FROM payloads", [], |row| {
            row.get(0)
        })
        .unwrap();

    assert!(
        initialize(&mut conn, false)
            .unwrap()
            .schema_migration_committed
    );
    assert_eq!(
        conn.query_row("SELECT count(*) FROM files", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        1003
    );
    assert_eq!(
        conn.query_row("SELECT count(*) FROM payloads", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        502
    );
    assert_eq!(
        conn.query_row("SELECT sum(length(content)) FROM payloads", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        expected_payload_bytes
    );
    assert_eq!(
        conn.query_row(
            "SELECT seq FROM sqlite_sequence WHERE name = 'files'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1500
    );
}

#[test]
fn temporary_resource_limit_surfaces_as_rollback_error_and_retry_succeeds() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("resource-limit.sqlite3");
    let mut conn = open_v1(&path);
    conn.execute(
        "UPDATE payloads SET content = ?1 WHERE file_id = 1",
        [vec![0xA5_u8; 4096]],
    )
    .unwrap();
    let before = snapshot_v1(&conn);
    let previous = conn.set_limit(Limit::SQLITE_LIMIT_LENGTH, 1024).unwrap();

    assert!(initialize(&mut conn, false).is_err());
    conn.set_limit(Limit::SQLITE_LIMIT_LENGTH, previous)
        .unwrap();
    assert_eq!(snapshot_v1(&conn), before);
    assert_no_migration_objects(&conn);
    assert!(
        initialize(&mut conn, false)
            .unwrap()
            .schema_migration_committed
    );
    assert_eq!(
        conn.query_row(
            "SELECT length(content) FROM payloads WHERE file_id = 1",
            [],
            |row| { row.get::<_, i64>(0) }
        )
        .unwrap(),
        4096
    );
}

#[test]
fn current_v5_reopens_are_idempotent_and_write_nothing() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("idempotent.sqlite3");
    let mut conn = Connection::open(&path).unwrap();
    assert!(
        initialize(&mut conn, false)
            .unwrap()
            .schema_migration_committed
    );
    conn.execute_batch(
        "INSERT INTO files
            (namespace, path, mtime, file_size, hash, updated_at,
             payload_version, last_accessed_at)
         VALUES ('default', '/fixture/current', 11, 12, NULL, 13, 0, 14);
         INSERT INTO payloads(file_id, content, encoding)
         VALUES (1, X'0001FF', 'raw');",
    )
    .unwrap();
    let changes = conn.total_changes();
    let schema: Vec<(String, String, Option<String>)> = conn
        .prepare("SELECT type, name, sql FROM sqlite_schema ORDER BY type, name")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    for _ in 0..2 {
        let outcome = initialize(&mut conn, false).unwrap();
        assert!(!outcome.schema_migration_committed);
        assert_eq!(conn.total_changes(), changes);
        let after: Vec<(String, String, Option<String>)> = conn
            .prepare("SELECT type, name, sql FROM sqlite_schema ORDER BY type, name")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(after, schema);
    }
}

#[test]
fn fresh_creation_is_atomic_when_failure_occurs_before_commit() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("fresh-rollback.sqlite3");
    let mut conn = Connection::open(&path).unwrap();
    let error = initialize_with_hook(&mut conn, false, &mut |point| {
        if point == MigrationPoint::ImmediatelyBeforeCommit {
            Err(LocalFileCacheError::UnsupportedFeature(
                "synthetic fresh failure".into(),
            ))
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert!(error.to_string().contains("synthetic fresh failure"));
    assert_eq!(classifier::read_user_version(&conn).unwrap(), 0);
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM sqlite_schema
             WHERE name NOT LIKE 'sqlite_%'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0
    );
    assert_no_migration_objects(&conn);
}
