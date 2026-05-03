//! [`CacheEngine`] implementation.

use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};
use serde::{Serialize, de::DeserializeOwned};

use crate::cache::entry::{CacheEntry, CacheStatus, EntryInfo};
use crate::cache::options::{
    CacheOptions, ChangeDetectionMode, Codec, ScanOptions, is_memory_path,
};
use crate::db::{repository, schema};
use crate::detection::hash::{compute_full_hash, compute_partial_hash};
use crate::detection::metadata::collect_metadata;
use crate::detection::strategy::detect_change;
use crate::error::LocalFileCacheError;
use crate::path::normalize_path;
use crate::serialization::{decode_payload, encode_payload};

// ---------------------------------------------------------------------------
// Public result types
// ---------------------------------------------------------------------------

/// Result summary returned by [`CacheEngine::batch_set`].
#[derive(Debug, Default)]
pub struct BatchSetReport {
    /// Number of entries stored successfully.
    pub succeeded: usize,
    /// Per-item errors: `(path, error)`.
    pub failed: Vec<(PathBuf, LocalFileCacheError)>,
}

// ---------------------------------------------------------------------------
// CacheEngine
// ---------------------------------------------------------------------------

/// The main entry point for `localcache`.
///
/// ## Quick example
///
/// ```no_run
/// use localcache::{CacheEngine, CacheOptions, ChangeDetectionMode};
///
/// let engine = CacheEngine::<Vec<f32>>::open(CacheOptions {
///     database_path: "cache.sqlite3".into(),
///     change_detection_mode: ChangeDetectionMode::MetadataThenFullHash,
///     ..CacheOptions::default()
/// })?;
///
/// engine.set("sample.txt", &vec![0.1_f32, 0.2, 0.3])?;
///
/// if let Some(entry) = engine.get_if_fresh("sample.txt")? {
///     println!("cached: {:?}", entry.payload);
/// }
/// # Ok::<(), localcache::LocalFileCacheError>(())
/// ```
pub struct CacheEngine<T> {
    pub(crate) conn: Connection,
    pub(crate) mode: ChangeDetectionMode,
    pub(crate) codec: Codec,
    pub(crate) namespace: String,
    pub(crate) ttl: Option<Duration>,
    pub(crate) read_only: bool,
    pub(crate) payload_version: u32,
    pub(crate) compress: bool,
    pub(crate) max_entries: Option<usize>,
    #[cfg(feature = "encryption")]
    pub(crate) encryption_key: Option<[u8; 32]>,
    _phantom: PhantomData<T>,
}

impl<T> CacheEngine<T>
where
    T: Serialize + DeserializeOwned,
{
    // ------------------------------------------------------------------
    // Construction
    // ------------------------------------------------------------------

    /// Open (or create) a [`CacheEngine`] using `options`.
    pub fn open(options: CacheOptions) -> Result<Self, LocalFileCacheError> {
        let is_memory = is_memory_path(&options.database_path);

        let conn = if is_memory {
            Connection::open_in_memory()?
        } else if options.read_only {
            Connection::open_with_flags(
                &options.database_path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?
        } else {
            Connection::open(&options.database_path)?
        };

        if is_memory || !options.read_only {
            if !is_memory {
                conn.execute_batch(&format!(
                    "PRAGMA journal_mode = {}; PRAGMA synchronous = {};",
                    options.journal_mode.as_str(),
                    options.synchronous.as_str(),
                ))?;
            }
            schema::initialize(&conn)?;
        } else {
            schema::enable_foreign_keys(&conn)?;
        }

        let compress = {
            #[cfg(feature = "compression")]
            {
                options.compress_payloads
            }
            #[cfg(not(feature = "compression"))]
            {
                false
            }
        };

        #[cfg(feature = "encryption")]
        let encryption_key: Option<[u8; 32]> = match options.encryption_key {
            None => None,
            Some(ref k) => {
                let arr: [u8; 32] = k.as_slice().try_into().map_err(|_| {
                    LocalFileCacheError::UnsupportedFeature(format!(
                        "encryption key must be exactly 32 bytes, got {}",
                        k.len()
                    ))
                })?;
                Some(arr)
            }
        };

        Ok(Self {
            conn,
            mode: options.change_detection_mode,
            codec: options.codec,
            namespace: options.namespace,
            ttl: options.ttl,
            read_only: options.read_only,
            payload_version: options.payload_version,
            compress,
            max_entries: options.max_entries,
            #[cfg(feature = "encryption")]
            encryption_key,
            _phantom: PhantomData,
        })
    }

    // ------------------------------------------------------------------
    // Reads
    // ------------------------------------------------------------------

    /// Return the cached entry for `path`, if one exists.
    ///
    /// Updates `last_accessed_at` on a cache hit (LRU tracking).
    /// No change-detection or version check is performed.
    pub fn get<P>(&self, path: P) -> Result<Option<CacheEntry<T>>, LocalFileCacheError>
    where
        P: AsRef<Path>,
    {
        let canonical = normalize_path(path.as_ref())?;
        let path_str = path_to_str(&canonical)?;

        let Some(row) = repository::find_file(&self.conn, &self.namespace, path_str)? else {
            return Ok(None);
        };
        let Some(payload_row) = repository::load_payload(&self.conn, row.id)? else {
            return Ok(None);
        };
        let payload: T = self.decode(&payload_row.content, &payload_row.encoding)?;
        if !self.read_only {
            let _ = repository::touch_last_accessed(&self.conn, row.id);
        }
        Ok(Some(CacheEntry {
            path: PathBuf::from(&row.path),
            metadata: row.metadata,
            payload,
        }))
    }

    /// Return the cached entry for `path` only if it is still fresh.
    ///
    /// Updates `last_accessed_at` on a fresh hit (LRU tracking).
    pub fn get_if_fresh<P>(&self, path: P) -> Result<Option<CacheEntry<T>>, LocalFileCacheError>
    where
        P: AsRef<Path>,
    {
        let canonical = match normalize_path(path.as_ref()) {
            Ok(p) => p,
            Err(LocalFileCacheError::FileNotFound { .. }) => return Ok(None),
            Err(e) => return Err(e),
        };
        let path_str = path_to_str(&canonical)?;

        let Some(row) = repository::find_file(&self.conn, &self.namespace, path_str)? else {
            return Ok(None);
        };
        if is_expired(row.updated_at, self.ttl) {
            return Ok(None);
        }
        if self.payload_version > 0 && row.payload_version != self.payload_version {
            return Ok(None);
        }
        match detect_change(&canonical, &row.metadata, self.mode)? {
            CacheStatus::Stale | CacheStatus::Missing => return Ok(None),
            CacheStatus::Fresh => {}
        }
        let Some(payload_row) = repository::load_payload(&self.conn, row.id)? else {
            return Ok(None);
        };
        let payload: T = self.decode(&payload_row.content, &payload_row.encoding)?;
        if !self.read_only {
            let _ = repository::touch_last_accessed(&self.conn, row.id);
        }
        Ok(Some(CacheEntry {
            path: PathBuf::from(&row.path),
            metadata: row.metadata,
            payload,
        }))
    }

    // ------------------------------------------------------------------
    // Batch reads
    // ------------------------------------------------------------------

    pub fn batch_get<P>(
        &self,
        paths: &[P],
    ) -> Vec<Result<Option<CacheEntry<T>>, LocalFileCacheError>>
    where
        P: AsRef<Path>,
    {
        paths.iter().map(|p| self.get(p.as_ref())).collect()
    }

    pub fn batch_get_fresh<P>(
        &self,
        paths: &[P],
    ) -> Vec<Result<Option<CacheEntry<T>>, LocalFileCacheError>>
    where
        P: AsRef<Path>,
    {
        paths
            .iter()
            .map(|p| self.get_if_fresh(p.as_ref()))
            .collect()
    }

    // ------------------------------------------------------------------
    // Writes
    // ------------------------------------------------------------------

    pub fn set<P>(&self, path: P, payload: &T) -> Result<(), LocalFileCacheError>
    where
        P: AsRef<Path>,
    {
        self.guard_write()?;
        let canonical = normalize_path(path.as_ref())?;
        let path_str = path_to_str(&canonical)?;
        let mut metadata = collect_metadata(&canonical)?;
        metadata.hash = compute_hash_for_mode(&canonical, self.mode)?;
        let (bytes, encoding) = self.encode(payload)?;
        repository::upsert(
            &self.conn,
            &self.namespace,
            path_str,
            &metadata,
            &bytes,
            encoding,
            self.payload_version,
        )?;
        self.enforce_max_entries()?;
        Ok(())
    }

    pub fn batch_set<P>(&self, items: &[(P, T)]) -> Result<BatchSetReport, LocalFileCacheError>
    where
        P: AsRef<Path>,
    {
        self.guard_write()?;

        let mut report = BatchSetReport::default();
        let mut prepared: Vec<(
            String,
            crate::detection::metadata::FileMetadata,
            Vec<u8>,
            &'static str,
        )> = Vec::with_capacity(items.len());

        for (path, payload) in items {
            let canonical = match normalize_path(path.as_ref()) {
                Ok(p) => p,
                Err(e) => {
                    report.failed.push((path.as_ref().to_path_buf(), e));
                    continue;
                }
            };
            let path_str = match path_to_str(&canonical) {
                Ok(s) => s.to_owned(),
                Err(e) => {
                    report.failed.push((canonical.clone(), e));
                    continue;
                }
            };
            let mut metadata = match collect_metadata(&canonical) {
                Ok(m) => m,
                Err(e) => {
                    report.failed.push((canonical.clone(), e));
                    continue;
                }
            };
            match compute_hash_for_mode(&canonical, self.mode) {
                Ok(h) => metadata.hash = h,
                Err(e) => {
                    report.failed.push((canonical.clone(), e));
                    continue;
                }
            }
            let (bytes, encoding) = match self.encode(payload) {
                Ok(r) => r,
                Err(e) => {
                    report.failed.push((canonical.clone(), e));
                    continue;
                }
            };
            prepared.push((path_str, metadata, bytes, encoding));
        }

        let tx = self.conn.unchecked_transaction()?;
        for (path_str, metadata, bytes, encoding) in &prepared {
            repository::upsert_in_tx(
                &tx,
                &self.namespace,
                path_str,
                metadata,
                bytes,
                encoding,
                self.payload_version,
            )?;
            report.succeeded += 1;
        }
        tx.commit()?;
        self.enforce_max_entries()?;
        Ok(report)
    }

    // ------------------------------------------------------------------
    // Removal
    // ------------------------------------------------------------------

    pub fn remove<P>(&self, path: P) -> Result<bool, LocalFileCacheError>
    where
        P: AsRef<Path>,
    {
        self.guard_write()?;
        match normalize_path(path.as_ref()) {
            Ok(canonical) => {
                let path_str = path_to_str(&canonical)?;
                return repository::delete_by_path(&self.conn, &self.namespace, path_str);
            }
            Err(LocalFileCacheError::FileNotFound { .. }) => {}
            Err(e) => return Err(e),
        }
        let raw = path.as_ref().to_string_lossy();
        let stored = repository::all_paths_in_namespace(&self.conn, &self.namespace)?;
        for s in &stored {
            if s.as_str() == raw.as_ref()
                || (s.ends_with(raw.as_ref())
                    && Path::new(s).file_name().and_then(|n| n.to_str())
                        == Path::new(raw.as_ref()).file_name().and_then(|n| n.to_str()))
            {
                return repository::delete_by_path(&self.conn, &self.namespace, s);
            }
        }
        repository::delete_by_path(&self.conn, &self.namespace, raw.as_ref())
    }

    // ------------------------------------------------------------------
    // Status
    // ------------------------------------------------------------------

    pub fn check_status<P>(&self, path: P) -> Result<CacheStatus, LocalFileCacheError>
    where
        P: AsRef<Path>,
    {
        let canonical = match normalize_path(path.as_ref()) {
            Ok(p) => p,
            Err(LocalFileCacheError::FileNotFound { .. }) => return Ok(CacheStatus::Missing),
            Err(e) => return Err(e),
        };
        let path_str = path_to_str(&canonical)?;
        let Some(row) = repository::find_file(&self.conn, &self.namespace, path_str)? else {
            return Ok(CacheStatus::Missing);
        };
        if is_expired(row.updated_at, self.ttl) {
            return Ok(CacheStatus::Stale);
        }
        if self.payload_version > 0 && row.payload_version != self.payload_version {
            return Ok(CacheStatus::Stale);
        }
        detect_change(&canonical, &row.metadata, self.mode)
    }

    // ------------------------------------------------------------------
    // Directory scan
    // ------------------------------------------------------------------

    pub fn scan_dir<P: AsRef<Path>>(
        &self,
        dir: P,
        recursive: bool,
    ) -> Result<Vec<(PathBuf, CacheStatus)>, LocalFileCacheError> {
        self.scan_dir_filtered(
            dir,
            ScanOptions {
                recursive,
                ..ScanOptions::default()
            },
        )
    }

    /// Scan `dir` with fine-grained filtering via [`ScanOptions`].
    ///
    /// Supports extension filtering, `max_depth`, and glob patterns on file
    /// names (`*` matches any sequence; `?` matches exactly one character).
    pub fn scan_dir_filtered<P: AsRef<Path>>(
        &self,
        dir: P,
        options: ScanOptions,
    ) -> Result<Vec<(PathBuf, CacheStatus)>, LocalFileCacheError> {
        let dir = dir.as_ref();
        if !dir.is_dir() {
            return Err(LocalFileCacheError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("not a directory: {}", dir.display()),
            )));
        }
        // Compile glob pattern once before walking.
        let glob = options
            .glob_pattern
            .as_deref()
            .map(glob_to_regex)
            .transpose()?;

        let files = walk_dir_filtered(dir, &options, &glob, 0)?;
        let mut results = Vec::with_capacity(files.len());
        for file in files {
            let status = self.check_status(&file)?;
            results.push((file, status));
        }
        Ok(results)
    }

    // ------------------------------------------------------------------
    // Observability
    // ------------------------------------------------------------------

    /// Return lightweight metadata for all entries in the current namespace.
    ///
    /// Entries are ordered by `updated_at` descending (most recently written
    /// first).  Payload content is **not** loaded.
    pub fn list_entries(&self) -> Result<Vec<EntryInfo>, LocalFileCacheError> {
        repository::list_entries(&self.conn, &self.namespace)
    }

    pub fn entry_count(&self) -> Result<usize, LocalFileCacheError> {
        repository::count_in_namespace(&self.conn, &self.namespace)
    }

    pub fn entry_count_by_version(&self) -> Result<Vec<(u32, usize)>, LocalFileCacheError> {
        repository::count_by_version(&self.conn, &self.namespace)
    }

    /// Return aggregate statistics for the current namespace.
    ///
    /// Statistics include total entry count, total stored payload bytes
    /// (compressed/encrypted as applicable), oldest and newest `updated_at`
    /// timestamps, and per-encoding / per-version breakdowns.
    pub fn cache_stats(&self) -> Result<crate::cache::entry::CacheStats, LocalFileCacheError> {
        use crate::cache::entry::CacheStats;

        let raw = repository::aggregate_stats(&self.conn, &self.namespace)?;
        let entries_by_encoding = repository::encoding_breakdown(&self.conn, &self.namespace)?;
        let entries_by_payload_version = repository::count_by_version(&self.conn, &self.namespace)?;

        Ok(CacheStats {
            namespace: self.namespace.clone(),
            total_entries: raw.total_entries,
            total_payload_bytes: raw.total_payload_bytes,
            oldest_updated_at: raw.oldest_updated_at,
            newest_updated_at: raw.newest_updated_at,
            entries_by_encoding,
            entries_by_payload_version,
        })
    }

    // ------------------------------------------------------------------
    // Batch status
    // ------------------------------------------------------------------

    /// Check the freshness of multiple paths in a single call.
    ///
    /// Returns one `Result<CacheStatus, _>` per input path, in the same order.
    /// Individual errors (e.g. I/O errors reading metadata for one file) do
    /// not abort the remaining checks.
    pub fn check_status_batch<P>(
        &self,
        paths: &[P],
    ) -> Vec<Result<CacheStatus, LocalFileCacheError>>
    where
        P: AsRef<Path>,
    {
        paths
            .iter()
            .map(|p| self.check_status(p.as_ref()))
            .collect()
    }

    // ------------------------------------------------------------------
    // Key rotation
    // ------------------------------------------------------------------

    /// Re-encrypt all entries in the current namespace with `new_key`.
    ///
    /// Every payload whose encoding ends in `"-aes256gcm"` is decrypted with
    /// the current key and re-encrypted with `new_key`.  The operation is
    /// performed inside a single SQLite transaction so that a failure leaves
    /// the database consistent (still encrypted with the old key).
    ///
    /// Returns the number of entries that were re-encrypted.
    ///
    /// # Errors
    ///
    /// * [`LocalFileCacheError::ReadOnly`] — engine is in read-only mode.
    /// * [`LocalFileCacheError::UnsupportedFeature`] — no encryption key is
    ///   currently set on this engine (nothing to rotate).
    /// * [`LocalFileCacheError::EncryptionError`] — decryption or re-encryption
    ///   failed.
    #[cfg(feature = "encryption")]
    pub fn rotate_encryption_key(&self, new_key: &[u8]) -> Result<usize, LocalFileCacheError> {
        self.guard_write()?;

        let old_key = self.encryption_key.ok_or_else(|| {
            LocalFileCacheError::UnsupportedFeature(
                "rotate_encryption_key requires an existing encryption key on this engine".into(),
            )
        })?;

        let new_key_arr: [u8; 32] = new_key.try_into().map_err(|_| {
            LocalFileCacheError::UnsupportedFeature(format!(
                "new encryption key must be exactly 32 bytes, got {}",
                new_key.len()
            ))
        })?;

        // Load all encrypted payload rows for this namespace.
        let rows = repository::load_encrypted_payloads(&self.conn, &self.namespace)?;
        if rows.is_empty() {
            return Ok(0);
        }

        // Re-encrypt each row; collect updates before opening the transaction
        // to keep the borrow of `self.conn` clean.
        let mut updates: Vec<(i64, Vec<u8>)> = Vec::with_capacity(rows.len());
        for row in &rows {
            // Decrypt with old key.
            let plaintext = crate::serialization::decrypt_for_rotation(&row.content, &old_key)?;
            // Re-encrypt with new key.
            let ciphertext = crate::serialization::encrypt_for_rotation(&plaintext, &new_key_arr)?;
            updates.push((row.file_id, ciphertext));
        }

        // Write all updates atomically.
        let tx = self.conn.unchecked_transaction()?;
        for (file_id, new_content) in &updates {
            repository::update_payload_content(&tx, *file_id, new_content)?;
        }
        tx.commit()?;

        Ok(updates.len())
    }

    // ------------------------------------------------------------------
    // Builder entrypoint
    // ------------------------------------------------------------------

    /// Return a fluent builder for constructing a [`CacheEngine`].
    ///
    /// # Example
    ///
    /// ```no_run
    /// use std::time::Duration;
    /// use localcache::{CacheEngine, ChangeDetectionMode};
    ///
    /// let engine = CacheEngine::<Vec<f32>>::builder()
    ///     .database("cache.sqlite3")
    ///     .namespace("embeddings")
    ///     .change_detection(ChangeDetectionMode::MetadataThenFullHash)
    ///     .ttl(Duration::from_secs(3600))
    ///     .max_entries(500)
    ///     .build()?;
    /// # Ok::<(), localcache::LocalFileCacheError>(())
    /// ```
    pub fn builder() -> crate::cache::builder::CacheEngineBuilder<T> {
        crate::cache::builder::CacheEngineBuilder::new()
    }

    // ------------------------------------------------------------------
    // Maintenance
    // ------------------------------------------------------------------

    pub fn cleanup_missing_files(&self) -> Result<usize, LocalFileCacheError> {
        self.guard_write()?;
        let paths = repository::all_paths_in_namespace(&self.conn, &self.namespace)?;
        let mut removed = 0;
        for p in &paths {
            if !Path::new(p).exists() {
                repository::delete_path(&self.conn, &self.namespace, p)?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    pub fn cleanup_expired(&self) -> Result<usize, LocalFileCacheError> {
        self.guard_write()?;
        let Some(ttl) = self.ttl else {
            return Ok(0);
        };
        let rows = repository::all_file_rows_in_namespace(&self.conn, &self.namespace)?;
        let mut removed = 0;
        for (_, path, updated_at) in &rows {
            if is_expired(*updated_at, Some(ttl)) {
                repository::delete_path(&self.conn, &self.namespace, path)?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    pub fn purge_stale_versions(&self) -> Result<usize, LocalFileCacheError> {
        self.guard_write()?;
        repository::delete_by_other_version(&self.conn, &self.namespace, self.payload_version)
    }

    pub fn shrink_database(&self) -> Result<(), LocalFileCacheError> {
        self.guard_write()?;
        self.conn.execute_batch("VACUUM;")?;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Private helpers
    // ------------------------------------------------------------------

    #[inline]
    fn guard_write(&self) -> Result<(), LocalFileCacheError> {
        if self.read_only {
            Err(LocalFileCacheError::ReadOnly)
        } else {
            Ok(())
        }
    }

    fn enforce_max_entries(&self) -> Result<(), LocalFileCacheError> {
        let Some(max) = self.max_entries else {
            return Ok(());
        };
        let count = repository::count_in_namespace(&self.conn, &self.namespace)?;
        if count > max {
            repository::delete_lru_n(&self.conn, &self.namespace, count - max)?;
        }
        Ok(())
    }

    fn encode(&self, payload: &T) -> Result<(Vec<u8>, &'static str), LocalFileCacheError> {
        encode_payload(
            payload,
            self.compress,
            self.codec,
            #[cfg(feature = "encryption")]
            self.encryption_key.as_ref(),
        )
    }

    fn decode(&self, bytes: &[u8], encoding: &str) -> Result<T, LocalFileCacheError> {
        decode_payload(
            bytes,
            encoding,
            #[cfg(feature = "encryption")]
            self.encryption_key.as_ref(),
        )
    }
}

// ---------------------------------------------------------------------------
// Free helpers (pub(crate) for async_engine)
// ---------------------------------------------------------------------------

pub(crate) fn path_to_str(path: &Path) -> Result<&str, LocalFileCacheError> {
    path.to_str()
        .ok_or_else(|| LocalFileCacheError::InvalidPath {
            path: path.to_path_buf(),
        })
}

pub(crate) fn compute_hash_for_mode(
    path: &Path,
    mode: ChangeDetectionMode,
) -> Result<Option<String>, LocalFileCacheError> {
    match mode {
        ChangeDetectionMode::MetadataOnly => Ok(None),
        ChangeDetectionMode::MetadataThenPartialHash => Ok(Some(compute_partial_hash(path)?)),
        ChangeDetectionMode::MetadataThenFullHash | ChangeDetectionMode::StrictFullHash => {
            Ok(Some(compute_full_hash(path)?))
        }
    }
}

pub(crate) fn is_expired(updated_at: i64, ttl: Option<Duration>) -> bool {
    let Some(ttl) = ttl else {
        return false;
    };
    let now = repository::now_secs();
    now.saturating_sub(updated_at) as u64 >= ttl.as_secs()
}

// ---------------------------------------------------------------------------
// Glob matching (with brace expansion)
// ---------------------------------------------------------------------------

/// A compiled glob pattern that may represent multiple alternatives after
/// brace expansion (`{a,b,c}` → three sub-patterns).
struct GlobPattern {
    /// One or more `*`/`?` patterns to match against.  A file name matches if
    /// it matches **any** of these patterns.
    alternatives: Vec<SingleGlob>,
}

/// A single compiled glob pattern (no brace expansion).
struct SingleGlob {
    _raw: String,
    parts: Vec<String>,
    trailing_star: bool,
}

/// Compile `pattern` into a [`GlobPattern`], expanding `{a,b,c}` brace groups.
///
/// Only the first brace group is expanded (non-recursive); nesting and
/// multiple groups in one pattern are not yet supported.
fn glob_to_regex(pattern: &str) -> Result<GlobPattern, LocalFileCacheError> {
    let expanded = expand_braces(pattern);
    let alternatives = expanded
        .into_iter()
        .map(|p| {
            let parts: Vec<String> = p.split('*').map(|s| s.to_owned()).collect();
            let trailing_star = p.ends_with('*');
            SingleGlob {
                _raw: p,
                parts,
                trailing_star,
            }
        })
        .collect();
    Ok(GlobPattern { alternatives })
}

impl GlobPattern {
    fn matches(&self, text: &str) -> bool {
        self.alternatives
            .iter()
            .any(|g| glob_match(&g.parts, g.trailing_star, text))
    }
}

/// Expand a single `{a,b,c}` brace group in `pattern`.
///
/// Returns a `Vec` of concrete patterns with the braces replaced by each
/// alternative.  If no brace group is found, returns a single-element vec
/// with the original pattern.
fn expand_braces(pattern: &str) -> Vec<String> {
    if let (Some(open), Some(close_rel)) = (
        pattern.find('{'),
        pattern.find('{').and_then(|o| pattern[o..].find('}')),
    ) {
        let close = open + close_rel;
        let prefix = &pattern[..open];
        let suffix = &pattern[close + 1..];
        let inner = &pattern[open + 1..close];
        return inner
            .split(',')
            .map(|alt| format!("{prefix}{alt}{suffix}"))
            .collect();
    }
    vec![pattern.to_owned()]
}

/// Recursive glob matcher.
///
/// `parts` are the substrings between consecutive `*` wildcards; `?` within
/// each part matches exactly one character.
fn glob_match(parts: &[String], trailing_star: bool, text: &str) -> bool {
    if parts.is_empty() {
        return trailing_star || text.is_empty();
    }
    if parts.len() == 1 && !trailing_star {
        // No `*` in pattern → must match exactly (but `?` can vary).
        return question_match(&parts[0], text);
    }

    // First part must match at the beginning of `text`.
    let first = &parts[0];
    if !text.starts_with_question(first) {
        return false;
    }
    let after_first = &text[question_len(first)..];

    // Each subsequent part must appear somewhere after the previous match.
    let mut remaining = after_first;
    for part in &parts[1..parts.len() - 1] {
        if let Some(pos) = find_question(part, remaining) {
            remaining = &remaining[pos + question_len(part)..];
        } else {
            return false;
        }
    }

    // Last part.
    let last = parts.last().unwrap();
    if trailing_star {
        // Last segment can match anywhere.
        find_question(last, remaining).is_some()
    } else {
        // Last segment must match at the end.
        remaining.len() >= question_len(last)
            && question_match(last, &remaining[remaining.len() - question_len(last)..])
    }
}

// Helper: length in chars of a `?`-containing pattern segment.
fn question_len(pattern: &str) -> usize {
    pattern.chars().count()
}

// Helper: does `text` exactly match `pattern` where `?` matches one char?
fn question_match(pattern: &str, text: &str) -> bool {
    let mut pt = pattern.chars();
    let mut tt = text.chars();
    loop {
        match (pt.next(), tt.next()) {
            (None, None) => return true,
            (Some('?'), Some(_)) => {}
            (Some(p), Some(t)) if p == t => {}
            _ => return false,
        }
    }
}

// Helper: find the first position in `text` where `pattern` starts (question matching).
fn find_question(pattern: &str, text: &str) -> Option<usize> {
    let plen = question_len(pattern);
    if plen == 0 {
        return Some(0);
    }
    let chars: Vec<char> = text.chars().collect();
    if chars.len() < plen {
        return None;
    }
    for i in 0..=(chars.len() - plen) {
        let slice: String = chars[i..i + plen].iter().collect();
        if question_match(pattern, &slice) {
            return Some(i);
        }
    }
    None
}

// Extension trait for starts_with with `?` patterns.
trait StartsWithQuestion {
    fn starts_with_question(&self, pattern: &str) -> bool;
}

impl StartsWithQuestion for str {
    fn starts_with_question(&self, pattern: &str) -> bool {
        let plen = question_len(pattern);
        if self.chars().count() < plen {
            return false;
        }
        let prefix: String = self.chars().take(plen).collect();
        question_match(pattern, &prefix)
    }
}

// ---------------------------------------------------------------------------
// Directory walking
// ---------------------------------------------------------------------------

fn walk_dir_filtered(
    dir: &Path,
    opts: &ScanOptions,
    glob: &Option<GlobPattern>,
    current_depth: usize,
) -> Result<Vec<PathBuf>, LocalFileCacheError> {
    let mut files = Vec::new();

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        let path = entry.path();

        if ft.is_file() {
            // Extension filter.
            if !opts.extensions.is_empty() {
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                if !opts.extensions.iter().any(|e| e.to_lowercase() == ext) {
                    continue;
                }
            }
            // Glob filter (matched against file name, not full path).
            if let Some(pat) = glob {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !pat.matches(name) {
                    continue;
                }
            }
            files.push(path);
        } else if ft.is_dir() {
            let can_descend =
                opts.recursive && opts.max_depth.is_none_or(|max| current_depth < max);
            if can_descend {
                let sub = walk_dir_filtered(&path, opts, glob, current_depth + 1)?;
                files.extend(sub);
            }
        }
    }

    Ok(files)
}
