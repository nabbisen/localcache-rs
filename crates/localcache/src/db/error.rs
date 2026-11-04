//! The database layer's error type (RFC 030 slice 2): the five variants of the
//! public `LocalFileCacheError` this layer produces, with the same `Display`
//! texts. It reaches the public enum through the crate-private lift in
//! `crate::error`, never through a public `From` impl.

/// An error of the database layer.
#[derive(Debug, thiserror::Error)]
pub(crate) enum StorageError {
    #[error("database error: {0}")]
    Database(#[source] DatabaseError),
    #[error(
        "database schema is not current; a read-only open cannot migrate it; database was not modified"
    )]
    SchemaNotCurrent,
    #[error("incompatible database schema: {0}")]
    IncompatibleSchema(String),
    #[error("database state: {0}")]
    DatabaseState(String),
    #[error("invalid argument: {argument}: {reason}")]
    InvalidArgument {
        argument: &'static str,
        reason: String,
    },
}

/// The arguments the database layer can reject (a subset of the public ones).
#[derive(Debug, Clone, Copy)]
pub(crate) enum ArgumentName {
    IndexName,
    Records,
}

impl ArgumentName {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::IndexName => "index_name",
            Self::Records => "records",
        }
    }
}

pub(crate) fn invalid_argument(argument: ArgumentName, reason: impl Into<String>) -> StorageError {
    StorageError::InvalidArgument {
        argument: argument.as_str(),
        reason: reason.into(),
    }
}

/// An error from the underlying SQLite database, as carried by
/// [`crate::LocalFileCacheError::Database`].
///
/// The error is opaque: no database-library type appears in this crate's
/// public API. Its [`Display`](std::fmt::Display) is the database's own message,
/// [`std::error::Error::source`] returns the underlying error, and
/// [`sqlite_extended_code`](Self::sqlite_extended_code) gives the SQLite result
/// code when the database reported one.
///
/// A value of this type is created by the library only.
#[derive(Debug)]
pub struct DatabaseError {
    source: Box<dyn std::error::Error + Send + Sync + 'static>,
    extended_code: Option<i32>,
}

impl DatabaseError {
    /// The SQLite *extended* result code of this error, or `None` when the
    /// failure did not come from SQLite reporting a code (for example a
    /// query that returned no rows, or a value that could not be converted).
    ///
    /// The *primary* result code is `code & 0xff`. For example a busy database
    /// is `SQLITE_BUSY`, primary code 5, so
    /// `error.sqlite_extended_code().map(|code| code & 0xff) == Some(5)` tells a
    /// caller the database was busy without naming any database library. The
    /// extended codes are listed in the SQLite documentation, "Result and Error
    /// Codes".
    pub fn sqlite_extended_code(&self) -> Option<i32> {
        self.extended_code
    }

    pub(crate) fn from_sqlite(error: rusqlite::Error) -> Self {
        let extended_code = match &error {
            rusqlite::Error::SqliteFailure(failure, _) => Some(failure.extended_code),
            _ => None,
        };
        Self {
            source: Box::new(error),
            extended_code,
        }
    }
}

impl std::fmt::Display for DatabaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.source, f)
    }
}

impl std::error::Error for DatabaseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&*self.source)
    }
}

/// Crate-private conversion of a SQLite result into the layer's error type.
pub(crate) trait SqliteResultExt<T> {
    fn db(self) -> Result<T, StorageError>;
}

impl<T> SqliteResultExt<T> for Result<T, rusqlite::Error> {
    fn db(self) -> Result<T, StorageError> {
        self.map_err(|error| StorageError::Database(DatabaseError::from_sqlite(error)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_extension_trait_wraps_an_error_and_passes_a_value() {
        let busy = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            Some("database is locked".to_owned()),
        );
        let failed: Result<(), rusqlite::Error> = Err(busy);
        match failed.db() {
            Err(StorageError::Database(error)) => {
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
}
