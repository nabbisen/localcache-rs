use super::*;

fn busy() -> rusqlite::Error {
    rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
        Some("database is locked".to_owned()),
    )
}

#[test]
fn a_sqlite_failure_keeps_its_extended_code() {
    let error = DatabaseError::from_sqlite(busy());
    assert_eq!(
        error.sqlite_extended_code(),
        Some(rusqlite::ffi::SQLITE_BUSY)
    );
}

#[test]
fn the_extended_code_is_kept_whole_not_only_its_primary_part() {
    let failure = rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY_SNAPSHOT);
    let error = DatabaseError::from_sqlite(rusqlite::Error::SqliteFailure(failure, None));
    assert_eq!(
        error.sqlite_extended_code(),
        Some(rusqlite::ffi::SQLITE_BUSY_SNAPSHOT)
    );
    assert_eq!(
        error.sqlite_extended_code().map(|code| code & 0xff),
        Some(5)
    );
}

#[test]
fn an_error_without_a_sqlite_code_has_none() {
    let error = DatabaseError::from_sqlite(rusqlite::Error::QueryReturnedNoRows);
    assert_eq!(error.sqlite_extended_code(), None);
}

#[test]
fn display_is_exactly_what_the_underlying_error_displays() {
    assert_eq!(
        DatabaseError::from_sqlite(busy()).to_string(),
        busy().to_string()
    );
    assert_eq!(
        DatabaseError::from_sqlite(rusqlite::Error::QueryReturnedNoRows).to_string(),
        rusqlite::Error::QueryReturnedNoRows.to_string()
    );
    let wrapped = LocalFileCacheError::Database(DatabaseError::from_sqlite(busy()));
    assert_eq!(wrapped.to_string(), format!("database error: {}", busy()));
}

#[test]
fn source_is_the_underlying_error_type_erased() {
    let error = DatabaseError::from_sqlite(busy());
    let source = std::error::Error::source(&error).expect("has a source");
    assert!(source.downcast_ref::<rusqlite::Error>().is_some());
}

#[test]
fn the_extension_trait_wraps_an_error_and_passes_a_value() {
    let failed: Result<(), rusqlite::Error> = Err(busy());
    match failed.db() {
        Err(LocalFileCacheError::Database(error)) => {
            assert_eq!(
                error.sqlite_extended_code(),
                Some(rusqlite::ffi::SQLITE_BUSY)
            );
        }
        other => panic!("expected a database error, got {other:?}"),
    }
    let fine: Result<u8, rusqlite::Error> = Ok(7);
    assert_eq!(fine.db().unwrap(), 7);
}
