//! Unit tests needing an interleaving hook the public API cannot reach:
//! RFC 022 R1 Amendment 2 (C2) key-rotation locking, and RFC 022 R6 item 3
//! (Q0c review C1) failed-eviction atomicity. `TestPoint`/`TEST_HOOK` are
//! declared in `super` (`cache::engine`), not gated on any Cargo feature,
//! so both tests below can share the same hook plumbing.

use std::path::PathBuf;

use tempfile::TempDir;

use super::*;
use crate::CacheEngine;

struct HookGuard;

impl Drop for HookGuard {
    fn drop(&mut self) {
        TEST_HOOK.with(|slot| *slot.borrow_mut() = None);
    }
}

fn set_hook(hook: impl FnMut(TestPoint) -> Result<(), LocalFileCacheError> + 'static) -> HookGuard {
    TEST_HOOK.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
    HookGuard
}

fn write_file(dir: &TempDir, name: &str) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, b"x").unwrap();
    path
}

/// RFC 022 R1 Amendment 2 (C2) — `rotate_encryption_key` must hold the
/// database write lock from its first read to its commit, so a concurrent
/// write cannot land between the load and the `UPDATE`s and then be
/// silently overwritten with a stale re-encrypted payload.
#[cfg(feature = "encryption")]
fn key(seed: u8) -> Vec<u8> {
    vec![seed; 32]
}

#[cfg(feature = "encryption")]
#[test]
fn rotation_holds_the_write_lock_so_a_concurrent_write_is_refused_with_busy() {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use rusqlite::Connection;

    let dir = TempDir::new().unwrap();
    let db = dir.path().join("rot_race.sqlite3");

    let engine: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(&db)
        .encryption_key(key(0x31))
        .build()
        .unwrap();
    let path = write_file(&dir, "race.txt");
    engine.set(&path, &vec![1.0_f32]).unwrap();

    let stored_path = engine.keys(None).unwrap()[0].to_str().unwrap().to_owned();
    const MARKER: &[u8] = b"CONCURRENT-WRITE-MARKER-BYTES!!";

    // Recorded from inside the hook: `Some((is_busy, debug_text))` describing
    // the concurrent writer's result, or `None` if the hook never ran.
    let write_result: Arc<Mutex<Option<(bool, String)>>> = Arc::new(Mutex::new(None));
    let write_result_in_hook = Arc::clone(&write_result);
    let db_in_hook = db.clone();
    let stored_path_in_hook = stored_path.clone();

    let hook = set_hook(move |point| {
        if point == TestPoint::AfterLoad {
            // The rotating transaction holds the write lock at this point
            // (BEGIN IMMEDIATE, opened before the load). A concurrent
            // writer that refuses to wait for it must fail immediately,
            // never block -- a real second `CacheEngine` would use the
            // default busy timeout and stall this test for seconds.
            let writer = Connection::open(&db_in_hook).unwrap();
            writer.busy_timeout(Duration::ZERO).unwrap();
            let result = writer.execute(
                "UPDATE payloads SET content = ?1
                 WHERE file_id = (SELECT id FROM files WHERE path = ?2)",
                rusqlite::params![MARKER, stored_path_in_hook],
            );
            // Under a zero-timeout writer and an `IMMEDIATE` lock held since
            // before the load, the only correct outcome is `SQLITE_BUSY`.
            // Any other outcome -- success, or a different error such as a
            // misspelled column or a closed file -- means the lock was not
            // actually held at this point, which is the regression this
            // test exists to catch, so only `DatabaseBusy` counts.
            let is_busy = matches!(
                &result,
                Err(rusqlite::Error::SqliteFailure(e, _))
                    if e.code == rusqlite::ErrorCode::DatabaseBusy
            );
            *write_result_in_hook.lock().unwrap() = Some((is_busy, format!("{result:?}")));
        }
        Ok(())
    });

    let rotated = engine.rotate_encryption_key(&key(0x32)).unwrap();
    drop(hook);

    assert_eq!(rotated, 1);
    let (refused_with_busy, recorded_result) = write_result
        .lock()
        .unwrap()
        .clone()
        .expect("the AfterLoad hook must have run");
    assert!(
        refused_with_busy,
        "concurrent write during rotation must be refused with SQLITE_BUSY -- \
         the IMMEDIATE lock was not held at the load point (writer result: {recorded_result})"
    );

    // Positive check: the rotation completed intact under the new key, not
    // merely that the marker bytes are absent from the raw row.
    let entry = engine
        .get(&path)
        .unwrap_or_else(|err| panic!("entry must decode through the rotating engine: {err}"))
        .expect("entry must still exist");
    assert_eq!(entry.payload, vec![1.0_f32]);
}

/// RFC 022 R6 item 3 (Q0c review C1) — a write and its eviction share one
/// `IMMEDIATE` transaction, so a failed eviction leaves nothing changed:
/// `Err` from `set` means the entry was never stored, not "stored, then a
/// separate eviction step failed afterwards".
#[test]
fn failed_eviction_during_set_leaves_nothing_changed() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(":memory:")
        .max_entries(1)
        .build()
        .unwrap();

    let pa = write_file(&dir, "a.txt");
    engine.set(&pa, &vec![1.0_f32]).unwrap();

    let hook = set_hook(|point| {
        if point == TestPoint::BeforeEviction {
            return Err(LocalFileCacheError::UnsupportedFeature(
                "forced eviction failure for the test".into(),
            ));
        }
        Ok(())
    });

    let pb = write_file(&dir, "b.txt");
    let result = engine.set(&pb, &vec![2.0_f32]);
    drop(hook);

    assert!(
        result.is_err(),
        "set must fail when eviction fails, got {result:?}"
    );
    assert!(
        !engine.contains(&pb).unwrap(),
        "b must not be stored when set returns Err"
    );
    assert!(
        engine.contains(&pa).unwrap(),
        "the original entry must be untouched"
    );
}

// ---------------------------------------------------------------------------
// RFC 025 R1 — `ttl_remaining`, the one place TTL arithmetic happens
// ---------------------------------------------------------------------------

fn secs(n: u64) -> Option<Duration> {
    Some(Duration::from_secs(n))
}

#[test]
fn ttl_remaining_is_none_without_a_ttl() {
    assert_eq!(ttl_remaining(1000, 0, None), None);
    assert_eq!(ttl_remaining(i64::MIN, i64::MAX, None), None);
    assert_eq!(ttl_remaining(i64::MAX, i64::MIN, None), None);
}

#[test]
fn ttl_remaining_at_before_and_after_the_ttl() {
    let now = 1_000_000;
    for ttl in [1_u64, 3600] {
        let age = |a: i64| now - a; // updated_at for a given age
        let ttl_i = ttl as i64;
        assert_eq!(
            ttl_remaining(now, age(ttl_i - 1), secs(ttl)),
            Some(1),
            "one second before the TTL, ttl={ttl}"
        );
        assert_eq!(
            ttl_remaining(now, age(ttl_i), secs(ttl)),
            Some(0),
            "at the TTL, ttl={ttl}"
        );
        assert_eq!(
            ttl_remaining(now, age(ttl_i + 1), secs(ttl)),
            Some(0),
            "one second after the TTL, ttl={ttl}"
        );
    }
    // A brand-new entry has the whole TTL left.
    assert_eq!(ttl_remaining(now, now, secs(3600)), Some(3600));
}

#[test]
fn ttl_remaining_zero_ttl_is_always_expired() {
    assert_eq!(ttl_remaining(1000, 1000, secs(0)), Some(0));
    assert_eq!(ttl_remaining(1000, 5000, secs(0)), Some(0));
}

/// An `updated_at` in the future counts as age zero: the whole TTL remains.
#[test]
fn ttl_remaining_clamps_a_future_updated_at_to_age_zero() {
    assert_eq!(ttl_remaining(1000, 1001, secs(3600)), Some(3600));
    assert_eq!(ttl_remaining(1000, 1_000_000, secs(3600)), Some(3600));
    assert_eq!(ttl_remaining(1000, i64::MAX, secs(3600)), Some(3600));
    assert_eq!(ttl_remaining(i64::MIN, 0, secs(3600)), Some(3600));
    assert_eq!(ttl_remaining(i64::MIN, i64::MAX, secs(3600)), Some(3600));
}

#[test]
fn ttl_remaining_saturates_at_the_extremes_without_panicking() {
    // The subtraction saturates at `i64::MAX`, far past any TTL below it.
    assert_eq!(ttl_remaining(1000, i64::MIN, secs(3600)), Some(0));
    assert_eq!(ttl_remaining(i64::MAX, i64::MIN, secs(3600)), Some(0));

    // `Duration::MAX` is `u64::MAX` seconds. Nothing wraps or reads as expired.
    let max = Some(Duration::MAX);
    assert_eq!(ttl_remaining(1000, 1000, max), Some(u64::MAX));
    assert_eq!(ttl_remaining(1000, 0, max), Some(u64::MAX - 1000));
    assert_eq!(ttl_remaining(1000, 1001, max), Some(u64::MAX));
    assert_eq!(
        ttl_remaining(i64::MAX, i64::MIN, max),
        Some(u64::MAX - i64::MAX as u64),
        "the widest possible age still leaves a positive remainder"
    );
}

/// `is_expired` is `ttl_remaining(now, ..) == Some(0)`, so the two cannot
/// disagree, including for a future `updated_at`.
#[test]
fn is_expired_follows_ttl_remaining() {
    let now = repository::now_secs();
    assert!(!is_expired(now, None));
    assert!(!is_expired(now + 1000, secs(3600)));
    assert!(!is_expired(i64::MAX, secs(3600)));
    assert!(!is_expired(now, Some(Duration::MAX)));
    assert!(is_expired(now - 7200, secs(3600)));
    assert!(is_expired(i64::MIN, secs(3600)));
    assert!(is_expired(now, secs(0)));
}

/// `explain` judges `status`, `ttl_remaining_secs` and the expired flag at one
/// `now`. With the clock controlled, the three can be checked at the expiry
/// boundary, where two separate clock reads could once have contradicted each
/// other (RFC 025 R1, review 024 K1).
#[test]
fn explain_at_judges_status_and_ttl_at_one_instant() {
    let dir = TempDir::new().unwrap();
    let engine: CacheEngine<Vec<f32>> = CacheEngine::builder()
        .database(":memory:")
        .ttl(Duration::from_secs(60))
        .build()
        .unwrap();
    let path = write_file(&dir, "boundary.txt");
    engine.set(&path, &vec![1.0]).unwrap();
    let updated_at: i64 = engine
        .conn
        .query_row("SELECT updated_at FROM files", [], |row| row.get(0))
        .unwrap();

    for (age, expired) in [(59_i64, false), (60, true), (61, true)] {
        let diagnosis = engine.explain_at(&path, updated_at + age).unwrap();
        let status_stale = diagnosis.status == crate::cache::entry::CacheStatus::Stale;
        let remaining_zero = diagnosis.ttl_remaining_secs == Some(0);
        assert_eq!(status_stale, expired, "status at age {age}");
        assert_eq!(remaining_zero, expired, "ttl_remaining_secs at age {age}");
        assert_eq!(
            diagnosis.summary.starts_with("TTL expired"),
            expired,
            "summary at age {age}: {}",
            diagnosis.summary
        );
        assert_eq!(
            engine.check_status_at(&path, updated_at + age).unwrap(),
            diagnosis.status,
            "check_status_at agrees at age {age}"
        );
    }
    // A clock stepped back: age zero, the whole TTL remains, and it is fresh.
    let earlier = engine.explain_at(&path, updated_at - 500).unwrap();
    assert_eq!(earlier.status, crate::cache::entry::CacheStatus::Fresh);
    assert_eq!(earlier.ttl_remaining_secs, Some(60));
}
