//! A clock a test can fix, in the style of the crate's `TEST_HOOK`s: a
//! thread-local override of [`super::now_us`].

use std::cell::RefCell;

type Clock = Box<dyn FnMut() -> i64>;

thread_local! {
    static CLOCK: RefCell<Option<Clock>> = const { RefCell::new(None) };
}

/// Restores the real clock when dropped.
pub(crate) struct Guard;

impl Drop for Guard {
    fn drop(&mut self) {
        CLOCK.with(|slot| *slot.borrow_mut() = None);
    }
}

/// Make `now_us` return `clock()` on this thread until the guard drops.
pub(crate) fn set(clock: impl FnMut() -> i64 + 'static) -> Guard {
    CLOCK.with(|slot| *slot.borrow_mut() = Some(Box::new(clock)));
    Guard
}

pub(super) fn next() -> Option<i64> {
    CLOCK.with(|slot| slot.borrow_mut().as_mut().map(|clock| clock()))
}
