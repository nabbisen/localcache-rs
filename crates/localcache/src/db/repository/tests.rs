//! RFC 006 Erratum 1: `path_in_dir` is an exact, binary-collated range on
//! `path`, and the index serves it.

use rusqlite::Connection;

use super::{build_path_sql, explain_query, keys, path_filter_clauses, range_upper_bound};
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
            "EXPLAIN QUERY PLAN SELECT id, path, mtime, file_size, hash, last_accessed_at \
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
