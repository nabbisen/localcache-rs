use clap::CommandFactory;

use super::*;

#[test]
fn every_command_has_explicit_database_authority() {
    let cases: &[(&[&str], DatabaseAuthority)] = &[
        (&["localcache", "list"], DatabaseAuthority::ReadOnly),
        (&["localcache", "stats"], DatabaseAuthority::ReadOnly),
        (
            &["localcache", "check", "file"],
            DatabaseAuthority::ReadOnly,
        ),
        (&["localcache", "scan", "."], DatabaseAuthority::ReadOnly),
        (&["localcache", "export"], DatabaseAuthority::ReadOnly),
        (&["localcache", "query"], DatabaseAuthority::ReadOnly),
        (
            &["localcache", "inspect", "file"],
            DatabaseAuthority::ReadOnly,
        ),
        (&["localcache", "namespaces"], DatabaseAuthority::ReadOnly),
        (&["localcache", "cleanup"], DatabaseAuthority::Writable),
        (&["localcache", "vacuum"], DatabaseAuthority::Writable),
        (
            &["localcache", "purge-version", "1"],
            DatabaseAuthority::Writable,
        ),
        (&["localcache", "import"], DatabaseAuthority::Writable),
        (
            &["localcache", "copy", "--from", "source"],
            DatabaseAuthority::Writable,
        ),
        (
            &[
                "localcache",
                "copy",
                "--from",
                "source",
                "--from-db",
                "other.sqlite3",
                "--upgrade-source",
            ],
            DatabaseAuthority::Writable,
        ),
        (
            &["localcache", "migrate", "--src-db", "source.sqlite3"],
            DatabaseAuthority::Writable,
        ),
        (&["localcache", "watch"], DatabaseAuthority::Writable),
    ];

    for (arguments, expected) in cases {
        let cli = Cli::try_parse_from(*arguments).unwrap();
        assert_eq!(command_database_authority(&cli.command), *expected);
    }
}

#[test]
fn migrate_help_discloses_source_schema_upgrade() {
    let mut command = Cli::command();
    let migrate = command
        .find_subcommand_mut("migrate")
        .expect("migrate subcommand");
    let help = migrate.render_long_help().to_string();
    assert!(help.contains("source is opened writable"), "{help}");
    assert!(help.contains("may be upgraded"), "{help}");
}

// RFC 024 R11 — `NO_COLOR` (<https://no-color.org>) disables colour only when
// it is set and non-empty.

#[test]
fn no_color_unset_does_not_disable_colour() {
    assert!(!no_color_requested(None));
}

#[test]
fn no_color_empty_does_not_disable_colour() {
    assert!(!no_color_requested(Some(std::ffi::OsString::new())));
}

#[test]
fn no_color_non_empty_disables_colour() {
    for value in ["1", "true", "0", "anything"] {
        assert!(
            no_color_requested(Some(value.into())),
            "NO_COLOR={value} must disable colour"
        );
    }
}
