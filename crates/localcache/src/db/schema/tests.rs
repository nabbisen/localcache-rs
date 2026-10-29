//! Focused RFC 010 transactional migration tests.

mod atomicity;
mod configuration;
mod helpers;
mod numeric;
mod read_only;

/// The only test that names the current schema version. Raising
/// `CURRENT_VERSION` is a deliberate edit of this line, together with the new
/// migration, the classifier tables and the fixtures; it must not happen as a
/// side effect of another change.
#[test]
fn current_schema_version_is_pinned() {
    assert_eq!(super::CURRENT_VERSION, 5);
}
