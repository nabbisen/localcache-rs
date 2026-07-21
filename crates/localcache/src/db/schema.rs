//! Database schema initialization and RFC 010 migration coordination.

use rusqlite::{Connection, TransactionBehavior};

use crate::{JournalMode, SynchronousMode, error::LocalFileCacheError};

mod classifier;
mod configuration;
mod migration;

use classifier::SchemaState;
use migration::{MigrationPoint, StartingSnapshot};

const CURRENT_VERSION: i64 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InitializationOutcome {
    pub(crate) schema_migration_committed: bool,
}

/// Initialize or migrate a writable database.
///
/// Existing v5 databases use a consistent read transaction. Fresh databases
/// and versions 1 through 4 use one typed `Immediate` transaction containing
/// authoritative classification, all schema work, postconditions, and commit.
pub(crate) fn initialize(
    conn: &mut Connection,
    is_memory: bool,
) -> Result<InitializationOutcome, LocalFileCacheError> {
    initialize_with_hook(conn, is_memory, &mut |_| Ok(()))
}

fn initialize_with_hook(
    conn: &mut Connection,
    is_memory: bool,
    hook: &mut dyn FnMut(MigrationPoint) -> Result<(), LocalFileCacheError>,
) -> Result<InitializationOutcome, LocalFileCacheError> {
    enable_foreign_keys(conn)?;

    let preliminary_version = classifier::read_user_version(conn)?;
    hook(MigrationPoint::AfterPreliminaryVersionRead)?;

    if preliminary_version == CURRENT_VERSION {
        validate_current_read_snapshot(conn)?;
        return Ok(InitializationOutcome {
            schema_migration_committed: false,
        });
    }
    if !(0..CURRENT_VERSION).contains(&preliminary_version) {
        // Reuse the classifier's stable fail-closed error contract.
        classifier::classify(conn, preliminary_version)?;
        unreachable!("unsupported version unexpectedly classified")
    }

    if !is_memory {
        configuration::prepare_file_migration(conn)?;
    }

    let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let authoritative_version = classifier::read_user_version(&transaction)?;
    let state = classifier::classify(&transaction, authoritative_version)?;
    hook(MigrationPoint::AfterAuthoritativeClassification)?;

    if matches!(state, SchemaState::Version { version: 5, .. }) {
        hook(MigrationPoint::ImmediatelyBeforeCommit)?;
        transaction.commit()?;
        return Ok(InitializationOutcome {
            schema_migration_committed: false,
        });
    }

    let mut snapshot = StartingSnapshot::capture(&transaction, state)?;
    match state {
        SchemaState::Fresh => migration::create_fresh(&transaction)?,
        SchemaState::Version {
            version: 1,
            files_high_water,
        } => {
            migration::migrate_v1_to_v2(&transaction, files_high_water, hook)?;
            migration::migrate_v2_to_v3(&transaction, hook)?;
            migration::migrate_v3_to_v4(&transaction, hook)?;
            migration::migrate_v4_to_v5(&transaction, hook)?;
        }
        SchemaState::Version { version: 2, .. } => {
            migration::migrate_v2_to_v3(&transaction, hook)?;
            migration::migrate_v3_to_v4(&transaction, hook)?;
            migration::migrate_v4_to_v5(&transaction, hook)?;
        }
        SchemaState::Version { version: 3, .. } => {
            migration::migrate_v3_to_v4(&transaction, hook)?;
            migration::migrate_v4_to_v5(&transaction, hook)?;
        }
        SchemaState::Version { version: 4, .. } => {
            migration::migrate_v4_to_v5(&transaction, hook)?;
        }
        SchemaState::Version { .. } => {
            unreachable!("classifier returned an unsupported migration state")
        }
    }

    transaction.execute_batch("PRAGMA user_version = 5;")?;
    hook(MigrationPoint::AfterFinalUserVersionWrite)?;

    snapshot.validate_and_remove(&transaction)?;
    migration::validate_final_postconditions(&transaction)?;
    hook(MigrationPoint::AfterFinalPostconditions)?;
    hook(MigrationPoint::ImmediatelyBeforeCommit)?;
    transaction.commit()?;

    Ok(InitializationOutcome {
        schema_migration_committed: true,
    })
}

fn validate_current_read_snapshot(conn: &mut Connection) -> Result<(), LocalFileCacheError> {
    let transaction = conn.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let version = classifier::read_user_version(&transaction)?;
    match classifier::classify(&transaction, version)? {
        SchemaState::Version { version: 5, .. } => transaction.commit()?,
        _ => {
            return Err(LocalFileCacheError::UnsupportedFeature(
                "database changed while current schema was being validated; database was not modified"
                    .into(),
            ));
        }
    }
    Ok(())
}

pub(crate) fn apply_runtime_configuration(
    conn: &Connection,
    journal_mode: JournalMode,
    synchronous: SynchronousMode,
    schema_migration_committed: bool,
) -> Result<(), LocalFileCacheError> {
    configuration::apply_runtime_configuration(
        conn,
        journal_mode,
        synchronous,
        schema_migration_committed,
    )
}

pub(crate) fn enable_foreign_keys(conn: &Connection) -> Result<(), LocalFileCacheError> {
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    let enabled: i64 = conn.query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
    if enabled != 1 {
        return Err(LocalFileCacheError::UnsupportedFeature(
            "SQLite foreign-key enforcement could not be enabled".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
