use rusqlite::{Connection, OpenFlags};
use tempfile::TempDir;

use super::super::{initialize, validate_read_only};
use super::helpers::{open_v1, snapshot_v1};

#[test]
fn read_only_validation_enables_connection_guards_and_accepts_current_v5() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("current.sqlite3");
    {
        let mut writer = Connection::open(&path).unwrap();
        initialize(&mut writer, false).unwrap();
    }

    let mut reader = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    validate_read_only(&mut reader).unwrap();

    assert_eq!(
        reader
            .query_row("PRAGMA query_only", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        reader
            .query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert!(reader.execute("DELETE FROM files", []).is_err());
}

#[test]
fn read_only_validation_rejects_historical_schema_unchanged() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("historical.sqlite3");
    let writer = open_v1(&path);
    let before = snapshot_v1(&writer);
    drop(writer);

    let mut reader = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let error = validate_read_only(&mut reader).unwrap_err();
    assert_eq!(
        error.to_string(),
        "unsupported feature: read-only open requires the current database schema; initialization or migration is not permitted; database was not modified"
    );
    drop(reader);

    let after = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(snapshot_v1(&after), before);
}
