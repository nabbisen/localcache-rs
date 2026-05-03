//! Database schema initialisation and migration.
//!
//! Schema versioning uses SQLite's built-in `PRAGMA user_version`.
//!
//! | user_version | Description                          |
//! |--------------|--------------------------------------|
//! | 0            | Empty / pre-migration                |
//! | 1            | v0.1 schema (no namespace column)    |
//! | 2            | v0.2 schema (namespace column added) |

use rusqlite::Connection;

use crate::error::LocalFileCacheError;

/// Current schema version produced by this build.
const CURRENT_VERSION: u32 = 2;

/// Apply the current schema to `conn`, running any necessary migrations.
pub(crate) fn initialize(conn: &Connection) -> Result<(), LocalFileCacheError> {
    // foreign_keys must be ON before any DML, but migrations use DDL only,
    // so we enable it once here and rely on it for all subsequent operations.
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;

    let version = user_version(conn)?;
    match version {
        0 => create_fresh(conn)?,
        1 => migrate_v1_to_v2(conn)?,
        CURRENT_VERSION => {} // already up to date
        v => {
            return Err(LocalFileCacheError::UnsupportedFeature(format!(
                "database schema version {v} is newer than this library supports (max {CURRENT_VERSION})"
            )));
        }
    }
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

/// Create a brand-new v2 schema (no prior data exists).
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

/// Migrate a v1 database (single `path UNIQUE`, no namespace) to v2.
///
/// The migration is performed atomically:
/// 1. Create `files_v2` with the new schema.
/// 2. Copy all existing rows, assigning them to the `'default'` namespace.
/// 3. Drop `payloads` temporarily (will be recreated pointing at the new table).
/// 4. Drop old `files`, rename `files_v2` → `files`.
/// 5. Recreate `payloads`.
/// 6. Bump `user_version`.
fn migrate_v1_to_v2(conn: &Connection) -> Result<(), LocalFileCacheError> {
    conn.execute_batch(
        "
        BEGIN;

        -- Step 1: new table
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

        -- Step 2: copy rows
        INSERT INTO files_v2 (id, namespace, path, mtime, file_size, hash, updated_at)
        SELECT id, 'default', path, mtime, file_size, hash, updated_at FROM files;

        -- Step 3: drop payloads (FK references files.id; will be recreated)
        DROP TABLE payloads;

        -- Step 4: swap tables
        DROP TABLE files;
        ALTER TABLE files_v2 RENAME TO files;

        -- Step 5: recreate payloads
        CREATE TABLE payloads (
            file_id INTEGER PRIMARY KEY,
            content BLOB    NOT NULL,
            FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
        );

        -- Step 6: index
        CREATE INDEX IF NOT EXISTS idx_files_namespace_path ON files(namespace, path);

        COMMIT;
        ",
    )?;
    set_user_version(conn, CURRENT_VERSION)?;
    Ok(())
}
