//! Background file-system watcher for automatic cache invalidation.
//!
//! [`CacheWatcher`] monitors the source files of cached entries on the OS
//! level.  When a watched file is modified, renamed, or deleted, the
//! corresponding cache entry is removed from the database and a [`WatchEvent`]
//! is sent on the event channel.
//!
//! # Lifetime note
//!
//! The `CacheWatcher` **must remain alive** for events to be delivered.
//! Dropping it stops the OS-level watcher and closes the event channel.
//! Use [`CacheWatcher::events`] to borrow the receiver while keeping the
//! watcher in scope, or spawn a thread that owns the watcher and forwards
//! events via a separate channel.
//!
//! # Example
//!
//! ```no_run
//! use localcache::{CacheEngine, CacheOptions};
//!
//! let engine = CacheEngine::<Vec<f32>>::builder()
//!     .database("cache.sqlite3")
//!     .build()?;
//!
//! let mut watcher = engine.watcher()?;
//! let rx = watcher.events();
//!
//! loop {
//!     match rx.recv() {
//!         Ok(event) => println!("invalidated: {} ({:?})",
//!                               event.path.display(), event.reason),
//!         Err(_) => break, // watcher dropped
//!     }
//! }
//! # Ok::<(), localcache::LocalFileCacheError>(())
//! ```

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher, event};
use serde::{Serialize, de::DeserializeOwned};

use crate::cache::engine::CacheEngine;
use crate::cache::entry::{InvalidationReason, WatchEvent};
use crate::error::LocalFileCacheError;

// ---------------------------------------------------------------------------
// Registration and invalidation diagnostics (RFC 015 R4/R5)
// ---------------------------------------------------------------------------

/// A path that failed OS-level watch registration at construction time.
///
/// Construction still succeeds with partial coverage — this type makes that
/// partial failure observable via
/// [`CacheWatcher::registration_errors`]/[`CacheDebouncedWatcher::registration_errors`]
/// instead of silently discarding it. Each failure is also emitted as a
/// `tracing::warn!` when the `tracing` feature is enabled, so the accessor
/// is an audit trail rather than the only signal.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PathRegistrationError {
    path: PathBuf,
    message: String,
}

impl PathRegistrationError {
    fn new(path: PathBuf, message: String) -> Self {
        Self { path, message }
    }

    /// The path that failed to register with the OS watcher.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The underlying OS watcher error, rendered as text.
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Shared, atomically-updated failure counters for a watcher's background
/// callback. `Relaxed` ordering is sufficient — these are independent
/// monotonic counters with no other memory operation that depends on their
/// ordering relative to a read.
#[derive(Default)]
struct WatcherCounters {
    /// Invalidation events dropped because the bounded notification channel
    /// was full. The underlying cache invalidation already succeeded by the
    /// time a drop can occur — only the *notification* is lost.
    dropped_events: AtomicU64,
    /// Times removing an invalidated entry from the database failed. A
    /// failed removal is counted, not retried, and no notification is sent
    /// for that occurrence.
    failed_invalidations: AtomicU64,
}

// ---------------------------------------------------------------------------
// CacheWatcher
// ---------------------------------------------------------------------------

/// A background file-system watcher tied to a [`CacheEngine`].
///
/// Created via [`CacheEngine::watcher()`].  All source files that have a
/// cached entry at construction time are watched automatically.  Additional
/// paths can be added with [`CacheWatcher::watch`].
///
/// # Important
///
/// The `CacheWatcher` must remain alive for events to be delivered.  Use
/// [`CacheWatcher::events`] (borrows the receiver) or keep the watcher in
/// scope alongside a cloned [`std::sync::mpsc::SyncSender`].
///
/// Dropping the watcher stops the OS watcher and closes the channel.
pub struct CacheWatcher<T> {
    /// Shared inner engine for entry removal on invalidation.
    inner: Arc<WatcherInner<T>>,
    /// The OS-level watcher — kept alive by ownership, dropped with `self`.
    _os_watcher: RecommendedWatcher,
    /// Receiver end of the invalidation event channel.
    rx: mpsc::Receiver<WatchEvent>,
    /// Paths that failed initial OS-level registration at construction time.
    registration_errors: Vec<PathRegistrationError>,
}

struct WatcherInner<T> {
    engine: Mutex<CacheEngine<T>>,
    tx: mpsc::SyncSender<WatchEvent>,
    counters: WatcherCounters,
}

impl<T> CacheWatcher<T>
where
    T: Serialize + DeserializeOwned + Send + 'static,
{
    pub(crate) fn new_with_paths(
        engine: Arc<Mutex<CacheEngine<T>>>,
        paths: Vec<PathBuf>,
        watch_dirs: bool,
    ) -> Result<Self, LocalFileCacheError> {
        // Use a synchronous channel with a generous buffer so the notify
        // callback is never blocked.
        let (tx, rx) = mpsc::sync_channel::<WatchEvent>(256);

        // Build the shared inner state: a *dedicated* engine connection for
        // the watcher callback (SQLite connections are not Send).
        let watcher_engine = {
            let g = engine
                .lock()
                .map_err(|_| LocalFileCacheError::UnsupportedFeature("mutex poisoned".into()))?;
            CacheEngine::<T>::open(crate::cache::options::CacheOptions {
                database_path: g.database_path.clone(),
                change_detection_mode: g.mode,
                codec: g.codec,
                namespace: g.namespace.clone(),
                ttl: g.ttl,
                read_only: false,
                payload_version: g.payload_version,
                ..crate::cache::options::CacheOptions::default()
            })?
        };

        let inner = Arc::new(WatcherInner {
            engine: Mutex::new(watcher_engine),
            tx: tx.clone(),
            counters: WatcherCounters::default(),
        });

        let inner_cb = Arc::clone(&inner);
        let mut os_watcher =
            notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                let Ok(ev) = res else { return };
                let reason = match classify_event(&ev.kind) {
                    Some(r) => r,
                    None => return,
                };
                if let Ok(eng) = inner_cb.engine.lock() {
                    for path in &ev.paths {
                        // With recursive directory watching, OS events arrive
                        // for *all* files in the tree, including files that
                        // were never cached.  Only invalidate (and emit an
                        // event for) paths that actually have a cache entry.
                        // `contains()` is a single indexed SELECT — cheap —
                        // and falls back to a raw-path lookup for files that
                        // no longer exist on disk, so Remove events still
                        // match their stored entry. An *error* from
                        // `contains()` is not evidence the path is
                        // uncached, so only a definite `Ok(false)` skips.
                        if matches!(eng.contains(path), Ok(false)) {
                            continue;
                        }
                        if eng.remove(path).is_err() {
                            inner_cb
                                .counters
                                .failed_invalidations
                                .fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                        if inner_cb
                            .tx
                            .try_send(WatchEvent {
                                path: path.clone(),
                                reason: reason.clone(),
                            })
                            .is_err()
                        {
                            inner_cb
                                .counters
                                .dropped_events
                                .fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            })
            .map_err(|e| {
                LocalFileCacheError::UnsupportedFeature(format!(
                    "failed to create file-system watcher: {e}"
                ))
            })?;

        // Pre-register all currently cached paths (provided by caller):
        // either each file individually (default) or each unique parent
        // directory recursively (`watch_dirs = true`). Construction still
        // succeeds with partial coverage; a registration failure is
        // collected rather than discarded.
        let mut registration_errors = Vec::new();
        if watch_dirs {
            for dir in unique_parent_dirs(&paths) {
                if dir.exists() {
                    if let Err(e) = os_watcher.watch(&dir, RecursiveMode::Recursive) {
                        #[cfg(feature = "tracing")]
                        tracing::warn!(path = %dir.display(), error = %e, "watch registration failed");
                        registration_errors
                            .push(PathRegistrationError::new(dir.clone(), e.to_string()));
                    }
                }
            }
        } else {
            for path in &paths {
                if path.exists() {
                    if let Err(e) = os_watcher.watch(path, RecursiveMode::NonRecursive) {
                        #[cfg(feature = "tracing")]
                        tracing::warn!(path = %path.display(), error = %e, "watch registration failed");
                        registration_errors
                            .push(PathRegistrationError::new(path.clone(), e.to_string()));
                    }
                }
            }
        }

        Ok(Self {
            inner,
            _os_watcher: os_watcher,
            rx,
            registration_errors,
        })
    }

    // ------------------------------------------------------------------
    // Public API
    // ------------------------------------------------------------------

    /// Borrow the invalidation event receiver.
    ///
    /// The watcher **must stay alive** while you read from this receiver.
    pub fn events(&self) -> &mpsc::Receiver<WatchEvent> {
        &self.rx
    }

    /// Start watching an additional `path`.
    ///
    /// Has no effect if the path is already watched or does not exist.
    pub fn watch<P: AsRef<Path>>(&mut self, path: P) -> Result<(), LocalFileCacheError> {
        self._os_watcher
            .watch(path.as_ref(), RecursiveMode::NonRecursive)
            .map_err(|e| {
                LocalFileCacheError::UnsupportedFeature(format!(
                    "watch failed for '{}': {e}",
                    path.as_ref().display()
                ))
            })
    }

    /// Stop watching `path`.
    pub fn unwatch<P: AsRef<Path>>(&mut self, path: P) -> Result<(), LocalFileCacheError> {
        self._os_watcher.unwatch(path.as_ref()).map_err(|e| {
            LocalFileCacheError::UnsupportedFeature(format!(
                "unwatch failed for '{}': {e}",
                path.as_ref().display()
            ))
        })
    }

    /// Watch all files under `dir` **recursively**.
    ///
    /// Any OS event for a file under `dir` that has a corresponding cache
    /// entry triggers invalidation; files without a cache entry are silently
    /// ignored by the callback.  This covers files cached *after* the call,
    /// as long as they live under `dir`.
    ///
    /// Recursive and per-file registrations can coexist on the same watcher.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use localcache::CacheEngine;
    /// # let engine = CacheEngine::<Vec<f32>>::builder()
    /// #     .database("cache.sqlite3")
    /// #     .build()?;
    /// let mut watcher = engine.watcher()?;
    /// watcher.watch_dir("/data/documents")?;
    /// # Ok::<(), localcache::LocalFileCacheError>(())
    /// ```
    pub fn watch_dir<P: AsRef<Path>>(&mut self, dir: P) -> Result<(), LocalFileCacheError> {
        self._os_watcher
            .watch(dir.as_ref(), RecursiveMode::Recursive)
            .map_err(|e| {
                LocalFileCacheError::UnsupportedFeature(format!(
                    "watch_dir failed for '{}': {e}",
                    dir.as_ref().display()
                ))
            })
    }

    /// Stop watching the directory `dir` (and its subtree).
    pub fn unwatch_dir<P: AsRef<Path>>(&mut self, dir: P) -> Result<(), LocalFileCacheError> {
        self._os_watcher.unwatch(dir.as_ref()).map_err(|e| {
            LocalFileCacheError::UnsupportedFeature(format!(
                "unwatch_dir failed for '{}': {e}",
                dir.as_ref().display()
            ))
        })
    }

    /// Number of entries currently cached in the watcher's engine snapshot.
    pub fn watched_count(&self) -> usize {
        self.inner
            .engine
            .lock()
            .map(|g| g.entry_count().unwrap_or(0))
            .unwrap_or(0)
    }

    /// Paths that failed OS-level watch registration at construction time.
    ///
    /// Construction still succeeds with partial coverage — this is how a
    /// partial failure becomes observable. Each failure is also emitted as a
    /// `tracing::warn!` when the `tracing` feature is enabled.
    pub fn registration_errors(&self) -> &[PathRegistrationError] {
        &self.registration_errors
    }

    /// Number of invalidation events dropped because the notification
    /// channel (bounded to 256 events) was full.
    ///
    /// The underlying cache invalidation is unaffected — a dropped event
    /// only means the *notification* was lost. See
    /// [`CacheWatcher::failed_invalidation_count`] for invalidation itself
    /// failing.
    pub fn dropped_event_count(&self) -> u64 {
        self.inner.counters.dropped_events.load(Ordering::Relaxed)
    }

    /// Number of times removing an invalidated entry from the database
    /// failed.
    ///
    /// A failed removal is counted, not retried, and no notification is
    /// sent for that occurrence.
    pub fn failed_invalidation_count(&self) -> u64 {
        self.inner
            .counters
            .failed_invalidations
            .load(Ordering::Relaxed)
    }
}

// ---------------------------------------------------------------------------
// Event classification helper
// ---------------------------------------------------------------------------

fn classify_event(kind: &EventKind) -> Option<InvalidationReason> {
    match kind {
        EventKind::Modify(
            event::ModifyKind::Data(_)
            | event::ModifyKind::Metadata(_)
            | event::ModifyKind::Any
            | event::ModifyKind::Other,
        ) => Some(InvalidationReason::FileModified),

        EventKind::Remove(_) => Some(InvalidationReason::FileRemoved),

        // A Create on a watched path means truncate+rewrite.
        EventKind::Create(_) => Some(InvalidationReason::FileModified),

        EventKind::Modify(event::ModifyKind::Name(_)) => Some(InvalidationReason::FileRenamed),

        // Access, Other, Unknown — not actionable.
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// CacheDebouncedWatcher
// ---------------------------------------------------------------------------

/// A debounced background file-system watcher.
///
/// Created via [`CacheEngine::debounced_watcher()`].  File-system events
/// within `window` of each other are merged into a single [`WatchEvent`],
/// preventing floods of invalidation messages during rapid writes.
///
/// Like [`CacheWatcher`], this type must remain alive for events to be
/// delivered.
pub struct CacheDebouncedWatcher<T> {
    /// Dedicated engine for the callback thread.
    _inner: std::sync::Arc<std::sync::Mutex<CacheEngine<T>>>,
    /// The OS-level debounced watcher (kept alive).
    _debouncer: notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>,
    /// Receiver for deduplicated invalidation events.
    rx: std::sync::mpsc::Receiver<WatchEvent>,
    /// Paths that failed initial OS-level registration at construction time.
    registration_errors: Vec<PathRegistrationError>,
    /// Shared failure counters, updated from the debounce callback.
    counters: Arc<WatcherCounters>,
}

impl<T> CacheDebouncedWatcher<T>
where
    T: Serialize + DeserializeOwned + Send + 'static,
{
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_with_paths(
        database_path: std::path::PathBuf,
        mode: crate::cache::options::ChangeDetectionMode,
        codec: crate::cache::options::Codec,
        namespace: String,
        ttl: Option<std::time::Duration>,
        payload_version: u32,
        paths: Vec<PathBuf>,
        window: std::time::Duration,
        watch_dirs: bool,
    ) -> Result<Self, LocalFileCacheError> {
        use std::sync::{Arc, Mutex, mpsc};

        let watcher_engine = CacheEngine::<T>::open(crate::cache::options::CacheOptions {
            database_path,
            change_detection_mode: mode,
            codec,
            namespace,
            ttl,
            read_only: false,
            payload_version,
            ..crate::cache::options::CacheOptions::default()
        })?;

        let inner = Arc::new(Mutex::new(watcher_engine));
        let (tx, rx) = mpsc::sync_channel::<WatchEvent>(256);
        let inner_cb = Arc::clone(&inner);
        let counters = Arc::new(WatcherCounters::default());
        let counters_cb = Arc::clone(&counters);

        let debouncer = notify_debouncer_mini::new_debouncer(
            window,
            move |res: notify_debouncer_mini::DebounceEventResult| {
                let events = match res {
                    Ok(evs) => evs,
                    Err(_) => return,
                };
                // Deduplicate paths within the debounce window.
                let mut seen = std::collections::HashSet::new();
                for ev in events {
                    // DebouncedEvent has a single `path` field (not `paths`).
                    let path = ev.path;
                    if seen.insert(path.clone()) {
                        // DebouncedEventKind has only Any / AnyContinuous —
                        // no remove variant; treat all as FileModified.
                        let reason = InvalidationReason::FileModified;
                        let mut invalidation_failed = false;
                        if let Ok(eng) = inner_cb.lock() {
                            // Recursive directory watching delivers events
                            // for uncached files too — filter them out. An
                            // *error* from `contains()` is not evidence the
                            // path is uncached, so only a definite
                            // `Ok(false)` skips.
                            if matches!(eng.contains(&path), Ok(false)) {
                                continue;
                            }
                            if eng.remove(&path).is_err() {
                                counters_cb
                                    .failed_invalidations
                                    .fetch_add(1, Ordering::Relaxed);
                                invalidation_failed = true;
                            }
                        }
                        if invalidation_failed {
                            // Removal was attempted and failed: count it,
                            // don't retry, and don't send a notification
                            // claiming invalidation happened.
                            continue;
                        }
                        if tx.try_send(WatchEvent { path, reason }).is_err() {
                            counters_cb.dropped_events.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            },
        )
        .map_err(|e| {
            LocalFileCacheError::UnsupportedFeature(format!(
                "failed to create debounced watcher: {e}"
            ))
        })?;

        // Register all pre-existing cached paths — per file (default) or per
        // unique parent directory, recursively (`watch_dirs = true`).
        // Construction still succeeds with partial coverage; a registration
        // failure is collected rather than discarded.
        {
            let mut deb = debouncer;
            let mut registration_errors = Vec::new();
            if watch_dirs {
                for dir in unique_parent_dirs(&paths) {
                    if dir.exists() {
                        if let Err(e) = deb.watcher().watch(&dir, RecursiveMode::Recursive) {
                            #[cfg(feature = "tracing")]
                            tracing::warn!(path = %dir.display(), error = %e, "watch registration failed");
                            registration_errors
                                .push(PathRegistrationError::new(dir.clone(), e.to_string()));
                        }
                    }
                }
            } else {
                for path in &paths {
                    if path.exists() {
                        if let Err(e) = deb.watcher().watch(path, RecursiveMode::NonRecursive) {
                            #[cfg(feature = "tracing")]
                            tracing::warn!(path = %path.display(), error = %e, "watch registration failed");
                            registration_errors
                                .push(PathRegistrationError::new(path.clone(), e.to_string()));
                        }
                    }
                }
            }
            Ok(Self {
                _inner: inner,
                _debouncer: deb,
                rx,
                registration_errors,
                counters,
            })
        }
    }

    /// Borrow the deduplicated event receiver.
    ///
    /// The watcher must stay alive while reading.
    pub fn events(&self) -> &std::sync::mpsc::Receiver<WatchEvent> {
        &self.rx
    }

    /// Watch all files under `dir` **recursively** (debounced).
    ///
    /// See [`CacheWatcher::watch_dir`] — identical semantics with debounced
    /// event delivery.
    pub fn watch_dir<P: AsRef<Path>>(&mut self, dir: P) -> Result<(), LocalFileCacheError> {
        self._debouncer
            .watcher()
            .watch(dir.as_ref(), RecursiveMode::Recursive)
            .map_err(|e| {
                LocalFileCacheError::UnsupportedFeature(format!(
                    "watch_dir failed for '{}': {e}",
                    dir.as_ref().display()
                ))
            })
    }

    /// Stop watching the directory `dir` (and its subtree).
    pub fn unwatch_dir<P: AsRef<Path>>(&mut self, dir: P) -> Result<(), LocalFileCacheError> {
        self._debouncer
            .watcher()
            .unwatch(dir.as_ref())
            .map_err(|e| {
                LocalFileCacheError::UnsupportedFeature(format!(
                    "unwatch_dir failed for '{}': {e}",
                    dir.as_ref().display()
                ))
            })
    }

    /// Paths that failed OS-level watch registration at construction time.
    ///
    /// See [`CacheWatcher::registration_errors`] — identical semantics.
    pub fn registration_errors(&self) -> &[PathRegistrationError] {
        &self.registration_errors
    }

    /// Number of invalidation events dropped because the notification
    /// channel (bounded to 256 events) was full.
    ///
    /// See [`CacheWatcher::dropped_event_count`] — identical semantics.
    pub fn dropped_event_count(&self) -> u64 {
        self.counters.dropped_events.load(Ordering::Relaxed)
    }

    /// Number of times removing an invalidated entry from the database
    /// failed.
    ///
    /// See [`CacheWatcher::failed_invalidation_count`] — identical
    /// semantics.
    pub fn failed_invalidation_count(&self) -> u64 {
        self.counters.failed_invalidations.load(Ordering::Relaxed)
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Collect the unique parent directories of `paths`, deepest path untouched.
///
/// Used by the `watch_dirs` registration mode: instead of one OS watch per
/// file, one **recursive** OS watch per distinct parent directory.
fn unique_parent_dirs(paths: &[PathBuf]) -> Vec<PathBuf> {
    let set: std::collections::HashSet<&Path> = paths.iter().filter_map(|p| p.parent()).collect();
    set.into_iter().map(Path::to_path_buf).collect()
}
