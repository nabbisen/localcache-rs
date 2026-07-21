use super::*;

#[test]
fn released_public_mixed_case_user_index_reopens_successfully() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("mixed-case-index.sqlite3");
    let engine = CacheEngine::<Vec<f32>>::builder()
        .database(&database)
        .build()
        .unwrap();
    for suffix in ["MixedCase_9", "dollar$sign", "éclair"] {
        assert_eq!(
            engine.create_path_index(suffix).unwrap(),
            format!("lc_user_{suffix}")
        );
    }
    drop(engine);

    let reopened = CacheEngine::<Vec<f32>>::builder()
        .database(&database)
        .build();
    assert!(
        reopened.is_ok(),
        "valid released mixed-case index was rejected"
    );
    drop(reopened);

    let conn = Connection::open_with_flags(
        &database,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .unwrap();
    let indexes = conn
        .prepare(
            "SELECT name FROM sqlite_schema
             WHERE type = 'index' AND name LIKE 'lc_user_%'
             ORDER BY name",
        )
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        indexes,
        [
            "lc_user_MixedCase_9",
            "lc_user_dollar$sign",
            "lc_user_éclair"
        ]
    );
}

#[test]
fn unrecognized_public_open_never_applies_requested_runtime_pragmas() {
    for (journal_mode, synchronous) in [
        (JournalMode::Wal, SynchronousMode::Normal),
        (JournalMode::Memory, SynchronousMode::Off),
        (JournalMode::Delete, SynchronousMode::Off),
    ] {
        let directory = TempDir::new().unwrap();
        let database = directory.path().join("unrecognized.sqlite3");
        let conn = Connection::open(&database).unwrap();
        conn.execute_batch(
            "CREATE TABLE unrelated(id INTEGER PRIMARY KEY);
             INSERT INTO unrelated(id) VALUES (7);
             PRAGMA user_version = 0;",
        )
        .unwrap();
        let before_schema: String = conn
            .query_row(
                "SELECT group_concat(type || ':' || name || ':' || coalesce(sql, ''), '|')
                 FROM (SELECT type, name, sql FROM sqlite_schema ORDER BY type, name)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let before_journal: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        drop(conn);

        let error = match CacheEngine::<Vec<f32>>::builder()
            .database(&database)
            .journal_mode(journal_mode)
            .synchronous(synchronous)
            .build()
        {
            Ok(_) => panic!("unrecognized database unexpectedly opened"),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("physical version 0"), "{error}");
        assert!(error.contains("database was not modified"), "{error}");

        let conn = Connection::open(&database).unwrap();
        assert_eq!(
            conn.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
                .unwrap(),
            before_journal
        );
        assert_eq!(
            conn.query_row(
                "SELECT group_concat(type || ':' || name || ':' || coalesce(sql, ''), '|')
                 FROM (SELECT type, name, sql FROM sqlite_schema ORDER BY type, name)",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            before_schema
        );
        assert_eq!(
            conn.query_row("SELECT id FROM unrelated", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            7
        );
        drop(conn);
        assert!(!directory.path().join("unrecognized.sqlite3-wal").exists());
        assert!(!directory.path().join("unrecognized.sqlite3-shm").exists());
    }
}
