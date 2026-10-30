//! RFC 025 R3 to R5: the conditions that used to be `UnsupportedFeature` have
//! purpose-named variants. One test per condition reachable through the public
//! API; the others are in the module tests next to the code.

mod common;

use localcache::{CacheEngine, ExportRecord, LocalFileCacheError};
use tempfile::TempDir;

use common::write_file;

#[test]
fn import_of_a_record_with_bad_base64_is_an_invalid_records_argument() {
    let directory = TempDir::new().unwrap();
    let source = write_file(&directory, "a.bin", b"x");
    let engine: CacheEngine<Vec<u8>> = CacheEngine::builder().database(":memory:").build().unwrap();
    engine.set(&source, &vec![1_u8]).unwrap();
    let mut records: Vec<ExportRecord> = engine.export_entries().unwrap();
    records[0].payload_b64 = "***not base64***".to_owned();

    let target: CacheEngine<Vec<u8>> = CacheEngine::builder().database(":memory:").build().unwrap();
    let error = target.import_entries(&records).unwrap_err();
    match &error {
        LocalFileCacheError::InvalidArgument { argument, reason } => {
            assert_eq!(*argument, "records");
            assert!(reason.starts_with("base64 decode error for '"), "{reason}");
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
    assert!(
        error
            .to_string()
            .starts_with("invalid argument: records: base64 decode error")
    );
}

#[cfg(feature = "encryption")]
mod encryption {
    use super::*;
    use localcache::CacheOptions;

    fn key(seed: u8) -> Vec<u8> {
        vec![seed; 32]
    }

    #[test]
    fn a_key_of_the_wrong_length_at_open_is_an_invalid_encryption_key_option() {
        let directory = TempDir::new().unwrap();
        let options = CacheOptions {
            database_path: directory.path().join("k.sqlite3"),
            encryption_key: Some(vec![7; 31]),
            ..CacheOptions::default()
        };
        let error = match CacheEngine::<Vec<u8>>::open(options) {
            Ok(_) => panic!("a 31-byte key was accepted"),
            Err(error) => error,
        };
        assert!(
            matches!(
                error,
                LocalFileCacheError::InvalidConfiguration {
                    option: "encryption_key",
                    ..
                }
            ),
            "{error:?}"
        );
        assert_eq!(
            error.to_string(),
            "invalid configuration: encryption_key: must be exactly 32 bytes, got 31"
        );
    }

    #[test]
    fn rotating_to_a_key_of_the_wrong_length_is_an_invalid_new_key_argument() {
        let directory = TempDir::new().unwrap();
        let engine: CacheEngine<Vec<u8>> = CacheEngine::builder()
            .database(directory.path().join("r.sqlite3"))
            .encryption_key(key(1))
            .build()
            .unwrap();
        let error = engine.rotate_encryption_key(&[9; 5]).unwrap_err();
        assert!(
            matches!(
                error,
                LocalFileCacheError::InvalidArgument {
                    argument: "new_key",
                    ..
                }
            ),
            "{error:?}"
        );
        assert_eq!(
            error.to_string(),
            "invalid argument: new_key: must be exactly 32 bytes, got 5"
        );
    }

    #[test]
    fn rotating_on_an_engine_without_a_key_is_a_missing_encryption_key() {
        let directory = TempDir::new().unwrap();
        let engine: CacheEngine<Vec<u8>> = CacheEngine::builder()
            .database(directory.path().join("n.sqlite3"))
            .build()
            .unwrap();
        let error = engine.rotate_encryption_key(&key(2)).unwrap_err();
        assert!(
            matches!(error, LocalFileCacheError::MissingEncryptionKey),
            "{error:?}"
        );
        assert_eq!(error.to_string(), "no encryption key is configured");
    }

    #[test]
    fn reading_an_encrypted_entry_without_a_key_is_a_missing_encryption_key() {
        let directory = TempDir::new().unwrap();
        let database = directory.path().join("e.sqlite3");
        let source = write_file(&directory, "s.bin", b"x");
        {
            let writer: CacheEngine<Vec<u8>> = CacheEngine::builder()
                .database(&database)
                .encryption_key(key(3))
                .build()
                .unwrap();
            writer.set(&source, &vec![1_u8, 2, 3]).unwrap();
        }
        let reader: CacheEngine<Vec<u8>> =
            CacheEngine::builder().database(&database).build().unwrap();
        let error = reader.get(&source).unwrap_err();
        assert!(
            matches!(error, LocalFileCacheError::MissingEncryptionKey),
            "{error:?}"
        );
    }
}
