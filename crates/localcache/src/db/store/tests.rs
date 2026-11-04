//! RFC 026 / RFC 030 slice 3: the use order, at the layer where it lives, with
//! a clock the test controls ([`test_clock`], private to `db`).
//!
//! The engine's own tests (`cache/engine/use_order_tests.rs`) run the same
//! scenarios with the real clock. The cases that depend on the clock itself —
//! one that never moves, one that runs backwards, uses inside one microsecond —
//! are here, against the [`Store`] and the eviction rule the engine applies
//! around it (count, then evict the excess, protecting the rows just written).

use crate::db::repository::test_clock;
use crate::db::{
    FileMetadata, JournalMode, Location, Store, StoreConfig, SynchronousMode, WriteTx,
};

fn store() -> Store {
    Store::open(&StoreConfig {
        location: Location::Memory,
        namespace: "default".to_owned(),
        read_only: false,
        journal_mode: JournalMode::default(),
        synchronous: SynchronousMode::default(),
    })
    .unwrap()
}

fn metadata() -> FileMetadata {
    FileMetadata {
        mtime: 1,
        file_size: 1,
        hash: None,
    }
}

/// Bound the namespace to `max_entries`, never evicting `protected`.
fn bound(tx: &WriteTx<'_>, max_entries: usize, protected: &[i64]) {
    let count = tx.count().unwrap();
    if count > max_entries {
        tx.evict_lru(count - max_entries, protected).unwrap();
    }
}

/// What `CacheEngine::set` does: write, evict, commit, in one transaction.
fn set(store: &Store, path: &str, max_entries: usize) {
    let tx = store.begin_write().unwrap();
    let id = tx.upsert(path, &metadata(), &[1], "raw", 1).unwrap();
    bound(&tx, max_entries, &[id]);
    tx.commit().unwrap();
}

/// What `CacheEngine::batch_set` does.
fn batch_set(store: &Store, paths: &[&str], max_entries: usize) {
    let tx = store.begin_write().unwrap();
    let ids: Vec<i64> = paths
        .iter()
        .map(|path| tx.upsert(path, &metadata(), &[1], "raw", 1).unwrap())
        .collect();
    bound(&tx, max_entries, &ids);
    tx.commit().unwrap();
}

/// A read hit or a `touch`: the row becomes the newest of its namespace.
fn used(store: &Store, path: &str) {
    let row = store.find_file(path).unwrap().unwrap();
    store.record_use(row.id).unwrap();
}

fn survivors(store: &Store) -> String {
    let mut names: Vec<String> = store
        .keys(None, None, None)
        .unwrap()
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    names.sort();
    names.join(",")
}

type Clock = Box<dyn FnMut() -> i64>;

/// The clocks the order must not depend on.
fn clocks() -> Vec<(&'static str, Clock)> {
    let mut backwards = 1_800_000_000_000_000_i64;
    vec![
        (
            "the real clock",
            Box::new(|| std::time::UNIX_EPOCH.elapsed().unwrap().as_micros() as i64),
        ),
        (
            "a clock that never moves",
            Box::new(|| 1_800_000_000_000_000),
        ),
        (
            "a clock that runs backwards",
            Box::new(move || {
                backwards -= 5_000_000;
                backwards
            }),
        ),
    ]
}

/// The pathology of RFC 026: before v0.23.0 an entry read once, ever, outlived
/// every entry that was only written, so the survivors were `a`, `g`, `h`.
#[test]
fn an_entry_read_once_is_not_immortal_under_any_clock() {
    for (label, clock) in clocks() {
        let _guard = test_clock::set(clock);
        let store = store();
        for name in ["a", "b", "c"] {
            set(&store, name, 3);
        }
        used(&store, "a");
        for name in ["d", "e", "f", "g", "h"] {
            set(&store, name, 3);
        }
        assert_eq!(survivors(&store), "f,g,h", "{label}");
    }
}

#[test]
fn touch_protects_from_eviction_even_inside_one_microsecond() {
    // Every use happens at the same instant.
    let _guard = test_clock::set(|| 1_800_000_000_000_000);
    let store = store();
    for name in ["a", "b", "c"] {
        set(&store, name, 3);
    }
    used(&store, "a");
    set(&store, "d", 3);
    // `b` is the oldest use; `a`, touched after it was written, is protected.
    assert_eq!(survivors(&store), "a,c,d");
}

#[test]
fn a_read_hit_protects_from_eviction_even_inside_one_microsecond() {
    let _guard = test_clock::set(|| 1_800_000_000_000_000);
    let store = store();
    set(&store, "a", 2);
    set(&store, "b", 2);
    used(&store, "a");
    set(&store, "c", 2);
    assert_eq!(survivors(&store), "a,c");
}

#[test]
fn an_overwrite_is_a_use_even_inside_one_microsecond() {
    let _guard = test_clock::set(|| 1_800_000_000_000_000);
    let store = store();
    set(&store, "a", 2);
    set(&store, "b", 2);
    set(&store, "a", 2); // `a` is now newer than `b`
    set(&store, "c", 2);
    assert_eq!(survivors(&store), "a,c");
}

#[test]
fn a_batch_is_a_run_of_uses_and_never_evicts_its_own_rows() {
    let _guard = test_clock::set(|| 1_800_000_000_000_000);
    let store = store();
    set(&store, "a", 3);
    set(&store, "b", 3);
    batch_set(&store, &["c", "d"], 3);
    assert_eq!(survivors(&store), "b,c,d");
}
