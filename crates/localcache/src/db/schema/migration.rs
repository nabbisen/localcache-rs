//! RFC 010 transactional schema transformations and preservation checks.

use rusqlite::{Transaction, params};

use crate::error::{LocalFileCacheError, SqliteResultExt};

use super::CURRENT_VERSION;
use super::classifier::{self, SchemaState};

const MTIME_SECONDS_MIN: i64 = -9_223_372_036;
const MTIME_SECONDS_MAX: i64 = 9_223_372_036;

/// The largest last-use second whose microseconds stay below 2^62, the bound of
/// a use value (RFC 026 R1). The import enforces the same range.
const RECENCY_SECONDS_MAX: i64 = (1 << 62) / 1_000_000;

/// `mtime` was seconds before v5 and is nanoseconds from v5 on.
const NANOSECONDS_PER_SECOND: i64 = 1_000_000_000;

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
    AfterV6ColumnAdded,
    AfterV6Backfill,
    AfterV6IndexesDropped,
    AfterV6ColumnDropped,
    AfterV6UseIndexCreated,
    AfterFinalUserVersionWrite,
    AfterFinalPostconditions,
    ImmediatelyBeforeCommit,
}

pub(super) struct StartingSnapshot {
    active: bool,
    captured_user_indexes: bool,
    /// What the snapshot's `old_mtime` must be multiplied by to equal the
    /// migrated `mtime`: a billion for versions that stored seconds, one for
    /// version 5, which already stores nanoseconds.
    mtime_factor: i64,
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
                    mtime_factor: 1,
                });
            }
            SchemaState::Version { version, .. } => version,
        };

        transaction
            .execute_batch(
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
            )
            .db()?;

        transaction
            .execute_batch(match version {
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
                4 | 5 => {
                    "INSERT INTO temp.__localcache_rfc010_start_rows
                 SELECT f.id, f.namespace, f.path, f.mtime, f.file_size, f.hash,
                        f.updated_at, f.payload_version, f.last_accessed_at,
                        p.file_id IS NOT NULL, p.content, p.encoding
                 FROM main.files AS f
                 LEFT JOIN main.payloads AS p ON p.file_id = f.id;"
                }
                _ => unreachable!("only versions 1 through 5 are migrated"),
            })
            .db()?;

        let captured_user_indexes = version >= 4;
        if captured_user_indexes {
            transaction
                .execute_batch(
                    "CREATE TEMP TABLE __localcache_rfc010_start_indexes (
                    name TEXT PRIMARY KEY,
                    sql  TEXT NOT NULL
                 );
                 INSERT INTO temp.__localcache_rfc010_start_indexes(name, sql)
                 SELECT name, sql
                 FROM main.sqlite_schema
                 WHERE type = 'index'
                   AND substr(name, 1, 8) = 'lc_user_';",
                )
                .db()?;
        }

        Ok(Self {
            active: true,
            captured_user_indexes,
            mtime_factor: if version >= 5 {
                1
            } else {
                NANOSECONDS_PER_SECOND
            },
        })
    }

    pub(super) fn validate_and_remove(
        &mut self,
        transaction: &Transaction<'_>,
    ) -> Result<(), LocalFileCacheError> {
        if !self.active {
            return Ok(());
        }

        let start_count: i64 = transaction
            .query_row(
                "SELECT count(*) FROM temp.__localcache_rfc010_start_rows",
                [],
                |row| row.get(0),
            )
            .db()?;
        let final_count: i64 = transaction
            .query_row("SELECT count(*) FROM main.files", [], |row| row.get(0))
            .db()?;
        // `last_accessed_at` no longer exists in the final table: the use
        // order that replaces it is checked against the snapshot below.
        let mismatch: i64 = transaction
            .query_row(
                "SELECT
               EXISTS(
                 SELECT id, namespace, path, old_mtime * ?1,
                        file_size, hash, updated_at, payload_version,
                        has_payload, content, encoding
                 FROM temp.__localcache_rfc010_start_rows
                 EXCEPT
                 SELECT f.id, f.namespace, f.path, f.mtime, f.file_size,
                        f.hash, f.updated_at, f.payload_version,
                        p.file_id IS NOT NULL, p.content,
                        p.encoding
                 FROM main.files AS f
                 LEFT JOIN main.payloads AS p ON p.file_id = f.id
               )
               OR EXISTS(
                 SELECT f.id, f.namespace, f.path, f.mtime, f.file_size,
                        f.hash, f.updated_at, f.payload_version,
                        p.file_id IS NOT NULL, p.content,
                        p.encoding
                 FROM main.files AS f
                 LEFT JOIN main.payloads AS p ON p.file_id = f.id
                 EXCEPT
                 SELECT id, namespace, path, old_mtime * ?1,
                        file_size, hash, updated_at, payload_version,
                        has_payload, content, encoding
                 FROM temp.__localcache_rfc010_start_rows
               )",
                [self.mtime_factor],
                |row| row.get(0),
            )
            .db()?;
        if start_count != final_count || mismatch != 0 {
            return Err(migration_invariant(
                "starting row/payload equivalence failed",
            ));
        }
        validate_use_order(transaction)?;

        if self.captured_user_indexes {
            let index_mismatch: i64 = transaction
                .query_row(
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
                )
                .db()?;
            if index_mismatch != 0 {
                return Err(migration_invariant(
                    "released public index definition changed during migration",
                ));
            }
            transaction
                .execute_batch("DROP TABLE temp.__localcache_rfc010_start_indexes;")
                .db()?;
        }
        transaction
            .execute_batch("DROP TABLE temp.__localcache_rfc010_start_rows;")
            .db()?;
        self.active = false;
        Ok(())
    }
}

pub(super) fn create_fresh(transaction: &Transaction<'_>) -> Result<(), LocalFileCacheError> {
    transaction
        .execute_batch(
            "CREATE TABLE files (
            id                INTEGER PRIMARY KEY AUTOINCREMENT,
            namespace         TEXT    NOT NULL DEFAULT 'default',
            path              TEXT    NOT NULL,
            mtime             INTEGER NOT NULL,
            file_size         INTEGER NOT NULL,
            hash              TEXT,
            updated_at        INTEGER NOT NULL,
            payload_version   INTEGER NOT NULL DEFAULT 0,
            last_used_us      INTEGER NOT NULL DEFAULT 0
                CHECK (typeof(last_used_us) = 'integer'
                       AND last_used_us < 4611686018427387904),
            UNIQUE(namespace, path)
        );
        CREATE TABLE payloads (
            file_id  INTEGER PRIMARY KEY,
            content  BLOB    NOT NULL,
            encoding TEXT    NOT NULL DEFAULT 'raw',
            FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
        );
        CREATE INDEX idx_files_use ON files(namespace, last_used_us);",
        )
        .db()?;
    Ok(())
}

pub(super) fn migrate_v1_to_v2(
    transaction: &Transaction<'_>,
    files_high_water: i64,
    hook: &mut dyn FnMut(MigrationPoint) -> Result<(), LocalFileCacheError>,
) -> Result<(), LocalFileCacheError> {
    transaction
        .execute_batch(
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
        )
        .db()?;
    hook(MigrationPoint::AfterShadowFilesCreation)?;

    transaction
        .execute_batch(
            "CREATE TABLE __localcache_rfc010_payloads_v2 (
            file_id INTEGER PRIMARY KEY,
            content BLOB NOT NULL,
            FOREIGN KEY(file_id)
                REFERENCES __localcache_rfc010_files_v2(id) ON DELETE CASCADE
        );",
        )
        .db()?;
    hook(MigrationPoint::AfterShadowPayloadsCreation)?;

    transaction
        .execute_batch(
            "INSERT INTO __localcache_rfc010_files_v2
            (id, namespace, path, mtime, file_size, hash, updated_at)
         SELECT id, 'default', path, mtime, file_size, hash, updated_at
         FROM files;",
        )
        .db()?;
    hook(MigrationPoint::AfterParentCopy)?;

    transaction
        .execute_batch(
            "INSERT INTO __localcache_rfc010_payloads_v2(file_id, content)
         SELECT file_id, content FROM payloads;",
        )
        .db()?;
    hook(MigrationPoint::AfterPayloadCopy)?;

    validate_v1_shadow_equivalence(transaction)?;
    hook(MigrationPoint::AfterBidirectionalEquivalence)?;

    transaction.execute_batch("DROP TABLE payloads;").db()?;
    hook(MigrationPoint::AfterOldPayloadsDrop)?;
    transaction.execute_batch("DROP TABLE files;").db()?;
    hook(MigrationPoint::AfterOldFilesDrop)?;
    transaction
        .execute_batch("ALTER TABLE __localcache_rfc010_files_v2 RENAME TO files;")
        .db()?;
    hook(MigrationPoint::AfterNewFilesRename)?;
    transaction
        .execute_batch("ALTER TABLE __localcache_rfc010_payloads_v2 RENAME TO payloads;")
        .db()?;
    hook(MigrationPoint::AfterNewPayloadsRename)?;

    transaction
        .execute_batch(
            "DELETE FROM sqlite_sequence
         WHERE name IN ('files', '__localcache_rfc010_files_v2');",
        )
        .db()?;
    let file_count: i64 = transaction
        .query_row("SELECT count(*) FROM files", [], |row| row.get(0))
        .db()?;
    if file_count != 0 || files_high_water != 0 {
        transaction
            .execute(
                "INSERT INTO sqlite_sequence(name, seq) VALUES ('files', ?1)",
                [files_high_water],
            )
            .db()?;
    }
    hook(MigrationPoint::AfterSequenceRestoration)?;

    transaction
        .execute_batch("CREATE INDEX idx_files_namespace_path ON files(namespace, path);")
        .db()?;
    classifier::validate_effective_version(transaction, 2)?;
    hook(MigrationPoint::AfterV2ShapeValidation)?;
    Ok(())
}

pub(super) fn migrate_v2_to_v3(
    transaction: &Transaction<'_>,
    hook: &mut dyn FnMut(MigrationPoint) -> Result<(), LocalFileCacheError>,
) -> Result<(), LocalFileCacheError> {
    transaction
        .execute_batch(
            "ALTER TABLE files
            ADD COLUMN payload_version INTEGER NOT NULL DEFAULT 0;
         ALTER TABLE payloads
            ADD COLUMN encoding TEXT NOT NULL DEFAULT 'raw';",
        )
        .db()?;
    classifier::validate_effective_version(transaction, 3)?;
    hook(MigrationPoint::AfterV2ToV3)?;
    Ok(())
}

pub(super) fn migrate_v3_to_v4(
    transaction: &Transaction<'_>,
    hook: &mut dyn FnMut(MigrationPoint) -> Result<(), LocalFileCacheError>,
) -> Result<(), LocalFileCacheError> {
    transaction
        .execute_batch(
            "ALTER TABLE files
            ADD COLUMN last_accessed_at INTEGER NOT NULL DEFAULT 0;
         CREATE INDEX idx_files_lru
            ON files(namespace, last_accessed_at, updated_at);",
        )
        .db()?;
    classifier::validate_effective_version(transaction, 4)?;
    hook(MigrationPoint::AfterV3ToV4)?;
    Ok(())
}

pub(super) fn migrate_v4_to_v5(
    transaction: &Transaction<'_>,
    hook: &mut dyn FnMut(MigrationPoint) -> Result<(), LocalFileCacheError>,
) -> Result<(), LocalFileCacheError> {
    let invalid_storage: i64 = transaction
        .query_row(
            "SELECT count(*) FROM files WHERE typeof(mtime) <> 'integer'",
            [],
            |row| row.get(0),
        )
        .db()?;
    if invalid_storage != 0 {
        return Err(migration_invariant(
            "v4 mtime must use SQLite INTEGER storage",
        ));
    }
    let outside_range: i64 = transaction
        .query_row(
            "SELECT count(*) FROM files WHERE mtime < ?1 OR mtime > ?2",
            params![MTIME_SECONDS_MIN, MTIME_SECONDS_MAX],
            |row| row.get(0),
        )
        .db()?;
    if outside_range != 0 {
        return Err(migration_invariant(
            "v4 mtime is outside the safe seconds range",
        ));
    }

    transaction
        .execute_batch(
            "CREATE TEMP TABLE __localcache_rfc010_mtime_v4 (
            id        INTEGER PRIMARY KEY,
            old_mtime INTEGER NOT NULL
         );
         INSERT INTO temp.__localcache_rfc010_mtime_v4(id, old_mtime)
         SELECT id, mtime FROM main.files;",
        )
        .db()?;
    hook(MigrationPoint::AfterV4NumericSnapshot)?;

    transaction
        .execute_batch("UPDATE files SET mtime = mtime * 1000000000;")
        .db()?;
    validate_v4_conversion(transaction)?;
    hook(MigrationPoint::AfterV4ConversionEquivalence)?;
    transaction
        .execute_batch("DROP TABLE temp.__localcache_rfc010_mtime_v4;")
        .db()?;
    Ok(())
}

/// RFC 026 R3: schema v5 to v6. The use order replaces `last_accessed_at` and
/// `idx_files_lru`, and the index that duplicates the `UNIQUE(namespace, path)`
/// autoindex is dropped.
///
/// The steps run in the order SQLite forces: the column is added and backfilled
/// first; both indexes go before the old column, because `DROP COLUMN` refuses
/// a column an index uses; the new index is created last. The backfill is **one
/// statement**: `v_i = max(s_i * 10^6, v_(i-1) + 1)` has the closed form
/// `v_i = i + max_(j <= i)(s_j * 10^6 - j)`, a running maximum, where `s` is
/// `last_accessed_at` when above 0 and `updated_at` otherwise, and the order is
/// `(s, updated_at, id)` within a namespace. A value that does not fit (a
/// damaged or hostile file) trips the column's `CHECK`, and the whole migration
/// fails and rolls back.
///
/// Postconditions (the rows and payloads unchanged, the use order equal to the
/// rule recomputed from the starting snapshot) are `StartingSnapshot`'s, run by
/// the caller after `user_version` is written.
pub(super) fn migrate_v5_to_v6(
    transaction: &Transaction<'_>,
    hook: &mut dyn FnMut(MigrationPoint) -> Result<(), LocalFileCacheError>,
) -> Result<(), LocalFileCacheError> {
    transaction
        .execute_batch(
            "ALTER TABLE files
            ADD COLUMN last_used_us INTEGER NOT NULL DEFAULT 0
                CHECK (typeof(last_used_us) = 'integer'
                       AND last_used_us < 4611686018427387904);",
        )
        .db()?;
    hook(MigrationPoint::AfterV6ColumnAdded)?;

    // Every stored use value is in [0, 2^62), the range an import enforces, so
    // what a database holds an export can carry back (RFC 026 R3). A row whose
    // last-use second is outside it (a clock set before 1970, or a damaged
    // file) refuses the migration, whichever end it is out of. This is a read,
    // made before the backfill, so the message is the same for both ends.
    let out_of_range: i64 = transaction
        .query_row(
            "SELECT count(*) FROM files
             WHERE (CASE WHEN last_accessed_at > 0
                         THEN last_accessed_at ELSE updated_at END) NOT BETWEEN 0 AND ?1",
            [RECENCY_SECONDS_MAX],
            |row| row.get(0),
        )
        .db()?;
    if out_of_range != 0 {
        return Err(migration_invariant(RECENCY_OUT_OF_RANGE));
    }

    transaction
        .execute_batch(
            "WITH base AS (
                SELECT id, namespace, updated_at,
                       CASE WHEN last_accessed_at > 0
                            THEN last_accessed_at ELSE updated_at END AS s
                FROM files),
            ranked AS (
                SELECT id, namespace, s,
                       ROW_NUMBER() OVER (PARTITION BY namespace
                                          ORDER BY s, updated_at, id) AS rn
                FROM base),
            run AS (
                SELECT id, rn,
                       MAX(s * 1000000 - rn) OVER (PARTITION BY namespace
                                                   ORDER BY rn
                                                   ROWS BETWEEN UNBOUNDED PRECEDING
                                                            AND CURRENT ROW) AS m
                FROM ranked)
         UPDATE files SET last_used_us = run.rn + run.m
         FROM run WHERE files.id = run.id;",
        )
        .map_err(|error| match error {
            rusqlite::Error::SqliteFailure(failure, _)
                if failure.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_CHECK =>
            {
                migration_invariant(RECENCY_OUT_OF_RANGE)
            }
            other => LocalFileCacheError::Database(crate::error::DatabaseError::from_sqlite(other)),
        })?;
    hook(MigrationPoint::AfterV6Backfill)?;

    transaction
        .execute_batch("DROP INDEX idx_files_lru; DROP INDEX idx_files_namespace_path;")
        .db()?;
    hook(MigrationPoint::AfterV6IndexesDropped)?;

    transaction
        .execute_batch("ALTER TABLE files DROP COLUMN last_accessed_at;")
        .db()?;
    hook(MigrationPoint::AfterV6ColumnDropped)?;

    transaction
        .execute_batch("CREATE INDEX idx_files_use ON files(namespace, last_used_us);")
        .db()?;
    classifier::validate_effective_version(transaction, 6)?;
    hook(MigrationPoint::AfterV6UseIndexCreated)?;
    Ok(())
}

/// R3 step 3: every stored use value equals the rule recomputed, row by row,
/// from the starting snapshot by the recurrence itself (not by the window form
/// the migration used), is an integer, and is below 2^62. Strictly increasing
/// per namespace in the rule's order follows from the recurrence.
fn validate_use_order(transaction: &Transaction<'_>) -> Result<(), LocalFileCacheError> {
    const LIMIT: i64 = 1 << 62;
    let mut statement = transaction
        .prepare(
            "SELECT s.namespace,
                    CASE WHEN s.last_accessed_at > 0
                         THEN s.last_accessed_at ELSE s.updated_at END AS seconds,
                    f.last_used_us
             FROM temp.__localcache_rfc010_start_rows AS s
             JOIN main.files AS f ON f.id = s.id
             ORDER BY s.namespace, seconds, s.updated_at, s.id",
        )
        .db()?;
    let mut rows = statement.query([]).db()?;
    let mut namespace = String::new();
    let mut previous: Option<i64> = None;
    while let Some(row) = rows.next().db()? {
        let row_namespace: String = row.get(0).db()?;
        let seconds: i64 = row.get(1).db()?;
        let stored: rusqlite::types::Value = row.get(2).db()?;
        let rusqlite::types::Value::Integer(stored) = stored else {
            return Err(migration_invariant("a stored use value is not an integer"));
        };
        if row_namespace != namespace {
            namespace = row_namespace;
            previous = None;
        }
        let expected = match (seconds.checked_mul(1_000_000), previous) {
            (Some(floor), Some(last)) => floor.max(last.saturating_add(1)),
            (Some(floor), None) => floor,
            (None, _) => {
                return Err(migration_invariant(
                    "a stored recency overflows the use order",
                ));
            }
        };
        if !(0..LIMIT).contains(&stored) || stored != expected {
            return Err(migration_invariant("the use order does not match the rule"));
        }
        previous = Some(stored);
    }
    Ok(())
}

pub(super) fn validate_final_postconditions(
    transaction: &Transaction<'_>,
) -> Result<(), LocalFileCacheError> {
    let version = classifier::read_user_version(transaction)?;
    match classifier::classify(transaction, version)? {
        SchemaState::Version {
            version: CURRENT_VERSION,
            ..
        } => Ok(()),
        _ => Err(migration_invariant(&format!(
            "final schema is not exact version {CURRENT_VERSION}"
        ))),
    }
}

fn validate_v1_shadow_equivalence(
    transaction: &Transaction<'_>,
) -> Result<(), LocalFileCacheError> {
    let files_mismatch: i64 = transaction
        .query_row(
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
        )
        .db()?;
    let payload_mismatch: i64 = transaction
        .query_row(
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
        )
        .db()?;
    let foreign_key_violations: i64 = transaction
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .db()?;
    if files_mismatch != 0 || payload_mismatch != 0 || foreign_key_violations != 0 {
        return Err(migration_invariant(
            "v1 shadow relations are not bidirectionally equivalent",
        ));
    }
    Ok(())
}

fn validate_v4_conversion(transaction: &Transaction<'_>) -> Result<(), LocalFileCacheError> {
    let mismatch: i64 = transaction
        .query_row(
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
        )
        .db()?;
    if mismatch != 0 {
        return Err(migration_invariant(
            "v4 mtime conversion equivalence failed",
        ));
    }
    Ok(())
}

const RECENCY_OUT_OF_RANGE: &str = "a stored recency is outside the range the use order can hold";

fn migration_invariant(reason: &str) -> LocalFileCacheError {
    LocalFileCacheError::DatabaseState(format!(
        "migration precondition or postcondition failed: {reason}; transaction will be rolled back"
    ))
}
