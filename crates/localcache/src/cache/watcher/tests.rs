use super::*;
use crate::cache::engine::CacheEngine;
use crate::error::LocalFileCacheError;

// RFC 018 R2/R4 — `CacheWatcher::new_with_paths` locks the caller-supplied
// shared engine before building its own dedicated connection. This is the
// one poisoning site unreachable from the public API (`CacheEngine::watcher`
// always hands it a freshly created, never-externally-visible mutex), so it
// is covered here rather than in an integration test.
#[test]
fn new_with_paths_poisoned_engine_lock_yields_poisoned_error() {
    let engine: CacheEngine<Vec<f32>> =
        CacheEngine::builder().database(":memory:").build().unwrap();
    let shared = Arc::new(Mutex::new(engine));

    // Poison the mutex: panic while a lock guard is held, caught so the test
    // process itself does not abort.
    let poison_target = Arc::clone(&shared);
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = poison_target.lock().unwrap();
        panic!("intentional test panic");
    }));

    let result = CacheWatcher::<Vec<f32>>::new_with_paths(shared, Vec::new(), false);
    match result {
        Err(LocalFileCacheError::Poisoned {
            resource: "CacheWatcher",
        }) => {}
        Err(other) => panic!("expected Poisoned {{ resource: \"CacheWatcher\" }}, got {other:?}"),
        Ok(_) => panic!("expected Poisoned {{ resource: \"CacheWatcher\" }}, got Ok"),
    }
}
