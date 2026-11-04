//! RFC 030 slice 1: the layering of the crate, checked.
//!
//! The crate is built in layers, lowest first. A module may name only the
//! layers listed for it in `ALLOWED`; naming a higher one fails this test with
//! the file, the line and the allowed layers. The table (the same one is in
//! `docs/src/architecture.md`, "Layers"):
//!
//! | layer | may use |
//! |---|---|
//! | `key` | nothing of the crate |
//! | `db` (schema, repository, indexes, the values it stores, its `StorageError`) | nothing of the crate |
//! | `serialization` (codec, compression, sealing, its `FormatError`) | `key` |
//! | `error` (the public `LocalFileCacheError` and the crate-private lift from the two layer errors) | `db`, `serialization` |
//! | `path` | `error` |
//! | `detection` (freshness) | `error`, `db` (it compares a stored `FileMetadata`) |
//! | `cache` (engine, query, options, builder, entry, glob) | everything above |
//! | adapters: `pool`, `read_pool`, `cache::async_engine`, `cache::watcher` | everything above |
//!
//! The cuts of RFC 030 turn the rows `db` and `serialization` into crates; this
//! test is what makes that cut a move and not a redesign. It reads the source
//! text, so it needs no feature and runs in every row.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const ADAPTER: &str = "adapters";
const CORE: &str = "cache";

/// layer -> layers it may name (a layer may always name itself).
const ALLOWED: &[(&str, &[&str])] = &[
    ("key", &[]),
    ("db", &[]),
    ("serialization", &["key"]),
    ("error", &["db", "serialization"]),
    ("path", &["error"]),
    ("detection", &["error", "db"]),
    (
        CORE,
        &["error", "key", "path", "db", "serialization", "detection"],
    ),
    (
        ADAPTER,
        &[
            "error",
            "key",
            "path",
            "db",
            "serialization",
            "detection",
            CORE,
        ],
    ),
];

/// Upward references that exist today and are named, with the reason, instead
/// of hidden. `(file under src/, the `crate::` path it names, why)`. Each is
/// a **public method of the engine or the builder that returns an adapter
/// type**; the cut of the adapters (RFC 030 R7, not decided) has to solve
/// them. A new entry here needs the architect's agreement; an entry that no
/// longer matches anything fails `every_exception_is_still_used`.
const EXCEPTIONS: &[(&str, &str, &str)] = &[
    (
        "cache/builder.rs",
        "ReadPool",
        "CacheEngineBuilder::build_read_pool returns a ReadPool",
    ),
    (
        "cache/engine.rs",
        "cache::watcher::CacheWatcher",
        "CacheEngine::watcher returns a CacheWatcher",
    ),
    (
        "cache/engine.rs",
        "cache::watcher::CacheDebouncedWatcher",
        "CacheEngine::debounced_watcher returns a CacheDebouncedWatcher",
    ),
];

fn is_excepted(file: &str, reference: &str) -> Option<usize> {
    EXCEPTIONS
        .iter()
        .position(|(f, r, _)| *f == file && reference.starts_with(r))
}

fn src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// The layer a source file belongs to, from its path under `src/`.
fn layer_of_path(relative: &Path) -> Option<&'static str> {
    let parts: Vec<&str> = relative
        .components()
        .map(|c| c.as_os_str().to_str().unwrap())
        .collect();
    let first = parts.first()?.trim_end_matches(".rs");
    Some(match first {
        "lib" => return None,
        "error" => "error",
        "key" => "key",
        "path" => "path",
        "db" => "db",
        "serialization" => "serialization",
        "detection" => "detection",
        "pool" | "read_pool" => ADAPTER,
        "cache" => match parts.get(1).map(|p| p.trim_end_matches(".rs")) {
            Some("async_engine") | Some("watcher") => ADAPTER,
            _ => CORE,
        },
        other => panic!("a new top-level module `{other}` has no layer in tests/layering.rs"),
    })
}

/// The layer a `crate::…` path names.
fn layer_of_reference(path: &str, root_names: &BTreeMap<String, String>) -> Option<&'static str> {
    let mut segments = path.split("::");
    let first = segments.next()?;
    match first {
        "error" => Some("error"),
        "key" => Some("key"),
        "path" => Some("path"),
        "db" => Some("db"),
        "serialization" => Some("serialization"),
        "detection" => Some("detection"),
        "pool" | "read_pool" => Some(ADAPTER),
        "cache" => match segments.next() {
            Some("async_engine") | Some("watcher") => Some(ADAPTER),
            _ => Some(CORE),
        },
        // A name re-exported at the crate root (`crate::CacheStatus`): the layer
        // of the module the root re-exports it from.
        name => root_names
            .get(name)
            .and_then(|module| layer_of_reference(module, root_names)),
    }
}

/// `pub use cache::entry::{A, B}` lines of `lib.rs` -> name -> module path.
fn root_reexports() -> BTreeMap<String, String> {
    let text = fs::read_to_string(src().join("lib.rs")).unwrap();
    let flat: String = text
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join(" ");
    let mut map = BTreeMap::new();
    for statement in flat.split(';') {
        let Some(rest) = statement.trim().strip_prefix("pub use ") else {
            continue;
        };
        let statement = rest.trim();
        if let Some((prefix, group)) = statement.split_once("::{") {
            for name in group.trim_end_matches('}').split(',') {
                let name = name.trim();
                if !name.is_empty() {
                    map.insert(name.to_owned(), prefix.trim().to_owned());
                }
            }
        } else if let Some((prefix, name)) = statement.rsplit_once("::") {
            map.insert(name.trim().to_owned(), prefix.trim().to_owned());
        }
    }
    map
}

/// Production sources only: test files are not part of a layer's contract.
fn is_test_file(path: &Path) -> bool {
    let name = path.file_name().unwrap().to_str().unwrap();
    name == "tests.rs"
        || name == "golden.rs"
        || name.starts_with("test_")
        || name.ends_with("_tests.rs")
        || path
            .components()
            .any(|c| c.as_os_str().to_str() == Some("tests"))
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") && !is_test_file(&path) {
            out.push(path);
        }
    }
}

/// The `crate::` paths named on one line (comments removed), including the
/// members of `use crate::{…}` groups.
fn references(line: &str) -> Vec<String> {
    let code = line.split("//").next().unwrap_or("");
    let mut found = Vec::new();
    let mut rest = code;
    while let Some(at) = rest.find("crate::") {
        let after = &rest[at + "crate::".len()..];
        if let Some(group) = after.strip_prefix('{') {
            let end = group.find('}').unwrap_or(group.len());
            for member in group[..end].split(',') {
                let member = member.trim();
                if !member.is_empty() {
                    found.push(member.split_whitespace().next().unwrap().to_owned());
                }
            }
        } else {
            let end = after
                .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':'))
                .unwrap_or(after.len());
            found.push(after[..end].to_owned());
        }
        rest = after;
    }
    found
}

fn violations_and_used_exceptions() -> (Vec<String>, Vec<bool>) {
    let mut used = vec![false; EXCEPTIONS.len()];
    let roots = root_reexports();
    let allowed: BTreeMap<&str, &[&str]> = ALLOWED.iter().copied().collect();
    let mut files = Vec::new();
    rust_files(&src(), &mut files);
    files.sort();
    let mut out = Vec::new();
    for file in files {
        let relative = file.strip_prefix(src()).unwrap();
        let Some(layer) = layer_of_path(relative) else {
            continue;
        };
        let may = allowed[layer];
        let text = fs::read_to_string(&file).unwrap();
        let mut in_block_comment = false;
        for (number, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if in_block_comment {
                in_block_comment = !trimmed.contains("*/");
                continue;
            }
            if trimmed.starts_with("/*") && !trimmed.contains("*/") {
                in_block_comment = true;
                continue;
            }
            if trimmed.starts_with("//") {
                continue;
            }
            for reference in references(line) {
                let Some(target) = layer_of_reference(&reference, &roots) else {
                    continue;
                };
                if target != layer && !may.contains(&target) {
                    if let Some(index) = is_excepted(&relative.display().to_string(), &reference) {
                        used[index] = true;
                        continue;
                    }
                    out.push(format!(
                        "src/{}:{}: layer `{layer}` names layer `{target}` (`crate::{reference}`); \
                         `{layer}` may use only: {}",
                        relative.display(),
                        number + 1,
                        if may.is_empty() {
                            "nothing of this crate".to_owned()
                        } else {
                            may.join(", ")
                        }
                    ));
                }
            }
        }
    }
    (out, used)
}

#[test]
fn no_layer_names_a_higher_one() {
    let (found, _) = violations_and_used_exceptions();
    assert!(
        found.is_empty(),
        "the layering of RFC 030 is broken ({} place(s)):\n{}\n\nThe table is at the top of tests/layering.rs.",
        found.len(),
        found.join("\n")
    );
}

#[test]
fn every_exception_is_still_used() {
    let (_, used) = violations_and_used_exceptions();
    for (index, (file, reference, reason)) in EXCEPTIONS.iter().enumerate() {
        assert!(
            used[index],
            "the exception for src/{file} naming `crate::{reference}` ({reason}) matches nothing any more: remove it"
        );
    }
}

/// The table must cover every top-level module, so a new module cannot slip in
/// unlayered (`layer_of_path` panics on an unknown one; this checks the table).
#[test]
fn every_layer_in_the_table_is_defined_and_only_lists_known_layers() {
    let names: Vec<&str> = ALLOWED.iter().map(|(n, _)| *n).collect();
    for (layer, may) in ALLOWED {
        for m in *may {
            assert!(names.contains(m), "`{layer}` allows an unknown layer `{m}`");
        }
    }
}
