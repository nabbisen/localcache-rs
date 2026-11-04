//! RFC 030 slice 2: the error taxonomy as expected text.
//!
//! `GOLDEN` is the table of the public `LocalFileCacheError`: variant, the
//! feature gate it exists under, and its `Display`. It was generated on the
//! v0.24.0 tree before the layer errors existed, and it is the contract the
//! split must keep: the variants, their gates and their texts do not change.
//!
//! * `every_variant_is_in_the_table` — `variant_name` is an exhaustive `match`
//!   with no wildcard, so a new or removed variant is a compile error here, and
//!   the table must list exactly the variants of the build.
//! * `every_layer_variant_lifts_to_its_public_variant` — builds each variant
//!   of `FormatError` and `StorageError`, lifts it, and checks the public
//!   variant, its payload and that the `Display` text is unchanged.

use std::path::PathBuf;

use crate::LocalFileCacheError as E;
use crate::db::{DatabaseError, StorageError};
use crate::error::Lift;
use crate::serialization::FormatError;

/// variant, feature gate (`-` for none; `a|b` is any of), `Display`.
const GOLDEN: &str = "\
AsyncTaskPanicked\tasync|async-std|smol\tasync blocking task panicked
Database\t-\tdatabase error: Query returned no rows
DatabaseState\t-\tdatabase state: state
EncryptionError\tencryption\tencryption error: detail
FileNotFound\t-\tfile does not exist: /p
IncompatibleSchema\t-\tincompatible database schema: found
InvalidArgument\t-\tinvalid argument: arg: why
InvalidConfiguration\t-\tinvalid configuration: opt: why
InvalidPath\t-\tinvalid path: /p
Io\t-\tio error: io detail
MissingEncryptionKey\tencryption\tno encryption key is configured
PayloadTooLarge\tcompression\tpayload exceeds the 7-byte limit for compressed payloads (CacheOptions::max_decompressed_bytes)
Poisoned\t-\tlock poisoned: res
ReadOnly\t-\toperation not permitted: cache is open in read-only mode
SchemaNotCurrent\t-\tdatabase schema is not current; a read-only open cannot migrate it; database was not modified
Serialization\t-\tserialization error: detail
UnknownEncoding\t-\tunknown payload encoding: tag
UnsealedEntry\tencryption\tunsealed entry: it is not sealed under this engine's key and bound to its path (it is stored plain, or was sealed before v0.24.0); call rotate_encryption_key once with the same key to seal it, or purge the entry
Watcher\twatching\tfile watching failed: detail
";

fn gate_is_on(gate: &str) -> bool {
    gate.split('|').any(|feature| match feature {
        "-" => true,
        "encryption" => cfg!(feature = "encryption"),
        "compression" => cfg!(feature = "compression"),
        "watching" => cfg!(feature = "watching"),
        "async" => cfg!(feature = "async"),
        "async-std" => cfg!(feature = "async-std"),
        "smol" => cfg!(feature = "smol"),
        other => panic!("the golden table names an unknown gate `{other}`"),
    })
}

/// Exhaustive on purpose: no wildcard arm.
fn variant_name(error: &E) -> &'static str {
    match error {
        E::Database(_) => "Database",
        E::Io(_) => "Io",
        E::Serialization(_) => "Serialization",
        E::FileNotFound { .. } => "FileNotFound",
        E::InvalidPath { .. } => "InvalidPath",
        E::ReadOnly => "ReadOnly",
        E::UnknownEncoding(_) => "UnknownEncoding",
        #[cfg(feature = "compression")]
        E::PayloadTooLarge { .. } => "PayloadTooLarge",
        E::Poisoned { .. } => "Poisoned",
        #[cfg(feature = "encryption")]
        E::EncryptionError(_) => "EncryptionError",
        #[cfg(any(feature = "async", feature = "async-std", feature = "smol"))]
        E::AsyncTaskPanicked => "AsyncTaskPanicked",
        E::InvalidConfiguration { .. } => "InvalidConfiguration",
        E::InvalidArgument { .. } => "InvalidArgument",
        E::SchemaNotCurrent => "SchemaNotCurrent",
        E::IncompatibleSchema(_) => "IncompatibleSchema",
        #[cfg(feature = "encryption")]
        E::MissingEncryptionKey => "MissingEncryptionKey",
        #[cfg(feature = "encryption")]
        E::UnsealedEntry => "UnsealedEntry",
        E::DatabaseState(_) => "DatabaseState",
        #[cfg(feature = "watching")]
        E::Watcher(_) => "Watcher",
    }
}

/// One value of every variant of this build.
fn every_variant() -> Vec<E> {
    vec![
        E::Database(DatabaseError::from_sqlite(
            rusqlite::Error::QueryReturnedNoRows,
        )),
        E::Io(std::io::Error::other("io detail")),
        E::Serialization("detail".to_owned()),
        E::FileNotFound {
            path: PathBuf::from("/p"),
        },
        E::InvalidPath {
            path: PathBuf::from("/p"),
        },
        E::ReadOnly,
        E::UnknownEncoding("tag".to_owned()),
        E::Poisoned { resource: "res" },
        E::InvalidConfiguration {
            option: "opt",
            reason: "why".to_owned(),
        },
        E::InvalidArgument {
            argument: "arg",
            reason: "why".to_owned(),
        },
        E::SchemaNotCurrent,
        E::IncompatibleSchema("found".to_owned()),
        E::DatabaseState("state".to_owned()),
        #[cfg(feature = "compression")]
        E::PayloadTooLarge { limit: 7 },
        #[cfg(feature = "encryption")]
        E::EncryptionError("detail".to_owned()),
        #[cfg(feature = "encryption")]
        E::MissingEncryptionKey,
        #[cfg(feature = "encryption")]
        E::UnsealedEntry,
        #[cfg(any(feature = "async", feature = "async-std", feature = "smol"))]
        E::AsyncTaskPanicked,
        #[cfg(feature = "watching")]
        E::Watcher("detail".to_owned()),
    ]
}

#[test]
fn every_variant_is_in_the_table() {
    let mut expected: Vec<(&str, String)> = GOLDEN
        .lines()
        .map(|line| {
            let mut cells = line.splitn(3, '\t');
            let (name, gate, text) = (
                cells.next().unwrap(),
                cells.next().unwrap(),
                cells.next().unwrap(),
            );
            (name, format!("{gate}\t{text}"))
        })
        .filter(|(_, rest)| gate_is_on(rest.split('\t').next().unwrap()))
        .map(|(name, rest)| (name, rest.split_once('\t').unwrap().1.to_owned()))
        .collect();
    expected.sort();
    let mut actual: Vec<(&str, String)> = every_variant()
        .iter()
        .map(|error| (variant_name(error), error.to_string()))
        .collect();
    actual.sort();
    assert_eq!(
        actual, expected,
        "the public error taxonomy differs from the golden table"
    );
}

/// Each layer variant, lifted, is the public variant of the same name with the
/// same payload and the same text.
#[test]
fn every_layer_variant_lifts_to_its_public_variant() {
    // The layer's own text must also equal the public one: the two enums
    // repeat the `Display` strings, and this keeps the copies in step.
    fn storage(error: StorageError) -> E {
        let own_text = error.to_string();
        let lifted = Err::<(), _>(error).lift().unwrap_err();
        assert_eq!(own_text, lifted.to_string());
        lifted
    }
    fn format(error: FormatError) -> E {
        let own_text = error.to_string();
        let lifted = Err::<(), _>(error).lift().unwrap_err();
        assert_eq!(own_text, lifted.to_string());
        lifted
    }

    // StorageError: 5 variants.
    let lifted = storage(StorageError::Database(DatabaseError::from_sqlite(
        rusqlite::Error::QueryReturnedNoRows,
    )));
    assert!(matches!(&lifted, E::Database(d) if d.sqlite_extended_code().is_none()));
    assert_eq!(lifted.to_string(), "database error: Query returned no rows");

    let lifted = storage(StorageError::SchemaNotCurrent);
    assert!(matches!(lifted, E::SchemaNotCurrent));
    assert_eq!(
        lifted.to_string(),
        "database schema is not current; a read-only open cannot migrate it; database was not modified"
    );

    let lifted = storage(StorageError::IncompatibleSchema("found".to_owned()));
    assert!(matches!(&lifted, E::IncompatibleSchema(m) if m == "found"));
    assert_eq!(lifted.to_string(), "incompatible database schema: found");

    let lifted = storage(StorageError::DatabaseState("state".to_owned()));
    assert!(matches!(&lifted, E::DatabaseState(m) if m == "state"));
    assert_eq!(lifted.to_string(), "database state: state");

    let lifted = storage(StorageError::InvalidArgument {
        argument: "arg",
        reason: "why".to_owned(),
    });
    assert!(matches!(&lifted, E::InvalidArgument { argument: "arg", reason } if reason == "why"));
    assert_eq!(lifted.to_string(), "invalid argument: arg: why");

    // FormatError: 7 variants (3 always, 4 behind features).
    let lifted = format(FormatError::Io(std::io::Error::other("io detail")));
    assert!(matches!(lifted, E::Io(_)));
    assert_eq!(lifted.to_string(), "io error: io detail");

    let lifted = format(FormatError::Serialization("detail".to_owned()));
    assert!(matches!(&lifted, E::Serialization(m) if m == "detail"));
    assert_eq!(lifted.to_string(), "serialization error: detail");

    let lifted = format(FormatError::UnknownEncoding("tag".to_owned()));
    assert!(matches!(&lifted, E::UnknownEncoding(m) if m == "tag"));
    assert_eq!(lifted.to_string(), "unknown payload encoding: tag");

    #[cfg(feature = "compression")]
    {
        let lifted = format(FormatError::PayloadTooLarge { limit: 7 });
        assert!(matches!(lifted, E::PayloadTooLarge { limit: 7 }));
        assert_eq!(
            lifted.to_string(),
            "payload exceeds the 7-byte limit for compressed payloads (CacheOptions::max_decompressed_bytes)"
        );
    }
    #[cfg(feature = "encryption")]
    {
        let lifted = format(FormatError::EncryptionError("detail".to_owned()));
        assert!(matches!(&lifted, E::EncryptionError(m) if m == "detail"));
        assert_eq!(lifted.to_string(), "encryption error: detail");

        let lifted = format(FormatError::MissingEncryptionKey);
        assert!(matches!(lifted, E::MissingEncryptionKey));
        assert_eq!(lifted.to_string(), "no encryption key is configured");

        let lifted = format(FormatError::UnsealedEntry);
        assert!(matches!(lifted, E::UnsealedEntry));
        assert!(
            lifted
                .to_string()
                .starts_with("unsealed entry: it is not sealed under this engine's key")
        );
    }
}
