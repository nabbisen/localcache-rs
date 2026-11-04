//! The one handle to the database (RFC 030 slice 3): the connection, the
//! namespace every method works in, and every statement behind a method.
//!
//! Nothing above the `db` layer names `rusqlite`: the engine, the query layer,
//! the pools and the watchers hold a [`Store`] and call its methods. The open
//! order, the transaction behaviours and the pragmas are the ones the engine
//! used before this module existed; moving them here changes no behaviour.

use std::path::PathBuf;

use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior};

use crate::db::error::{SqliteResultExt, StorageError};
#[cfg(feature = "encryption")]
use crate::db::repository::PayloadPageRow;
use crate::db::repository::{self, CandidateRow, FileRow, FullRow, PayloadRow, RawStats};
use crate::db::schema;
use crate::db::{EntryInfo, FileMetadata, JournalMode, SynchronousMode, indexes};

/// Where the database lives.
#[derive(Debug, Clone)]
pub(crate) enum Location {
    /// A private in-memory database.
    Memory,
    /// The named shared in-memory database of this process.
    SharedMemory,
    /// A file, opened as a plain connection.
    File(PathBuf),
    /// A file opened read-only on a shared cache. The path is UTF-8: a URI
    /// cannot carry anything else, and the caller checks it first.
    SharedFile(String),
}

impl Location {
    fn is_memory(&self) -> bool {
        matches!(self, Self::Memory | Self::SharedMemory)
    }
}

/// How to open a [`Store`].
#[derive(Debug, Clone)]
pub(crate) struct StoreConfig {
    pub location: Location,
    pub namespace: String,
    /// The effective read-only setting: a read-only open validates the schema
    /// and never initializes, migrates or changes a pragma.
    pub read_only: bool,
    pub journal_mode: JournalMode,
    pub synchronous: SynchronousMode,
}

/// The connection and every statement.
pub(crate) struct Store {
    conn: Connection,
    namespace: String,
}

/// A write transaction (`BEGIN IMMEDIATE`); rolls back on drop.
pub(crate) struct WriteTx<'s> {
    tx: Transaction<'s>,
    namespace: &'s str,
}

impl Store {
    /// Open the database, validating (read-only) or initializing and
    /// configuring it (writable), in the order the engine always used.
    pub(crate) fn open(config: &StoreConfig) -> Result<Self, StorageError> {
        let is_memory = config.location.is_memory();
        let mut conn = match &config.location {
            // Named shared in-memory database: every connection opened with
            // this URI within the process sees the same data.
            Location::SharedMemory => Connection::open_with_flags(
                "file::memory:?cache=shared",
                OpenFlags::SQLITE_OPEN_URI
                    | OpenFlags::SQLITE_OPEN_READ_WRITE
                    | OpenFlags::SQLITE_OPEN_CREATE
                    | OpenFlags::SQLITE_OPEN_SHARED_CACHE,
            )
            .db()?,
            Location::SharedFile(path) => {
                let uri = format!("file:{}?mode=ro&cache=shared", uri_encode_path(path));
                Connection::open_with_flags(
                    uri,
                    OpenFlags::SQLITE_OPEN_URI
                        | OpenFlags::SQLITE_OPEN_READ_ONLY
                        | OpenFlags::SQLITE_OPEN_SHARED_CACHE
                        | OpenFlags::SQLITE_OPEN_NO_MUTEX,
                )
                .db()?
            }
            Location::Memory => Connection::open_in_memory().db()?,
            Location::File(path) if config.read_only => Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )
            .db()?,
            Location::File(path) => Connection::open(path).db()?,
        };

        if config.read_only {
            schema::validate_read_only(&mut conn)?;
        } else {
            let outcome = schema::initialize(&mut conn, is_memory)?;
            if !is_memory {
                schema::apply_runtime_configuration(
                    &conn,
                    config.journal_mode,
                    config.synchronous,
                    &outcome,
                )?;
            }
        }
        Ok(Self {
            conn,
            namespace: config.namespace.clone(),
        })
    }

    /// Begin a write transaction: `BEGIN IMMEDIATE`, so a concurrent writer
    /// waits under the busy timeout instead of failing a deferred upgrade.
    pub(crate) fn begin_write(&self) -> Result<WriteTx<'_>, StorageError> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate).db()?;
        Ok(WriteTx {
            tx,
            namespace: &self.namespace,
        })
    }

    /// `VACUUM`.
    pub(crate) fn vacuum(&self) -> Result<(), StorageError> {
        self.conn.execute_batch("VACUUM;").db()
    }

    // ---- reads --------------------------------------------------------

    pub(crate) fn find_file(&self, path: &str) -> Result<Option<FileRow>, StorageError> {
        repository::find_file(&self.conn, &self.namespace, path)
    }

    pub(crate) fn load_payload(&self, file_id: i64) -> Result<Option<PayloadRow>, StorageError> {
        repository::load_payload(&self.conn, file_id)
    }

    pub(crate) fn exists(&self, path: &str) -> Result<bool, StorageError> {
        repository::exists(&self.conn, &self.namespace, path)
    }

    pub(crate) fn keys(
        &self,
        pattern: Option<&str>,
        path_in_dir: Option<(&str, bool)>,
        path_glob: Option<&[String]>,
    ) -> Result<Vec<PathBuf>, StorageError> {
        repository::keys(&self.conn, &self.namespace, pattern, path_in_dir, path_glob)
    }

    pub(crate) fn explain_query(
        &self,
        pattern: Option<&str>,
        path_in_dir: Option<(&str, bool)>,
        path_glob: Option<&[String]>,
    ) -> Result<String, StorageError> {
        repository::explain_query(&self.conn, &self.namespace, pattern, path_in_dir, path_glob)
    }

    pub(crate) fn list_entries(&self) -> Result<Vec<EntryInfo>, StorageError> {
        repository::list_entries(&self.conn, &self.namespace)
    }

    pub(crate) fn count(&self) -> Result<usize, StorageError> {
        repository::count_in_namespace(&self.conn, &self.namespace)
    }

    pub(crate) fn count_by_version(&self) -> Result<Vec<(u32, usize)>, StorageError> {
        repository::count_by_version(&self.conn, &self.namespace)
    }

    pub(crate) fn aggregate_stats(&self) -> Result<RawStats, StorageError> {
        repository::aggregate_stats(&self.conn, &self.namespace)
    }

    pub(crate) fn encoding_breakdown(&self) -> Result<Vec<(String, usize)>, StorageError> {
        repository::encoding_breakdown(&self.conn, &self.namespace)
    }

    /// Every namespace of the database (not only this store's).
    pub(crate) fn list_namespaces(&self) -> Result<Vec<String>, StorageError> {
        repository::list_namespaces(&self.conn)
    }

    pub(crate) fn paths_page(
        &self,
        after: &str,
        limit: usize,
    ) -> Result<Vec<String>, StorageError> {
        repository::paths_page_in_namespace(&self.conn, &self.namespace, after, limit)
    }

    pub(crate) fn path_rows_page(
        &self,
        after: &str,
        limit: usize,
    ) -> Result<Vec<(String, i64)>, StorageError> {
        repository::path_rows_page_in_namespace(&self.conn, &self.namespace, after, limit)
    }

    pub(crate) fn load_all_full(&self) -> Result<Vec<FullRow>, StorageError> {
        repository::load_all_full(&self.conn, &self.namespace)
    }

    pub(crate) fn query_candidates(
        &self,
        pattern: Option<&str>,
        path_in_dir: Option<(&str, bool)>,
        path_glob: Option<&[String]>,
    ) -> Result<Vec<CandidateRow>, StorageError> {
        repository::query_candidates(&self.conn, &self.namespace, pattern, path_in_dir, path_glob)
    }

    pub(crate) fn payloads_for_ids(
        &self,
        ids: &[i64],
    ) -> Result<Vec<(i64, Vec<u8>, String)>, StorageError> {
        repository::payloads_for_ids(&self.conn, ids)
    }

    #[cfg(feature = "json")]
    pub(crate) fn query_candidates_with_payloads(
        &self,
        pattern: Option<&str>,
        path_in_dir: Option<(&str, bool)>,
        path_glob: Option<&[String]>,
    ) -> Result<Vec<repository::FullCandidateRow>, StorageError> {
        repository::query_candidates_with_payloads(
            &self.conn,
            &self.namespace,
            pattern,
            path_in_dir,
            path_glob,
        )
    }

    #[cfg(feature = "json")]
    pub(crate) fn query_candidates_json_pushdown(
        &self,
        pattern: Option<&str>,
        path_in_dir: Option<(&str, bool)>,
        path_glob: Option<&[String]>,
        select_fields: &[String],
        where_fields: &[(String, &'static str, f64)],
    ) -> Result<Vec<repository::JsonPushdownRow>, StorageError> {
        repository::query_candidates_json_pushdown(
            &self.conn,
            &self.namespace,
            pattern,
            path_in_dir,
            path_glob,
            select_fields,
            where_fields,
        )
    }

    #[cfg(feature = "json")]
    pub(crate) fn namespace_all_json(&self) -> Result<bool, StorageError> {
        repository::namespace_all_json(&self.conn, &self.namespace)
    }

    pub(crate) fn list_path_indexes(&self) -> Result<Vec<String>, StorageError> {
        indexes::list_path_indexes(&self.conn)
    }

    // ---- writes outside a transaction ------------------------------------

    pub(crate) fn record_use(&self, file_id: i64) -> Result<(), StorageError> {
        repository::record_use(&self.conn, &self.namespace, file_id)
    }

    pub(crate) fn delete_by_path(&self, path: &str) -> Result<bool, StorageError> {
        repository::delete_by_path(&self.conn, &self.namespace, path)
    }

    pub(crate) fn delete_paths(&self, paths: &[String]) -> Result<usize, StorageError> {
        repository::delete_paths(&self.conn, &self.namespace, paths)
    }

    pub(crate) fn delete_by_other_version(
        &self,
        current_version: u32,
    ) -> Result<usize, StorageError> {
        repository::delete_by_other_version(&self.conn, &self.namespace, current_version)
    }

    pub(crate) fn drop_path_index(&self, name: &str) -> Result<bool, StorageError> {
        indexes::drop_path_index(&self.conn, name)
    }

    /// The raw connection, for tests that set up or inspect rows with SQL.
    /// It exists only in test builds and is not part of the layer's contract.
    #[cfg(test)]
    pub(crate) fn raw(&self) -> &Connection {
        &self.conn
    }
}

impl WriteTx<'_> {
    pub(crate) fn upsert(
        &self,
        path: &str,
        metadata: &FileMetadata,
        payload_bytes: &[u8],
        encoding: &str,
        payload_version: u32,
    ) -> Result<i64, StorageError> {
        repository::upsert_in_tx(
            &self.tx,
            self.namespace,
            path,
            metadata,
            payload_bytes,
            encoding,
            payload_version,
        )
    }

    #[cfg(feature = "encryption")]
    pub(crate) fn update_payload_content(
        &self,
        file_id: i64,
        new_content: &[u8],
        new_tag: &str,
    ) -> Result<(), StorageError> {
        repository::update_payload_content(&self.tx, file_id, new_content, new_tag)
    }

    pub(crate) fn count(&self) -> Result<usize, StorageError> {
        repository::count_in_namespace(&self.tx, self.namespace)
    }

    pub(crate) fn evict_lru(
        &self,
        n: usize,
        protected: &[i64],
    ) -> Result<Vec<PathBuf>, StorageError> {
        repository::evict_lru(&self.tx, self.namespace, n, protected)
    }

    pub(crate) fn import_rows(&self, rows: &[FullRow]) -> Result<Vec<i64>, StorageError> {
        repository::import_rows_in_tx(&self.tx, self.namespace, rows)
    }

    /// A page of payload rows read inside this transaction (key rotation).
    #[cfg(feature = "encryption")]
    pub(crate) fn load_payload_page(
        &self,
        after: &str,
        max_rows: usize,
        max_bytes: usize,
    ) -> Result<Vec<PayloadPageRow>, StorageError> {
        repository::load_payload_page(&self.tx, self.namespace, after, max_rows, max_bytes)
    }

    pub(crate) fn commit(self) -> Result<(), StorageError> {
        self.tx.commit().db()
    }
}

/// Percent-encode the characters that are significant inside a SQLite
/// `file:` URI path component: `%`, `#`, `?`, and space.
///
/// SQLite decodes `%XX` escapes in URI filenames, so a literal `%` must be
/// escaped first; `#` and `?` would otherwise terminate the path component.
/// No external dependency is required for this small, fixed set.
fn uri_encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        match c {
            '%' => out.push_str("%25"),
            '#' => out.push_str("%23"),
            '?' => out.push_str("%3F"),
            ' ' => out.push_str("%20"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests;
