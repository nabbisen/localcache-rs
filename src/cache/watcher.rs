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
use std::sync::{Arc, Mutex, mpsc};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher, event};
use serde::{Serialize, de::DeserializeOwned};

use crate::cache::engine::CacheEngine;
use crate::cache::entry::{InvalidationReason, WatchEvent};
use crate::error::LocalFileCacheError;

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
}

struct WatcherInner<T> {
    engine: Mutex<CacheEngine<T>>,
    tx: mpsc::SyncSender<WatchEvent>,
}

impl<T> CacheWatcher<T>
where
    T: Serialize + DeserializeOwned + Send + 'static,
{
    pub(crate) fn new_with_paths(
        engine: Arc<Mutex<CacheEngine<T>>>,
        paths: Vec<PathBuf>,
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
                        let _ = eng.remove(path);
                        let _ = inner_cb.tx.try_send(WatchEvent {
                            path: path.clone(),
                            reason: reason.clone(),
                        });
                    }
                }
            })
            .map_err(|e| {
                LocalFileCacheError::UnsupportedFeature(format!(
                    "failed to create file-system watcher: {e}"
                ))
            })?;

        // Pre-register all currently cached paths (provided by caller).
        for path in &paths {
            if path.exists() {
                let _ = os_watcher.watch(path, RecursiveMode::NonRecursive);
            }
        }

        Ok(Self {
            inner,
            _os_watcher: os_watcher,
            rx,
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

    /// Number of entries currently cached in the watcher's engine snapshot.
    pub fn watched_count(&self) -> usize {
        self.inner
            .engine
            .lock()
            .map(|g| g.entry_count().unwrap_or(0))
            .unwrap_or(0)
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
