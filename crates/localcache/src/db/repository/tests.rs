//! RFC 006 Erratum 1: `path_in_dir` is an exact, binary-collated range on
//! `path`, and the index serves it.

use rusqlite::Connection;

use super::{
    EVICTION_CANDIDATES_SQL, RECORD_USE_SQL, build_path_sql, explain_query, keys,
    path_filter_clauses, range_upper_bound,
};
use crate::db::schema;

const SEP: char = std::path::MAIN_SEPARATOR;

fn conn_with(paths: &[&str]) -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    schema::initialize(&mut conn, true).unwrap();
    for path in paths {
        conn.execute(
            "INSERT INTO files (namespace, path, mtime, file_size, updated_at) \
             VALUES ('ns', ?1, 0, 0, 0)",
            [path],
        )
        .unwrap();
    }
    conn
}

fn in_dir(conn: &Connection, prefix: &str, recursive: bool) -> Vec<String> {
    keys(conn, "ns", None, Some((prefix, recursive)), None)
        .unwrap()
        .into_iter()
        .map(|p| p.to_str().unwrap().to_owned())
        .collect()
}

/// The contract in plain strings: no `LIKE`, no collation, no SQL.
fn reference(paths: &[String], prefix: &str, recursive: bool) -> Vec<String> {
    let mut hits: Vec<String> = paths
        .iter()
        .filter(|path| match path.strip_prefix(prefix) {
            Some(rest) => recursive || !rest.contains(SEP),
            None => false,
        })
        .cloned()
        .collect();
    hits.sort();
    hits
}

#[test]
fn upper_bound_replaces_the_separator_with_its_successor() {
    assert_eq!(range_upper_bound("/data/Docs/"), "/data/Docs0");
    assert_eq!(range_upper_bound("C:\\data\\Docs\\"), "C:\\data\\Docs]");
    assert_eq!(range_upper_bound("/é/"), "/é0");
}

#[test]
fn case_variant_directories_are_not_returned() {
    let conn = conn_with(&["/data/DOCS/d.txt", "/data/Docs/a.txt", "/data/docs/b.txt"]);
    let prefix = format!("{SEP}data{SEP}Docs{SEP}");
    for recursive in [false, true] {
        assert_eq!(
            in_dir(&conn, &prefix, recursive),
            vec!["/data/Docs/a.txt".to_owned()],
            "recursive = {recursive}"
        );
    }
}

#[test]
fn sibling_with_the_directory_name_as_a_prefix_is_excluded() {
    let conn = conn_with(&["/d/dir/a", "/d/dir2/b", "/d/dir/sub/c", "/d/dir_x/e"]);
    assert_eq!(in_dir(&conn, "/d/dir/", false), vec!["/d/dir/a"]);
    assert_eq!(
        in_dir(&conn, "/d/dir/", true),
        vec!["/d/dir/a", "/d/dir/sub/c"]
    );
}

#[test]
fn like_metacharacters_and_non_ascii_names_match_literally() {
    let conn = conn_with(&[
        "/r/100%_done/x",
        "/r/100%_done/sub/y",
        "/r/100xxdone/z",
        "/r/a\\b/p",
        "/r/a]b/q",
        "/r/é/m",
        "/r/É/n",
    ]);
    assert_eq!(
        in_dir(&conn, "/r/100%_done/", true),
        vec!["/r/100%_done/sub/y", "/r/100%_done/x"]
    );
    assert_eq!(in_dir(&conn, "/r/a\\b/", false), vec!["/r/a\\b/p"]);
    assert_eq!(in_dir(&conn, "/r/a]b/", false), vec!["/r/a]b/q"]);
    assert_eq!(in_dir(&conn, "/r/é/", false), vec!["/r/é/m"]);
    assert_eq!(in_dir(&conn, "/r/É/", false), vec!["/r/É/n"]);
}

/// A tiny deterministic generator: the test must not depend on a crate or on
/// the clock, and a failure must reproduce.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, bound: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % bound
    }
}

#[test]
fn randomized_equivalence_with_a_plain_string_reference() {
    const NAMES: &[&str] = &[
        "dir",
        "dir2",
        "Dir",
        "DIR",
        "docs",
        "Docs",
        "100%_done",
        "a_b",
        "a%b",
        "a\\b",
        "a]b",
        "é",
        "É",
        "日本",
        "x",
        "sub",
    ];
    let mut rng = Lcg(0x5eed_0006);
    let mut paths: Vec<String> = Vec::new();
    while paths.len() < 400 {
        let depth = 1 + rng.next(4);
        let mut path = String::new();
        for _ in 0..depth {
            path.push(SEP);
            path.push_str(NAMES[rng.next(NAMES.len())]);
        }
        path.push(SEP);
        path.push_str(["f.txt", "g.txt", "dir"][rng.next(3)]);
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    let conn = conn_with(&refs);

    let mut cases = 0;
    for _ in 0..300 {
        let depth = 1 + rng.next(3);
        let mut prefix = String::new();
        for _ in 0..depth {
            prefix.push(SEP);
            prefix.push_str(NAMES[rng.next(NAMES.len())]);
        }
        prefix.push(SEP);
        for recursive in [false, true] {
            let mut got = in_dir(&conn, &prefix, recursive);
            got.sort();
            assert_eq!(
                got,
                reference(&paths, &prefix, recursive),
                "prefix {prefix:?}, recursive {recursive}"
            );
            cases += 1;
        }
    }
    assert_eq!(cases, 600);
}

fn assert_range_on_path(plan: &str, what: &str) {
    assert!(
        plan.contains("path>?") && plan.contains("path<?"),
        "{what}: expected a range on path in the plan, got:\n{plan}"
    );
    assert!(
        !plan.contains("SCAN"),
        "{what}: expected an index SEARCH, got:\n{plan}"
    );
    // RFC 026: the duplicate index is gone, and the autoindex of
    // `UNIQUE(namespace, path)` serves the range (the Q7 plan).
    assert!(
        plan.contains("sqlite_autoindex_files_1") && !plan.contains("idx_files_namespace_path"),
        "{what}: expected the UNIQUE autoindex, got:\n{plan}"
    );
}

fn plan_of(conn: &Connection, sql: &str, parameters: &[&dyn rusqlite::ToSql]) -> String {
    let mut statement = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
    statement
        .query_map(parameters, |row| row.get::<_, String>(3))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join("\n")
}

#[test]
fn the_path_listing_is_a_covering_range_on_the_unique_autoindex() {
    let conn = conn_with(&["/d/a", "/d/b/c"]);
    let prefix = format!("{SEP}d{SEP}");
    let plan = explain_query(&conn, "ns", None, Some((&prefix, true)), None).unwrap();
    assert!(
        plan.contains(
            "COVERING INDEX sqlite_autoindex_files_1 (namespace=? AND path>? AND path<?)"
        ),
        "{plan}"
    );
}

#[test]
fn the_eviction_scan_uses_the_use_index_and_no_sort() {
    let conn = conn_with(&["/d/a", "/d/b"]);
    let plan = plan_of(&conn, EVICTION_CANDIDATES_SQL, &[&"ns", &10_i64]);
    assert!(plan.contains("idx_files_use (namespace=?)"), "{plan}");
    assert!(
        !plan.contains("TEMP B-TREE") && !plan.contains("SCAN"),
        "{plan}"
    );
}

#[test]
fn recording_a_use_seeks_the_namespace_maximum_in_the_use_index() {
    let conn = conn_with(&["/d/a"]);
    let plan = plan_of(&conn, RECORD_USE_SQL, &[&1_i64, &"ns", &1_i64]);
    assert!(
        plan.contains("COVERING INDEX idx_files_use (namespace=?)"),
        "{plan}"
    );
}

#[test]
fn plans_use_a_range_on_path_for_both_forms() {
    let conn = conn_with(&["/d/a", "/d/b/c"]);
    let prefix = format!("{SEP}d{SEP}");
    for recursive in [false, true] {
        // The covering path listing (`dry_run`).
        let plan = explain_query(&conn, "ns", None, Some((&prefix, recursive)), None).unwrap();
        assert_range_on_path(&plan, &format!("listing, recursive={recursive}"));

        // The wider RFC 021 candidate query shares the clause builder; its
        // SELECT list differs, its WHERE and ORDER BY do not.
        let (clauses, params) = path_filter_clauses("ns", None, Some((&prefix, recursive)), None);
        let sql = format!(
            "EXPLAIN QUERY PLAN SELECT id, path, mtime, file_size, hash, last_used_us \
             FROM main.files WHERE {} ORDER BY path",
            clauses.join(" AND ")
        );
        let mut stmt = conn.prepare(&sql).unwrap();
        let plan = stmt
            .query_map(rusqlite::params_from_iter(params.iter()), |r| {
                r.get::<_, String>(3)
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .join("\n");
        assert_range_on_path(&plan, &format!("candidates, recursive={recursive}"));
    }
}

#[test]
fn sql_text_does_not_vary_with_the_directory() {
    let (a, _) = build_path_sql("ns", None, Some(("/a/", false)), None);
    let (b, _) = build_path_sql("ns", None, Some(("/b/é%/", false)), None);
    assert_eq!(a, b);
}

// ---------------------------------------------------------------------------
// RFC 026: the use order
// ---------------------------------------------------------------------------

use super::{record_use, test_clock, upsert_in_tx, use_error, use_value_of_record};
use crate::detection::metadata::FileMetadata;
use crate::error::LocalFileCacheError;

fn use_value(conn: &Connection, path: &str) -> i64 {
    conn.query_row(
        "SELECT last_used_us FROM files WHERE path = ?1",
        [path],
        |row| row.get(0),
    )
    .unwrap()
}

fn id_of(conn: &Connection, path: &str) -> i64 {
    conn.query_row("SELECT id FROM files WHERE path = ?1", [path], |row| {
        row.get(0)
    })
    .unwrap()
}

fn metadata() -> FileMetadata {
    FileMetadata {
        mtime: 1,
        file_size: 2,
        hash: None,
    }
}

#[test]
fn a_use_is_strictly_newer_than_everything_in_its_namespace_under_any_clock() {
    let conn = conn_with(&["/d/a", "/d/b", "/d/c"]);
    // A clock that never moves, and one that runs backwards.
    for clock in [0_i64, 1] {
        let mut tick = 1_000_000_000_000_i64;
        let _guard = test_clock::set(move || {
            if clock == 0 {
                tick
            } else {
                tick -= 7;
                tick
            }
        });
        let mut last = conn
            .query_row("SELECT max(last_used_us) FROM files", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap();
        for path in ["/d/a", "/d/b", "/d/c", "/d/a", "/d/a", "/d/b"] {
            record_use(&conn, "ns", id_of(&conn, path)).unwrap();
            let value = use_value(&conn, path);
            assert!(value > last, "{path}: {value} should exceed {last}");
            last = value;
        }
    }
}

#[test]
fn a_use_in_one_namespace_never_looks_at_another() {
    let conn = conn_with(&["/d/a"]);
    conn.execute(
        "INSERT INTO files (namespace, path, mtime, file_size, updated_at, last_used_us) \
         VALUES ('other', '/d/z', 0, 0, 0, 4000000000000000000)",
        [],
    )
    .unwrap();
    let _guard = test_clock::set(|| 5_000_000);
    record_use(&conn, "ns", id_of(&conn, "/d/a")).unwrap();
    assert_eq!(use_value(&conn, "/d/a"), 5_000_000);
}

#[test]
fn a_write_is_a_use_and_so_is_an_overwrite() {
    let mut conn = conn_with(&[]);
    let mut now = 10_000_000_i64;
    let _guard = test_clock::set(move || {
        now += 1;
        now
    });
    let write = |conn: &mut Connection, path: &str| {
        let tx = conn.transaction().unwrap();
        upsert_in_tx(&tx, "ns", path, &metadata(), b"x", "raw", 0).unwrap();
        tx.commit().unwrap();
    };
    write(&mut conn, "/d/a");
    write(&mut conn, "/d/b");
    assert!(use_value(&conn, "/d/b") > use_value(&conn, "/d/a"));
    // Overwriting `a` makes it the newest, and the row (and its id) is the same.
    let id = id_of(&conn, "/d/a");
    write(&mut conn, "/d/a");
    assert_eq!(id_of(&conn, "/d/a"), id);
    assert!(use_value(&conn, "/d/a") > use_value(&conn, "/d/b"));
}

#[test]
fn a_use_at_the_end_of_the_order_fails_with_a_typed_error_and_changes_nothing() {
    let mut conn = conn_with(&["/d/a", "/d/b"]);
    let top = (1_i64 << 62) - 1;
    conn.execute(
        "UPDATE files SET last_used_us = ?1 WHERE path = '/d/a'",
        [top],
    )
    .unwrap();
    conn.execute("UPDATE files SET last_used_us = 5 WHERE path = '/d/b'", [])
        .unwrap();

    // The next value would be 2^62.
    let error = record_use(&conn, "ns", id_of(&conn, "/d/b")).unwrap_err();
    assert!(
        matches!(&error, LocalFileCacheError::DatabaseState(message) if message.contains("use order exhausted")),
        "{error:?}"
    );
    assert_eq!(use_value(&conn, "/d/a"), top);
    assert_eq!(use_value(&conn, "/d/b"), 5);

    // A write fails the same way, whether it inserts or overwrites, and the
    // transaction around it can be dropped (rolled back) without a trace.
    for path in ["/d/new", "/d/b"] {
        let tx = conn.transaction().unwrap();
        let error = upsert_in_tx(&tx, "ns", path, &metadata(), b"x", "raw", 0).unwrap_err();
        assert!(
            matches!(&error, LocalFileCacheError::DatabaseState(m) if m.contains("use order exhausted")),
            "{path}: {error:?}"
        );
        drop(tx);
    }
    let rows: i64 = conn
        .query_row("SELECT count(*) FROM files", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 2);
    assert_eq!(use_value(&conn, "/d/b"), 5);
}

#[test]
fn only_the_use_value_check_is_reported_as_an_exhausted_order() {
    let failure = |extended_code: i32, message: &str| {
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(extended_code),
            Some(message.to_owned()),
        )
    };
    // The use value's CHECK.
    assert!(matches!(
        use_error(failure(
            rusqlite::ffi::SQLITE_CONSTRAINT_CHECK,
            "CHECK constraint failed: typeof(last_used_us) = 'integer' AND last_used_us < 4611686018427387904"
        )),
        LocalFileCacheError::DatabaseState(_)
    ));
    // Any other CHECK, any other constraint, any other failure.
    for error in [
        failure(
            rusqlite::ffi::SQLITE_CONSTRAINT_CHECK,
            "CHECK constraint failed: something_else",
        ),
        failure(
            rusqlite::ffi::SQLITE_CONSTRAINT_NOTNULL,
            "NOT NULL constraint failed: files.last_used_us",
        ),
        failure(rusqlite::ffi::SQLITE_BUSY, "database is locked"),
    ] {
        assert!(matches!(use_error(error), LocalFileCacheError::Database(_)));
    }
}

#[test]
fn an_imported_recency_is_seconds_times_a_million_and_refuses_what_does_not_fit() {
    assert_eq!(use_value_of_record(7, 3), Some(7_000_000));
    // Never read (0): placed by `updated_at`.
    assert_eq!(use_value_of_record(0, 3), Some(3_000_000));
    assert_eq!(use_value_of_record(0, 0), Some(0));
    // The largest second that fits below 2^62.
    let largest = (1_i64 << 62) / 1_000_000;
    assert_eq!(use_value_of_record(largest, 0), Some(largest * 1_000_000));
    assert_eq!(use_value_of_record(largest + 1, 0), None);
    assert_eq!(use_value_of_record(i64::MAX, 0), None);
    // Negative, in either field the rule reads.
    assert_eq!(use_value_of_record(0, -1), None);
    assert_eq!(use_value_of_record(-5, -5), None);
    // A negative `last_accessed_at` is "never read": `updated_at` decides.
    assert_eq!(use_value_of_record(-5, 4), Some(4_000_000));
}

/// RFC 029 R3 (step 0, C1): the rotation's page query pages by
/// `(namespace, path)`, a range on the UNIQUE autoindex with no sort. Paging
/// by `file_id` plans a scan of the whole namespace and a temp b-tree for every
/// page, which took 478 s instead of 1.6 s at 1,000,000 entries.
#[cfg(feature = "encryption")]
#[test]
fn the_rotation_page_is_a_range_on_the_unique_autoindex_with_no_sort() {
    let conn = conn_with(&["/d/a", "/d/b", "/e/c"]);
    let mut stmt = conn
        .prepare(&format!(
            "EXPLAIN QUERY PLAN {}",
            crate::db::repository::PAYLOAD_PAGE_SQL
        ))
        .unwrap();
    let plan = stmt
        .query_map(rusqlite::params!["ns", "", 10_i64], |r| {
            r.get::<_, String>(3)
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join("\n");
    assert!(
        plan.contains("sqlite_autoindex_files_1 (namespace=? AND path>?)"),
        "{plan}"
    );
    assert!(!plan.contains("TEMP B-TREE"), "{plan}");
    assert!(!plan.contains("SCAN"), "{plan}");
}
