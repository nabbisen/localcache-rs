//! Wrapper-parity test (RFC 024 R3).
//!
//! `SyncCacheEngine`, `ReadPool` and `AsyncCacheEngine` are supposed to expose every
//! public `CacheEngine` method, apart from a short, reasoned list. Nothing else compares the four
//! surfaces, so a new engine method used to depend on someone remembering three more files. This
//! test reads the sources, collects the method names, and fails when a wrapper falls behind.
//!
//! * The scan looks only at the **inherent `impl` blocks of the named type**, from the line that
//!   starts `impl … TYPE` to the next `}` at column 0. It collects `    pub fn NAME` and
//!   `    pub async fn NAME` (rustfmt fixes that indentation). It therefore ignores trait impls
//!   such as `impl CacheOptionsExt for CacheOptions` and every private helper type.
//! * It sees `#[cfg]`-gated methods whatever features are enabled, on purpose: the surface is the
//!   same in every build.
//! * Engine methods marked `#[deprecated]` are skipped. A deprecated method is never newly
//!   delegated (RFC 024 R1), so it needs no manifest entry either.
//! * Name parity is the contract. Whether a delegation keeps the engine's parameters and result
//!   type is reviewed by hand.

use std::collections::BTreeSet;

const ENGINE_FILES: &[(&str, &str)] = &[
    (
        "src/cache/engine.rs",
        include_str!("../src/cache/engine.rs"),
    ),
    (
        "src/cache/engine/maintenance.rs",
        include_str!("../src/cache/engine/maintenance.rs"),
    ),
    (
        "src/cache/engine/portable.rs",
        include_str!("../src/cache/engine/portable.rs"),
    ),
    (
        "src/cache/engine/diagnose.rs",
        include_str!("../src/cache/engine/diagnose.rs"),
    ),
];

const SYNC_SOURCE: &str = include_str!("../src/pool.rs");
const READ_POOL_SOURCE: &str = include_str!("../src/read_pool.rs");
const ASYNC_SOURCE: &str = include_str!("../src/cache/async_engine.rs");

const SYNC: &str = "SyncCacheEngine";
const READ_POOL: &str = "ReadPool";
const ASYNC: &str = "AsyncCacheEngine";

/// The only reasons a manifest entry may give.
const REASONS: &[&str] = &[
    "constructor",
    "borrows the engine",
    "writes",
    "spawn_blocking boundary (RFC 024 R2)",
];

/// Methods every wrapper must have that `CacheEngine` does not: the constructor and the closure
/// forms that stand in for `query`, which the manifest records as "borrows the engine". Without
/// this check the manifest would accept a wrapper that had no way to run a query at all.
const WRAPPER_REQUIRED: &[&str] = &["open", "query_run", "query_run_report", "query_dry_run"];

/// Every engine method a wrapper deliberately does not delegate, with the reason.
///
/// `(wrapper, engine method, reason)`. Deprecated engine methods are not listed: the scan
/// skips them.
const MANIFEST: &[(&str, &str, &str)] = &[
    // Each wrapper has its own `open`; `builder` is the engine's constructor.
    (SYNC, "builder", "constructor"),
    (READ_POOL, "builder", "constructor"),
    (ASYNC, "builder", "constructor"),
    // `query` returns a builder that borrows the engine. The wrappers take a closure instead;
    // `WRAPPER_REQUIRED` below checks that those substitutes exist.
    (SYNC, "query", "borrows the engine"),
    (READ_POOL, "query", "borrows the engine"),
    (ASYNC, "query", "borrows the engine"),
    // `ReadPool` never writes, and a watcher removes entries, so it counts as a write.
    (READ_POOL, "set", "writes"),
    (READ_POOL, "batch_set", "writes"),
    (READ_POOL, "remove", "writes"),
    (READ_POOL, "touch", "writes"),
    (READ_POOL, "preload", "writes"),
    (READ_POOL, "rotate_encryption_key", "writes"),
    (READ_POOL, "cleanup_missing_files", "writes"),
    (READ_POOL, "cleanup_expired", "writes"),
    (READ_POOL, "purge_stale_versions", "writes"),
    (READ_POOL, "shrink_database", "writes"),
    (READ_POOL, "import_entries", "writes"),
    (READ_POOL, "import_from", "writes"),
    (READ_POOL, "watcher", "writes"),
    (READ_POOL, "debounced_watcher", "writes"),
    // Its source argument is a `&CacheEngine<U>` that would have to cross `spawn_blocking`.
    // `export_entries` + `import_entries` is the supported route.
    (ASYNC, "import_from", "spawn_blocking boundary (RFC 024 R2)"),
];

/// The names of the `pub fn` / `pub async fn` items in the inherent `impl` blocks of `type_name`
/// in `source`. With `skip_deprecated`, a method preceded by `#[deprecated…]` is left out.
fn inherent_methods(source: &str, type_name: &str, skip_deprecated: bool) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut in_block = false;
    let mut deprecated = false;
    for line in source.lines() {
        if !in_block {
            in_block = starts_inherent_impl(line, type_name);
            deprecated = false;
            continue;
        }
        if line == "}" {
            in_block = false;
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with("#[deprecated") {
            deprecated = true;
        }
        let name = line
            .strip_prefix("    pub fn ")
            .or_else(|| line.strip_prefix("    pub async fn "));
        if let Some(rest) = name {
            let end = rest
                .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                .unwrap_or(rest.len());
            if !(skip_deprecated && deprecated) {
                names.insert(rest[..end].to_owned());
            }
            deprecated = false;
        } else if line.starts_with("    fn ") || line.starts_with("    async fn ") {
            deprecated = false;
        }
    }
    names
}

/// True for `impl<…> TYPE<…>` and `impl TYPE`, and false for `impl Trait for TYPE`.
fn starts_inherent_impl(line: &str, type_name: &str) -> bool {
    let Some(mut rest) = line.strip_prefix("impl") else {
        return false;
    };
    if rest.starts_with('<') {
        let mut depth = 0usize;
        let mut end = None;
        for (index, c) in rest.char_indices() {
            match c {
                '<' => depth += 1,
                '>' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(index + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        match end {
            Some(end) => rest = &rest[end..],
            None => return false,
        }
    }
    let Some(rest) = rest.strip_prefix(' ') else {
        return false;
    };
    match rest.strip_prefix(type_name) {
        Some(after) => !after.starts_with(|c: char| c.is_alphanumeric() || c == '_'),
        None => false,
    }
}

fn engine_methods() -> BTreeSet<String> {
    ENGINE_FILES
        .iter()
        .flat_map(|(_, source)| inherent_methods(source, "CacheEngine", true))
        .collect()
}

fn wrapper_methods(wrapper: &str) -> BTreeSet<String> {
    let source = match wrapper {
        SYNC => SYNC_SOURCE,
        READ_POOL => READ_POOL_SOURCE,
        ASYNC => ASYNC_SOURCE,
        other => panic!("unknown wrapper {other}"),
    };
    inherent_methods(source, wrapper, false)
}

#[test]
fn every_wrapper_delegates_the_engine_surface_or_records_why_not() {
    let engine = engine_methods();
    let mut violations = Vec::new();

    for (wrapper, method, reason) in MANIFEST {
        if !REASONS.contains(reason) {
            violations.push(format!(
                "manifest entry ({wrapper}, {method}) has a reason that is not permitted: {reason:?}"
            ));
        }
    }

    for wrapper in [SYNC, READ_POOL, ASYNC] {
        let delegated = wrapper_methods(wrapper);
        let recorded: BTreeSet<&str> = MANIFEST
            .iter()
            .filter(|(w, _, _)| *w == wrapper)
            .map(|(_, method, _)| *method)
            .collect();

        for method in WRAPPER_REQUIRED {
            if !delegated.contains(*method) {
                violations.push(format!(
                    "{wrapper} lacks {method}, which every wrapper must have (WRAPPER_REQUIRED)"
                ));
            }
        }

        for method in &engine {
            if !delegated.contains(method) && !recorded.contains(method.as_str()) {
                violations.push(format!(
                    "{wrapper} neither delegates CacheEngine::{method} nor records why not in MANIFEST"
                ));
            }
        }
        for method in recorded {
            if !engine.contains(method) {
                violations.push(format!(
                    "stale MANIFEST entry ({wrapper}, {method}): CacheEngine has no such method"
                ));
            } else if delegated.contains(method) {
                violations.push(format!(
                    "stale MANIFEST entry ({wrapper}, {method}): {wrapper} now delegates it"
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "the wrapper surfaces have drifted ({} problem(s)):\n  {}",
        violations.len(),
        violations.join("\n  ")
    );
}

/// The scan must actually see the sources. A path typo or a formatting change that made it find
/// nothing would otherwise make the test above pass vacuously.
#[test]
fn the_scan_sees_the_surfaces_it_is_meant_to_compare() {
    let engine = engine_methods();
    for expected in [
        "get",
        "set",
        "import_from",
        "namespace_list",
        "preload",
        "explain",
        "entry_count_by_version",
        "cleanup_expired",
        "watcher",
        "rotate_encryption_key",
    ] {
        assert!(engine.contains(expected), "engine scan missed {expected}");
    }
    // Deprecated engine methods are skipped, never listed.
    for skipped in [
        "namespace_copy",
        "create_path_index",
        "drop_path_index",
        "list_path_indexes",
    ] {
        assert!(
            !engine.contains(skipped),
            "the scan should skip the deprecated {skipped}"
        );
    }

    for wrapper in [SYNC, READ_POOL, ASYNC] {
        let methods = wrapper_methods(wrapper);
        assert!(
            methods.len() > 15,
            "{wrapper} scan found only {} methods",
            methods.len()
        );
        assert!(methods.contains("get"), "{wrapper} scan missed `get`");
        assert!(methods.contains("open"), "{wrapper} scan missed `open`");
    }
    // Only inherent impls count: the trait impl for `CacheOptions` adds nothing to a wrapper.
    assert!(!wrapper_methods(SYNC).contains("with_ttl_secs"));
    // `with` and `size` are wrapper-only methods, and must not be mistaken for engine methods.
    assert!(wrapper_methods(SYNC).contains("with"));
    assert!(!engine.contains("with"));
}
