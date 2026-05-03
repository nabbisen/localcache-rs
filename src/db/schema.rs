//! Database schema initialisation.
//!
//! All DDL statements live here so that schema evolution can be tracked in a
//! single file.

use rusqlite::Connection;

use crate::error::LocalFileCacheError;

/// Apply the current schema to `conn`, creating tables and indexes if they do
/// not already exist.
///
/// Also enables foreign-key enforcement, which is off by default in SQLite.
pub(crate) fn initialize(conn: &Connection) -> Result<(), LocalFileCacheError> {
    conn.execute_batch(
        "
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS files (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            path        TEXT    NOT NULL UNIQUE,
            mtime       INTEGER NOT NULL,
            file_size   INTEGER NOT NULL,
            hash        TEXT,
            updated_at  INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS payloads (
            file_id INTEGER PRIMARY KEY,
            content BLOB    NOT NULL,
            FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_files_path ON files(path);
        ",
    )?;
    Ok(())
}
