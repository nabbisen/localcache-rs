//! RFC 010 transactional schema transformations and preservation checks.

use rusqlite::{Transaction, params};

use crate::error::LocalFileCacheError;

use super::classifier::{self, SchemaState};

const MTIME_SECONDS_MIN: i64 = -9_223_372_036;
const MTIME_SECONDS_MAX: i64 = 9_223_372_036;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MigrationPoint {
    AfterPreliminaryVersionRead,
    AfterAuthoritativeClassification,
    AfterShadowFilesCreation,
    AfterShadowPayloadsCreation,
    AfterParentCopy,
    AfterPayloadCopy,
    AfterBidirectionalEquivalence,
    AfterOldPayloadsDrop,
    AfterOldFilesDrop,
    AfterNewFilesRename,
    AfterNewPayloadsRename,
    AfterSequenceRestoration,
    AfterV2ShapeValidation,
    AfterV2ToV3,
    AfterV3ToV4,
    AfterV4NumericSnapshot,
    AfterV4ConversionEquivalence,
    AfterFinalUserVersionWrite,
    AfterFinalPostconditions,
    ImmediatelyBeforeCommit,
}

pub(super) struct StartingSnapshot {
    active: bool,
    captured_user_indexes: bool,
}

impl StartingSnapshot {
    pub(super) fn capture(
        transaction: &Transaction<'_>,
        state: SchemaState,
    ) -> Result<Self, LocalFileCacheError> {
        let version = match state {
            SchemaState::Fresh => {
                return Ok(Self {
                    active: false,
                    captured_user_indexes: false,
                });
            }
            SchemaState::Version { version, .. } => version,
        };

        transaction.execute_batch(
            "CREATE TEMP TABLE __localcache_rfc010_start_rows (
                id               INTEGER PRIMARY KEY,
                namespace        TEXT NOT NULL,
                path             TEXT NOT NULL,
                old_mtime        INTEGER NOT NULL,
                file_size        INTEGER NOT NULL,
                hash             TEXT,
                updated_at       INTEGER NOT NULL,
                payload_version  INTEGER NOT NULL,
                last_accessed_at INTEGER NOT NULL,
                has_payload      INTEGER NOT NULL,
                content          BLOB,
                encoding         TEXT
            );",
        )?;

        transaction.execute_batch(match version {
            1 => {
                "INSERT INTO temp.__localcache_rfc010_start_rows
                 SELECT f.id, 'default', f.path, f.mtime, f.file_size, f.hash,
                        f.updated_at, 0, 0, p.file_id IS NOT NULL, p.content,
                        CASE WHEN p.file_id IS NOT NULL THEN 'raw' END
                 FROM main.files AS f
                 LEFT JOIN main.payloads AS p ON p.file_id = f.id;"
            }
            2 => {
                "INSERT INTO temp.__localcache_rfc010_start_rows
                 SELECT f.id, f.namespace, f.path, f.mtime, f.file_size, f.hash,
                        f.updated_at, 0, 0, p.file_id IS NOT NULL, p.content,
                        CASE WHEN p.file_id IS NOT NULL THEN 'raw' END
                 FROM main.files AS f
                 LEFT JOIN main.payloads AS p ON p.file_id = f.id;"
            }
            3 => {
                "INSERT INTO temp.__localcache_rfc010_start_rows
                 SELECT f.id, f.namespace, f.path, f.mtime, f.file_size, f.hash,
                        f.updated_at, f.payload_version, 0,
                        p.file_id IS NOT NULL, p.content, p.encoding
                 FROM main.files AS f
                 LEFT JOIN main.payloads AS p ON p.file_id = f.id;"
            }
            4 => {
                "INSERT INTO temp.__localcache_rfc010_start_rows
                 SELECT f.id, f.namespace, f.path, f.mtime, f.file_size, f.hash,
                        f.updated_at, f.payload_version, f.last_accessed_at,
                        p.file_id IS NOT NULL, p.content, p.encoding
                 FROM main.files AS f
                 LEFT JOIN main.payloads AS p ON p.file_id = f.id;"
            }
            _ => unreachable!("only versions 1 through 4 are migrated"),
        })?;

        let captured_user_indexes = version == 4;
        if captured_user_indexes {
            transaction.execute_batch(
                "CREATE TEMP TABLE __localcache_rfc010_start_indexes (
                    name TEXT PRIMARY KEY,
                    sql  TEXT NOT NULL
                 );
                 INSERT INTO temp.__localcache_rfc010_start_indexes(name, sql)
                 SELECT name, sql
                 FROM main.sqlite_schema
                 WHERE type = 'index'
                   AND substr(name, 1, 8) = 'lc_user_';",
            )?;
        }

        Ok(Self {
            active: true,
            captured_user_indexes,
        })
    }

    pub(super) fn validate_and_remove(
        &mut self,
        transaction: &Transaction<'_>,
    ) -> Result<(), LocalFileCacheError> {
        if !self.active {
            return Ok(());
        }

        let start_count: i64 = transaction.query_row(
            "SELECT count(*) FROM temp.__localcache_rfc010_start_rows",
            [],
            |row| row.get(0),
        )?;
        let final_count: i64 =
            transaction.query_row("SELECT count(*) FROM main.files", [], |row| row.get(0))?;
        let mismatch: i64 = transaction.query_row(
            "SELECT
               EXISTS(
                 SELECT id, namespace, path, old_mtime * 1000000000,
                        file_size, hash, updated_at, payload_version,
                        last_accessed_at, has_payload, content, encoding
                 FROM temp.__localcache_rfc010_start_rows
                 EXCEPT
                 SELECT f.id, f.namespace, f.path, f.mtime, f.file_size,
                        f.hash, f.updated_at, f.payload_version,
                        f.last_accessed_at, p.file_id IS NOT NULL, p.content,
                        p.encoding
                 FROM main.files AS f
                 LEFT JOIN main.payloads AS p ON p.file_id = f.id
               )
               OR EXISTS(
                 SELECT f.id, f.namespace, f.path, f.mtime, f.file_size,
                        f.hash, f.updated_at, f.payload_version,
                        f.last_accessed_at, p.file_id IS NOT NULL, p.content,
                        p.encoding
                 FROM main.files AS f
                 LEFT JOIN main.payloads AS p ON p.file_id = f.id
                 EXCEPT
                 SELECT id, namespace, path, old_mtime * 1000000000,
                        file_size, hash, updated_at, payload_version,
                        last_accessed_at, has_payload, content, encoding
                 FROM temp.__localcache_rfc010_start_rows
               )",
            [],
            |row| row.get(0),
        )?;
        if start_count != final_count || mismatch != 0 {
            return Err(migration_invariant(
                "starting row/payload equivalence failed",
            ));
        }

        if self.captured_user_indexes {
            let index_mismatch: i64 = transaction.query_row(
                "SELECT
                   EXISTS(
                     SELECT name, sql FROM temp.__localcache_rfc010_start_indexes
                     EXCEPT
                     SELECT name, sql FROM main.sqlite_schema
                     WHERE type = 'index' AND substr(name, 1, 8) = 'lc_user_'
                   )
                   OR EXISTS(
                     SELECT name, sql FROM main.sqlite_schema
                     WHERE type = 'index' AND substr(name, 1, 8) = 'lc_user_'
                     EXCEPT
                     SELECT name, sql FROM temp.__localcache_rfc010_start_indexes
                   )",
                [],
                |row| row.get(0),
            )?;
            if index_mismatch != 0 {
                return Err(migration_invariant(
                    "released public index definition changed during migration",
                ));
            }
            transaction.execute_batch("DROP TABLE temp.__localcache_rfc010_start_indexes;")?;
        }
        transaction.execute_batch("DROP TABLE temp.__localcache_rfc010_start_rows;")?;
        self.active = false;
        Ok(())
    }
}

pub(super) fn create_fresh(transaction: &Transaction<'_>) -> Result<(), LocalFileCacheError> {
    transaction.execute_batch(
        "CREATE TABLE files (
            id                INTEGER PRIMARY KEY AUTOINCREMENT,
            namespace         TEXT    NOT NULL DEFAULT 'default',
            path              TEXT    NOT NULL,
            mtime             INTEGER NOT NULL,
            file_size         INTEGER NOT NULL,
            hash              TEXT,
            updated_at        INTEGER NOT NULL,
            payload_version   INTEGER NOT NULL DEFAULT 0,
            last_accessed_at  INTEGER NOT NULL DEFAULT 0,
            UNIQUE(namespace, path)
        );
        CREATE TABLE payloads (
            file_id  INTEGER PRIMARY KEY,
            content  BLOB    NOT NULL,
            encoding TEXT    NOT NULL DEFAULT 'raw',
            FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
        );
        CREATE INDEX idx_files_namespace_path ON files(namespace, path);
        CREATE INDEX idx_files_lru
            ON files(namespace, last_accessed_at, updated_at);",
    )?;
    Ok(())
}

pub(super) fn migrate_v1_to_v2(
    transaction: &Transaction<'_>,
    files_high_water: i64,
    hook: &mut dyn FnMut(MigrationPoint) -> Result<(), LocalFileCacheError>,
) -> Result<(), LocalFileCacheError> {
    transaction.execute_batch(
        "CREATE TABLE __localcache_rfc010_files_v2 (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            namespace   TEXT    NOT NULL DEFAULT 'default',
            path        TEXT    NOT NULL,
            mtime       INTEGER NOT NULL,
            file_size   INTEGER NOT NULL,
            hash        TEXT,
            updated_at  INTEGER NOT NULL,
            UNIQUE(namespace, path)
        );",
    )?;
    hook(MigrationPoint::AfterShadowFilesCreation)?;

    transaction.execute_batch(
        "CREATE TABLE __localcache_rfc010_payloads_v2 (
            file_id INTEGER PRIMARY KEY,
            content BLOB NOT NULL,
            FOREIGN KEY(file_id)
                REFERENCES __localcache_rfc010_files_v2(id) ON DELETE CASCADE
        );",
    )?;
    hook(MigrationPoint::AfterShadowPayloadsCreation)?;

    transaction.execute_batch(
        "INSERT INTO __localcache_rfc010_files_v2
            (id, namespace, path, mtime, file_size, hash, updated_at)
         SELECT id, 'default', path, mtime, file_size, hash, updated_at
         FROM files;",
    )?;
    hook(MigrationPoint::AfterParentCopy)?;

    transaction.execute_batch(
        "INSERT INTO __localcache_rfc010_payloads_v2(file_id, content)
         SELECT file_id, content FROM payloads;",
    )?;
    hook(MigrationPoint::AfterPayloadCopy)?;

    validate_v1_shadow_equivalence(transaction)?;
    hook(MigrationPoint::AfterBidirectionalEquivalence)?;

    transaction.execute_batch("DROP TABLE payloads;")?;
    hook(MigrationPoint::AfterOldPayloadsDrop)?;
    transaction.execute_batch("DROP TABLE files;")?;
    hook(MigrationPoint::AfterOldFilesDrop)?;
    transaction.execute_batch("ALTER TABLE __localcache_rfc010_files_v2 RENAME TO files;")?;
    hook(MigrationPoint::AfterNewFilesRename)?;
    transaction.execute_batch("ALTER TABLE __localcache_rfc010_payloads_v2 RENAME TO payloads;")?;
    hook(MigrationPoint::AfterNewPayloadsRename)?;

    transaction.execute_batch(
        "DELETE FROM sqlite_sequence
         WHERE name IN ('files', '__localcache_rfc010_files_v2');",
    )?;
    let file_count: i64 =
        transaction.query_row("SELECT count(*) FROM files", [], |row| row.get(0))?;
    if file_count != 0 || files_high_water != 0 {
        transaction.execute(
            "INSERT INTO sqlite_sequence(name, seq) VALUES ('files', ?1)",
            [files_high_water],
        )?;
    }
    hook(MigrationPoint::AfterSequenceRestoration)?;

    transaction
        .execute_batch("CREATE INDEX idx_files_namespace_path ON files(namespace, path);")?;
    classifier::validate_effective_version(transaction, 2)?;
    hook(MigrationPoint::AfterV2ShapeValidation)?;
    Ok(())
}

pub(super) fn migrate_v2_to_v3(
    transaction: &Transaction<'_>,
    hook: &mut dyn FnMut(MigrationPoint) -> Result<(), LocalFileCacheError>,
) -> Result<(), LocalFileCacheError> {
    transaction.execute_batch(
        "ALTER TABLE files
            ADD COLUMN payload_version INTEGER NOT NULL DEFAULT 0;
         ALTER TABLE payloads
            ADD COLUMN encoding TEXT NOT NULL DEFAULT 'raw';",
    )?;
    classifier::validate_effective_version(transaction, 3)?;
    hook(MigrationPoint::AfterV2ToV3)?;
    Ok(())
}

pub(super) fn migrate_v3_to_v4(
    transaction: &Transaction<'_>,
    hook: &mut dyn FnMut(MigrationPoint) -> Result<(), LocalFileCacheError>,
) -> Result<(), LocalFileCacheError> {
    transaction.execute_batch(
        "ALTER TABLE files
            ADD COLUMN last_accessed_at INTEGER NOT NULL DEFAULT 0;
         CREATE INDEX idx_files_lru
            ON files(namespace, last_accessed_at, updated_at);",
    )?;
    classifier::validate_effective_version(transaction, 4)?;
    hook(MigrationPoint::AfterV3ToV4)?;
    Ok(())
}

pub(super) fn migrate_v4_to_v5(
    transaction: &Transaction<'_>,
    hook: &mut dyn FnMut(MigrationPoint) -> Result<(), LocalFileCacheError>,
) -> Result<(), LocalFileCacheError> {
    let invalid_storage: i64 = transaction.query_row(
        "SELECT count(*) FROM files WHERE typeof(mtime) <> 'integer'",
        [],
        |row| row.get(0),
    )?;
    if invalid_storage != 0 {
        return Err(migration_invariant(
            "v4 mtime must use SQLite INTEGER storage",
        ));
    }
    let outside_range: i64 = transaction.query_row(
        "SELECT count(*) FROM files WHERE mtime < ?1 OR mtime > ?2",
        params![MTIME_SECONDS_MIN, MTIME_SECONDS_MAX],
        |row| row.get(0),
    )?;
    if outside_range != 0 {
        return Err(migration_invariant(
            "v4 mtime is outside the safe seconds range",
        ));
    }

    transaction.execute_batch(
        "CREATE TEMP TABLE __localcache_rfc010_mtime_v4 (
            id        INTEGER PRIMARY KEY,
            old_mtime INTEGER NOT NULL
         );
         INSERT INTO temp.__localcache_rfc010_mtime_v4(id, old_mtime)
         SELECT id, mtime FROM main.files;",
    )?;
    hook(MigrationPoint::AfterV4NumericSnapshot)?;

    transaction.execute_batch("UPDATE files SET mtime = mtime * 1000000000;")?;
    validate_v4_conversion(transaction)?;
    hook(MigrationPoint::AfterV4ConversionEquivalence)?;
    transaction.execute_batch("DROP TABLE temp.__localcache_rfc010_mtime_v4;")?;
    Ok(())
}

pub(super) fn validate_final_postconditions(
    transaction: &Transaction<'_>,
) -> Result<(), LocalFileCacheError> {
    let version = classifier::read_user_version(transaction)?;
    match classifier::classify(transaction, version)? {
        SchemaState::Version { version: 5, .. } => Ok(()),
        _ => Err(migration_invariant("final schema is not exact version 5")),
    }
}

fn validate_v1_shadow_equivalence(
    transaction: &Transaction<'_>,
) -> Result<(), LocalFileCacheError> {
    let files_mismatch: i64 = transaction.query_row(
        "SELECT
           (SELECT count(*) FROM files) <> (SELECT count(*) FROM __localcache_rfc010_files_v2)
           OR EXISTS(
             SELECT id, 'default', path, mtime, file_size, hash, updated_at FROM files
             EXCEPT
             SELECT id, namespace, path, mtime, file_size, hash, updated_at
             FROM __localcache_rfc010_files_v2
           )
           OR EXISTS(
             SELECT id, namespace, path, mtime, file_size, hash, updated_at
             FROM __localcache_rfc010_files_v2
             EXCEPT
             SELECT id, 'default', path, mtime, file_size, hash, updated_at FROM files
           )",
        [],
        |row| row.get(0),
    )?;
    let payload_mismatch: i64 = transaction.query_row(
        "SELECT
           (SELECT count(*) FROM payloads) <>
             (SELECT count(*) FROM __localcache_rfc010_payloads_v2)
           OR EXISTS(
             SELECT file_id, content FROM payloads
             EXCEPT
             SELECT file_id, content FROM __localcache_rfc010_payloads_v2
           )
           OR EXISTS(
             SELECT file_id, content FROM __localcache_rfc010_payloads_v2
             EXCEPT
             SELECT file_id, content FROM payloads
           )",
        [],
        |row| row.get(0),
    )?;
    let foreign_key_violations: i64 =
        transaction.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })?;
    if files_mismatch != 0 || payload_mismatch != 0 || foreign_key_violations != 0 {
        return Err(migration_invariant(
            "v1 shadow relations are not bidirectionally equivalent",
        ));
    }
    Ok(())
}

fn validate_v4_conversion(transaction: &Transaction<'_>) -> Result<(), LocalFileCacheError> {
    let mismatch: i64 = transaction.query_row(
        "SELECT
           (SELECT count(*) FROM temp.__localcache_rfc010_mtime_v4) <>
             (SELECT count(*) FROM main.files)
           OR EXISTS(
             SELECT id, old_mtime * 1000000000
             FROM temp.__localcache_rfc010_mtime_v4
             EXCEPT
             SELECT id, mtime FROM main.files WHERE typeof(mtime) = 'integer'
           )
           OR EXISTS(
             SELECT id, mtime FROM main.files WHERE typeof(mtime) = 'integer'
             EXCEPT
             SELECT id, old_mtime * 1000000000
             FROM temp.__localcache_rfc010_mtime_v4
           )
           OR EXISTS(SELECT 1 FROM main.files WHERE typeof(mtime) <> 'integer')",
        [],
        |row| row.get(0),
    )?;
    if mismatch != 0 {
        return Err(migration_invariant(
            "v4 mtime conversion equivalence failed",
        ));
    }
    Ok(())
}

fn migration_invariant(reason: &str) -> LocalFileCacheError {
    LocalFileCacheError::UnsupportedFeature(format!(
        "database migration precondition or postcondition failed: {reason}; transaction will be rolled back"
    ))
}
