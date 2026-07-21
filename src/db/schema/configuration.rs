//! SQLite durability and post-initialization runtime configuration.

use rusqlite::Connection;

use crate::{JournalMode, LocalFileCacheError, SynchronousMode};

pub(super) fn prepare_file_migration(conn: &Connection) -> Result<(), LocalFileCacheError> {
    let journal = observed_journal_mode(conn)?;
    if !matches!(journal.as_str(), "delete" | "truncate" | "persist" | "wal") {
        return Err(LocalFileCacheError::UnsupportedFeature(format!(
            "database migration requires an existing rollback-capable or WAL journal; observed_journal_mode={journal}; database was not modified"
        )));
    }

    conn.execute_batch("PRAGMA synchronous = FULL;")?;
    let synchronous = observed_synchronous(conn)?;
    if synchronous != "full" {
        return Err(LocalFileCacheError::UnsupportedFeature(format!(
            "database migration requires synchronous=FULL; observed_synchronous={synchronous}; database was not modified"
        )));
    }
    Ok(())
}

pub(super) fn apply_runtime_configuration(
    conn: &Connection,
    requested_journal: JournalMode,
    requested_synchronous: SynchronousMode,
    schema_migration_committed: bool,
) -> Result<(), LocalFileCacheError> {
    if let Err(cause) = set_synchronous(conn, requested_synchronous) {
        return Err(configuration_error(
            conn,
            requested_journal,
            requested_synchronous,
            schema_migration_committed,
            &cause.to_string(),
        ));
    }
    let observed_synchronous = observed_synchronous(conn).unwrap_or_else(|_| "unknown".into());
    if observed_synchronous != synchronous_name(requested_synchronous) {
        return Err(configuration_error(
            conn,
            requested_journal,
            requested_synchronous,
            schema_migration_committed,
            "synchronous verification mismatch",
        ));
    }

    let observed_journal = match set_journal_mode(conn, requested_journal) {
        Ok(value) => value,
        Err(cause) => {
            return Err(configuration_error(
                conn,
                requested_journal,
                requested_synchronous,
                schema_migration_committed,
                &cause.to_string(),
            ));
        }
    };
    if observed_journal != journal_name(requested_journal) {
        return Err(configuration_error(
            conn,
            requested_journal,
            requested_synchronous,
            schema_migration_committed,
            "journal_mode verification mismatch",
        ));
    }
    Ok(())
}

fn set_synchronous(conn: &Connection, mode: SynchronousMode) -> Result<(), rusqlite::Error> {
    conn.execute_batch(match mode {
        SynchronousMode::Off => "PRAGMA synchronous = OFF;",
        SynchronousMode::Normal => "PRAGMA synchronous = NORMAL;",
        SynchronousMode::Full => "PRAGMA synchronous = FULL;",
        SynchronousMode::Extra => "PRAGMA synchronous = EXTRA;",
    })
}

fn set_journal_mode(conn: &Connection, mode: JournalMode) -> Result<String, rusqlite::Error> {
    conn.query_row(
        match mode {
            JournalMode::Wal => "PRAGMA journal_mode = WAL;",
            JournalMode::Delete => "PRAGMA journal_mode = DELETE;",
            JournalMode::Memory => "PRAGMA journal_mode = MEMORY;",
        },
        [],
        |row| row.get::<_, String>(0),
    )
    .map(|value| value.to_ascii_lowercase())
}

fn observed_journal_mode(conn: &Connection) -> Result<String, rusqlite::Error> {
    conn.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
        .map(|value| value.to_ascii_lowercase())
}

fn observed_synchronous(conn: &Connection) -> Result<String, rusqlite::Error> {
    conn.query_row("PRAGMA synchronous", [], |row| row.get::<_, i64>(0))
        .map(|value| match value {
            0 => "off".into(),
            1 => "normal".into(),
            2 => "full".into(),
            3 => "extra".into(),
            other => format!("unknown({other})"),
        })
}

fn journal_name(mode: JournalMode) -> &'static str {
    match mode {
        JournalMode::Wal => "wal",
        JournalMode::Delete => "delete",
        JournalMode::Memory => "memory",
    }
}

fn synchronous_name(mode: SynchronousMode) -> &'static str {
    match mode {
        SynchronousMode::Off => "off",
        SynchronousMode::Normal => "normal",
        SynchronousMode::Full => "full",
        SynchronousMode::Extra => "extra",
    }
}

fn configuration_error(
    conn: &Connection,
    requested_journal: JournalMode,
    requested_synchronous: SynchronousMode,
    schema_migration_committed: bool,
    cause: &str,
) -> LocalFileCacheError {
    let observed_journal = observed_journal_mode(conn).unwrap_or_else(|_| "unknown".into());
    let observed_synchronous = observed_synchronous(conn).unwrap_or_else(|_| "unknown".into());
    LocalFileCacheError::UnsupportedFeature(format!(
        "database runtime configuration failed: schema_migration_committed={schema_migration_committed}; requested_synchronous={}; observed_synchronous={observed_synchronous}; requested_journal_mode={}; observed_journal_mode={observed_journal}; cause={cause}",
        synchronous_name(requested_synchronous),
        journal_name(requested_journal),
    ))
}
