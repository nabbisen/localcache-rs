//! Database schema initialisation and migration.
//!
//! | user_version | Description                                              |
//! |--------------|----------------------------------------------------------|
//! | 0            | Empty / pre-migration                                    |
//! | 1            | v0.1 — no namespace                                      |
//! | 2            | v0.2 — namespace column                                  |
//! | 3            | v0.4 — `files.payload_version`, `payloads.encoding`      |
//! | 4            | v0.6 — `files.last_accessed_at`                          |

use rusqlite::Connection;

use crate::error::LocalFileCacheError;

const CURRENT_VERSION: u32 = 4;

/// Apply the current schema to `conn`, running any necessary migrations.
pub(crate) fn initialize(conn: &Connection) -> Result<(), LocalFileCacheError> {
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    let version = user_version(conn)?;
    match version {
        0 => create_fresh(conn)?,
        1 => {
            migrate_v1_to_v2(conn)?;
            migrate_v2_to_v3(conn)?;
            migrate_v3_to_v4(conn)?;
        }
        2 => {
            migrate_v2_to_v3(conn)?;
            migrate_v3_to_v4(conn)?;
        }
        3 => migrate_v3_to_v4(conn)?,
        CURRENT_VERSION => {}
        v => {
            return Err(LocalFileCacheError::UnsupportedFeature(format!(
                "database schema version {v} is newer than this build supports \
                 (max {CURRENT_VERSION})"
            )));
        }
    }
    Ok(())
}

pub(crate) fn enable_foreign_keys(conn: &Connection) -> Result<(), LocalFileCacheError> {
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    Ok(())
}

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

        CREATE TABLE IF NOT EXISTS payloads (
            file_id  INTEGER PRIMARY KEY,
            content  BLOB    NOT NULL,
            encoding TEXT    NOT NULL DEFAULT 'raw',
            FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_files_namespace_path
            ON files(namespace, path);
        CREATE INDEX IF NOT EXISTS idx_files_lru
            ON files(namespace, last_accessed_at, updated_at);
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
    set_user_version(conn, 2)?;
    Ok(())
}

fn migrate_v2_to_v3(conn: &Connection) -> Result<(), LocalFileCacheError> {
    conn.execute_batch(
        "
        ALTER TABLE files    ADD COLUMN payload_version INTEGER NOT NULL DEFAULT 0;
        ALTER TABLE payloads ADD COLUMN encoding        TEXT    NOT NULL DEFAULT 'raw';
        ",
    )?;
    set_user_version(conn, 3)?;
    Ok(())
}

/// Add `last_accessed_at` to `files` and an LRU composite index.
fn migrate_v3_to_v4(conn: &Connection) -> Result<(), LocalFileCacheError> {
    conn.execute_batch(
        "
        ALTER TABLE files ADD COLUMN last_accessed_at INTEGER NOT NULL DEFAULT 0;
        CREATE INDEX IF NOT EXISTS idx_files_lru
            ON files(namespace, last_accessed_at, updated_at);
        ",
    )?;
    set_user_version(conn, CURRENT_VERSION)?;
    Ok(())
}
