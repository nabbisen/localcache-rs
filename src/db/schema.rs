//! Database schema initialisation and migration.
//!
//! Schema versioning uses SQLite's built-in `PRAGMA user_version`.
//!
//! | user_version | Description                          |
//! |--------------|--------------------------------------|
//! | 0            | Empty / pre-migration                |
//! | 1            | v0.1 schema (no namespace column)    |
//! | 2            | v0.2 schema (namespace column added) |
//!
//! No structural changes were needed for v0.3; partial-hash values are
//! distinguished by a `"partial:"` prefix in the existing `hash` column.

use rusqlite::Connection;

use crate::error::LocalFileCacheError;

const CURRENT_VERSION: u32 = 2;

/// Apply the current schema to `conn`, running any necessary migrations.
///
/// Must not be called when `conn` was opened in read-only mode.
pub(crate) fn initialize(conn: &Connection) -> Result<(), LocalFileCacheError> {
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;

    let version = user_version(conn)?;
    match version {
        0 => create_fresh(conn)?,
        1 => migrate_v1_to_v2(conn)?,
        CURRENT_VERSION => {}
        v => {
            return Err(LocalFileCacheError::UnsupportedFeature(format!(
                "database schema version {v} is newer than this library supports \
                 (max {CURRENT_VERSION})"
            )));
        }
    }
    Ok(())
}

/// Enable foreign-key enforcement only (safe to call on a read-only connection).
pub(crate) fn enable_foreign_keys(conn: &Connection) -> Result<(), LocalFileCacheError> {
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn user_version(conn: &Connection) -> Result<u32, LocalFileCacheError> {
    let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    Ok(v as u32)
}

fn set_user_version(conn: &Connection, v: u32) -> Result<(), LocalFileCacheError> {
    conn.execute_batch(&format!("PRAGMA user_version = {v};"))?;
    Ok(())
}

fn create_fresh(conn: &Connection) -> Result<(), LocalFileCacheError> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS files (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            namespace   TEXT    NOT NULL DEFAULT 'default',
            path        TEXT    NOT NULL,
            mtime       INTEGER NOT NULL,
            file_size   INTEGER NOT NULL,
            hash        TEXT,
            updated_at  INTEGER NOT NULL,
            UNIQUE(namespace, path)
        );

        CREATE TABLE IF NOT EXISTS payloads (
            file_id INTEGER PRIMARY KEY,
            content BLOB    NOT NULL,
            FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_files_namespace_path ON files(namespace, path);
        ",
    )?;
    set_user_version(conn, CURRENT_VERSION)?;
    Ok(())
}

fn migrate_v1_to_v2(conn: &Connection) -> Result<(), LocalFileCacheError> {
    conn.execute_batch(
        "
        BEGIN;

        CREATE TABLE files_v2 (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            namespace   TEXT    NOT NULL DEFAULT 'default',
            path        TEXT    NOT NULL,
            mtime       INTEGER NOT NULL,
            file_size   INTEGER NOT NULL,
            hash        TEXT,
            updated_at  INTEGER NOT NULL,
            UNIQUE(namespace, path)
        );

        INSERT INTO files_v2 (id, namespace, path, mtime, file_size, hash, updated_at)
        SELECT id, 'default', path, mtime, file_size, hash, updated_at FROM files;

        DROP TABLE payloads;
        DROP TABLE files;
        ALTER TABLE files_v2 RENAME TO files;

        CREATE TABLE payloads (
            file_id INTEGER PRIMARY KEY,
            content BLOB    NOT NULL,
            FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_files_namespace_path ON files(namespace, path);

        COMMIT;
        ",
    )?;
    set_user_version(conn, CURRENT_VERSION)?;
    Ok(())
}
