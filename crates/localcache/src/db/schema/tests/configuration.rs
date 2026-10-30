use std::time::Duration;

use rusqlite::Connection;
use tempfile::TempDir;

use crate::{JournalMode, LocalFileCacheError, SynchronousMode};

use super::super::{
    CURRENT_VERSION, apply_runtime_configuration, configuration as runtime_configuration,
    initialize,
};
use super::helpers::{open_v1, snapshot_v1};

#[test]
fn unsafe_existing_file_journal_modes_reject_before_migration() {
    for pragma in [
        "PRAGMA journal_mode = MEMORY;",
        "PRAGMA journal_mode = OFF;",
    ] {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("unsafe-journal.sqlite3");
        let mut conn = open_v1(&path);
        conn.query_row(pragma, [], |row| row.get::<_, String>(0))
            .unwrap();
        let before = snapshot_v1(&conn);

        let error = initialize(&mut conn, false).unwrap_err();
        assert!(
            matches!(error, LocalFileCacheError::DatabaseState(_)),
            "{error:?}"
        );
        let error = error.to_string();
        assert!(
            error.starts_with(
                "database state: migration requires an existing rollback-capable or WAL journal"
            ),
            "{error}"
        );
        assert!(error.contains("database was not modified"), "{error}");
        assert_eq!(snapshot_v1(&conn), before);
    }
}

#[test]
fn migration_preparation_accepts_wal_and_forces_full_synchronous() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("wal-migration.sqlite3");
    let mut conn = open_v1(&path);
    assert_eq!(
        conn.query_row("PRAGMA journal_mode = WAL", [], |row| row
            .get::<_, String>(0))
            .unwrap(),
        "wal"
    );
    conn.execute_batch("PRAGMA synchronous = OFF;").unwrap();

    runtime_configuration::prepare_file_migration(&conn).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA synchronous", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert!(
        initialize(&mut conn, false)
            .unwrap()
            .schema_migration_committed
    );
    assert_eq!(
        conn.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "wal"
    );
}

#[test]
fn every_runtime_option_is_applied_only_after_atomic_initialization() {
    for journal in [JournalMode::Wal, JournalMode::Delete, JournalMode::Memory] {
        for synchronous in [
            SynchronousMode::Off,
            SynchronousMode::Normal,
            SynchronousMode::Full,
            SynchronousMode::Extra,
        ] {
            let directory = TempDir::new().unwrap();
            let path = directory.path().join("options.sqlite3");
            let mut conn = Connection::open(&path).unwrap();
            let outcome = initialize(&mut conn, false).unwrap();
            assert!(outcome.schema_migration_committed);

            apply_runtime_configuration(
                &conn,
                journal,
                synchronous,
                outcome.schema_migration_committed,
            )
            .unwrap();

            let observed_journal: String = conn
                .query_row("PRAGMA journal_mode", [], |row| row.get(0))
                .unwrap();
            let expected_journal = match journal {
                JournalMode::Wal => "wal",
                JournalMode::Delete => "delete",
                JournalMode::Memory => "memory",
            };
            assert_eq!(observed_journal, expected_journal);

            let observed_synchronous: i64 = conn
                .query_row("PRAGMA synchronous", [], |row| row.get(0))
                .unwrap();
            let expected_synchronous = match synchronous {
                SynchronousMode::Off => 0,
                SynchronousMode::Normal => 1,
                SynchronousMode::Full => 2,
                SynchronousMode::Extra => 3,
            };
            assert_eq!(observed_synchronous, expected_synchronous);
        }
    }
}

#[test]
fn post_commit_configuration_error_reports_stable_recovery_fields() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("post-commit-config.sqlite3");
    let mut conn = open_v1(&path);
    let outcome = initialize(&mut conn, false).unwrap();
    assert!(outcome.schema_migration_committed);
    conn.busy_timeout(Duration::ZERO).unwrap();
    conn.execute_batch("BEGIN IMMEDIATE;").unwrap();

    let error = apply_runtime_configuration(
        &conn,
        JournalMode::Wal,
        SynchronousMode::Off,
        outcome.schema_migration_committed,
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.starts_with("database state: runtime configuration failed:"),
        "{error}"
    );
    assert!(error.contains("schema_migration_committed=true"), "{error}");
    assert!(error.contains("requested_synchronous=off"), "{error}");
    assert!(error.contains("observed_synchronous="), "{error}");
    assert!(error.contains("requested_journal_mode=wal"), "{error}");
    assert!(error.contains("observed_journal_mode="), "{error}");
    conn.execute_batch("ROLLBACK;").unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        i64::from(CURRENT_VERSION)
    );
}

#[test]
fn no_migration_configuration_error_reports_false_and_observed_state() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("validated-config.sqlite3");
    let mut conn = Connection::open(&path).unwrap();
    assert!(
        initialize(&mut conn, false)
            .unwrap()
            .schema_migration_committed
    );
    let outcome = initialize(&mut conn, false).unwrap();
    assert!(!outcome.schema_migration_committed);
    conn.execute_batch("BEGIN IMMEDIATE;").unwrap();

    let error = apply_runtime_configuration(
        &conn,
        JournalMode::Memory,
        SynchronousMode::Off,
        outcome.schema_migration_committed,
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.starts_with("database state: runtime configuration failed:"),
        "{error}"
    );
    assert!(
        error.contains("schema_migration_committed=false"),
        "{error}"
    );
    assert!(error.contains("requested_journal_mode=memory"), "{error}");
    assert!(error.contains("observed_journal_mode="), "{error}");
    conn.execute_batch("ROLLBACK;").unwrap();
}

#[test]
fn journal_failure_after_synchronous_change_reports_committed_state() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("journal-config-failure.sqlite3");
    let mut conn = open_v1(&path);
    let outcome = initialize(&mut conn, false).unwrap();
    assert!(outcome.schema_migration_committed);
    conn.busy_timeout(Duration::ZERO).unwrap();

    let reader = Connection::open(&path).unwrap();
    reader.execute_batch("BEGIN;").unwrap();
    reader
        .query_row("SELECT count(*) FROM files", [], |row| row.get::<_, i64>(0))
        .unwrap();
    let error = apply_runtime_configuration(
        &conn,
        JournalMode::Wal,
        SynchronousMode::Off,
        outcome.schema_migration_committed,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("schema_migration_committed=true"), "{error}");
    assert!(error.contains("requested_synchronous=off"), "{error}");
    assert!(error.contains("observed_synchronous=off"), "{error}");
    assert!(error.contains("requested_journal_mode=wal"), "{error}");
    assert!(error.contains("observed_journal_mode=delete"), "{error}");
    reader.execute_batch("ROLLBACK;").unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        i64::from(CURRENT_VERSION)
    );
}

/// RFC 025 R4: SQLite not honouring a setting, and a failed migration
/// postcondition, are `DatabaseState` errors. The load-bearing clauses stay.
#[test]
fn foreign_key_enforcement_that_cannot_be_enabled_is_a_database_state_error() {
    let mut conn = Connection::open_in_memory().unwrap();
    // `PRAGMA foreign_keys` is a no-op inside a transaction.
    conn.execute_batch("PRAGMA foreign_keys = OFF; BEGIN;")
        .unwrap();
    let error = super::super::validate_read_only(&mut conn).unwrap_err();
    assert!(
        matches!(error, LocalFileCacheError::DatabaseState(_)),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "database state: SQLite foreign-key enforcement could not be enabled"
    );
}

#[test]
fn a_failed_migration_postcondition_keeps_the_rollback_clause() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("postcondition.sqlite3");
    let conn = open_v1(&path);
    let transaction = conn.unchecked_transaction().unwrap();
    // A version-1 database is not the final schema.
    let error = super::super::migration::validate_final_postconditions(&transaction).unwrap_err();
    assert!(
        matches!(error, LocalFileCacheError::DatabaseState(_)),
        "{error:?}"
    );
    let text = error.to_string();
    assert!(
        text.starts_with("database state: migration precondition or postcondition failed: "),
        "{text}"
    );
    assert!(
        text.ends_with("; transaction will be rolled back"),
        "{text}"
    );
}
