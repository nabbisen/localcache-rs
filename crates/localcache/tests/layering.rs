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
//! RFC 030 slice 3 adds two rules about the database, checked by
//! `above_db_nothing_names_rusqlite_or_the_inside_of_db`: outside `db`, no
//! production file names `rusqlite`, and none reaches into `db::repository`,
//! `db::schema` or `db::indexes`. The one door is `db::Store`. (Test files may
//! use `rusqlite` to set up and inspect rows; they are not a layer.)
//!
//! **What this test is not.** It reads text, so it is weaker than the compiler
//! and does not pretend otherwise. `no_layer_names_a_higher_one` sees `crate::`
//! paths only; `lower_layers_do_not_escape_by_relative_paths` closes the
//! relative spellings (`super::super::…`, a `super::` that reaches the crate
//! root from a top-level file, `localcache::…`, `use crate::*`). It does **not**
//! see: a path built by a macro, a `#[path]` attribute, a name reached through
//! a glob of another module, or an item smuggled through a trait object or a
//! generic parameter. Those need the compiler, which is what the cuts of
//! RFC 030 would add.
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

/// Production files outside `db` hold no database-library type and use the
/// layer only through its `Store`.
#[test]
fn above_db_nothing_names_rusqlite_or_the_inside_of_db() {
    let mut files = Vec::new();
    rust_files(&src(), &mut files);
    files.sort();
    let mut found = Vec::new();
    for file in files {
        let relative = file.strip_prefix(src()).unwrap();
        let first = relative
            .components()
            .next()
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap();
        if first == "db" || first == "db.rs" {
            continue;
        }
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
            let code = line.split("//").next().unwrap_or("");
            for forbidden in ["rusqlite", "db::repository", "db::schema", "db::indexes"] {
                if code.contains(forbidden) {
                    found.push(format!(
                        "src/{}:{}: names `{forbidden}`; above `db` only `db::Store` and the values `db` re-exports are allowed",
                        relative.display(),
                        number + 1
                    ));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "the storage boundary of RFC 030 is broken ({} place(s)):\n{}",
        found.len(),
        found.join("\n")
    );
}

/// The relative spellings of an upward reference (review 091, § 3). A lower
/// layer (everything below `cache`) may not contain:
/// * `super::super` (the review's rule, applied as text);
/// * a chain of `super::` as long as the file's depth under `src/`, which
///   reaches the crate root, followed by a module or item of the crate root
///   (`detection.rs` naming `super::cache::…`); an inline `mod tests { use
///   super::*; }` is the file's own module and is not flagged;
/// * `localcache::` as a path (the crate naming itself from outside);
/// * `crate::*`, a glob that would import every layer without naming one.
#[test]
fn lower_layers_do_not_escape_by_relative_paths() {
    let mut files = Vec::new();
    rust_files(&src(), &mut files);
    files.sort();
    let roots = root_reexports();
    let root_modules = [
        "error",
        "key",
        "path",
        "db",
        "serialization",
        "detection",
        "cache",
        "pool",
        "read_pool",
    ];
    let mut found = Vec::new();
    for file in files {
        let relative = file.strip_prefix(src()).unwrap();
        let Some(layer) = layer_of_path(relative) else {
            continue;
        };
        if layer == CORE || layer == ADAPTER {
            continue;
        }
        // `a.rs` is module depth 1, `a/b.rs` depth 2: that many `super` hops
        // reach the crate root.
        let depth = relative.components().count();
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
            let code = line.split("//").next().unwrap_or("");
            let mut problems = Vec::new();
            if code.contains("super::super") {
                problems.push("`super::super`".to_owned());
            }
            let mut rest = code;
            while let Some(at) = rest.find("super::") {
                let mut hops = 0;
                let mut tail = &rest[at..];
                while let Some(next) = tail.strip_prefix("super::") {
                    hops += 1;
                    tail = next;
                }
                let next_name: String = tail
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                let names_root_item =
                    root_modules.contains(&next_name.as_str()) || roots.contains_key(&next_name);
                if hops >= depth && names_root_item {
                    problems.push(format!(
                        "{hops} `super::` hops from a module at depth {depth} (the crate root) to `{next_name}`"
                    ));
                }
                rest = &rest[at + "super::".len()..];
            }
            let mut rest = code;
            while let Some(at) = rest.find("localcache::") {
                let before = rest[..at].chars().next_back();
                if !before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    problems.push("a `localcache::` path".to_owned());
                }
                rest = &rest[at + "localcache::".len()..];
            }
            if code.contains("crate::*") {
                problems.push("the glob `crate::*`".to_owned());
            }
            for problem in problems {
                found.push(format!(
                    "src/{}:{}: layer `{layer}` uses {problem}, which can name a higher layer without a `crate::` path",
                    relative.display(),
                    number + 1
                ));
            }
        }
    }
    assert!(
        found.is_empty(),
        "a lower layer escapes the layering test ({} place(s)):\n{}\n\nName the layer with a `crate::` path so the table can check it.",
        found.len(),
        found.join("\n")
    );
}
