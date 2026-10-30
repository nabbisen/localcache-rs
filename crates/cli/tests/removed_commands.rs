//! RFC 024 B4 — `migrate` was removed in v0.22.0; the replacement is
//! `copy --from-db` (its own tests are in `copy_from_db.rs`).

use std::process::Command;

use tempfile::TempDir;

/// `localcache migrate …` is an unknown subcommand: clap's own error, and its
/// usage-error exit status 2 (not the 1 every library error exits with).
#[test]
fn migrate_is_rejected_as_an_unknown_subcommand_with_exit_status_2() {
    let directory = TempDir::new().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_localcache"))
        .arg("--database")
        .arg(directory.path().join("db.sqlite3"))
        .args(["migrate", "--src-db", "source.sqlite3"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unrecognized subcommand 'migrate'"),
        "stderr: {stderr}"
    );
    assert!(
        !directory.path().join("db.sqlite3").exists(),
        "a rejected command must not create the database"
    );
}
