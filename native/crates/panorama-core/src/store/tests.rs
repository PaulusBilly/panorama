use std::{
    fs,
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
    thread,
};

use tempfile::tempdir;

use super::*;

fn memory(now: i64) -> (Store, Arc<AtomicI64>) {
    let time = Arc::new(AtomicI64::new(now));
    let clock = Arc::clone(&time);
    let store = Store::open_in_memory_with_clock(move || clock.load(Ordering::SeqCst)).unwrap();
    (store, time)
}

fn version_at(path: &Path) -> i64 {
    Connection::open(path)
        .unwrap()
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap()
}

fn quarantines(path: &Path) -> Vec<PathBuf> {
    fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|entry| {
            entry
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains(".corrupt-")
        })
        .collect()
}

#[test]
fn round_trip_overwrite_and_remove() {
    let (store, time) = memory(100);
    let key = Key::core("profile").unwrap();
    assert_eq!(store.get(&key).unwrap(), None);
    store.set(&key, b"first").unwrap();
    assert_eq!(
        store.get_entry(&key).unwrap(),
        Some(Entry {
            value: b"first".to_vec(),
            updated_at: 100
        })
    );
    time.store(200, Ordering::SeqCst);
    store.set(&key, b"second").unwrap();
    assert_eq!(
        store.get_entry(&key).unwrap(),
        Some(Entry {
            value: b"second".to_vec(),
            updated_at: 200
        })
    );
    store.remove(&key).unwrap();
    store.remove(&key).unwrap();
    assert_eq!(store.get(&key).unwrap(), None);
}

#[test]
fn set_many_writes_and_deletes_with_one_timestamp() {
    let (store, _) = memory(123);
    let first = Key::core("library").unwrap();
    let second = Key::core("library_recent").unwrap();
    store.set(&first, b"old").unwrap();
    store
        .set_many(&[
            (first.clone(), None),
            (second.clone(), Some(b"new".to_vec())),
        ])
        .unwrap();
    assert_eq!(store.get(&first).unwrap(), None);
    assert_eq!(store.get_entry(&second).unwrap().unwrap().updated_at, 123);
    store.set_many(&[]).unwrap();
}

#[test]
fn set_many_rolls_back_writes_and_deletes_on_oversized_item() {
    let (store, _) = memory(1);
    let existing = Key::core("profile").unwrap();
    let first = Key::meta("first").unwrap();
    let failing = Key::meta("failing").unwrap();
    store.set(&existing, b"keep").unwrap();
    let result = store.set_many(&[
        (existing.clone(), None),
        (first.clone(), Some(b"first".to_vec())),
        (failing.clone(), Some(vec![0; MAX_VALUE_BYTES + 1])),
    ]);
    assert!(matches!(result, Err(StoreError::ValueTooLarge { .. })));
    assert_eq!(store.get(&existing).unwrap(), Some(b"keep".to_vec()));
    assert_eq!(store.get(&first).unwrap(), None);
    assert_eq!(store.get(&failing).unwrap(), None);
}

#[test]
fn set_many_rolls_back_on_sqlite_failure() {
    let (store, _) = memory(1);
    store
        .lock()
        .execute_batch(
            "CREATE TRIGGER fail_write BEFORE INSERT ON kv WHEN NEW.key='meta:fail'
        BEGIN SELECT RAISE(ABORT, 'rejected'); END;",
        )
        .unwrap();
    let key = Key::meta("first").unwrap();
    assert!(
        store
            .set_many(&[
                (key.clone(), Some(vec![1])),
                (Key::meta("fail").unwrap(), Some(vec![2]))
            ])
            .is_err()
    );
    assert_eq!(store.get(&key).unwrap(), None);
}

#[test]
fn key_formats_and_classes() {
    let cases = [
        (
            Key::core("schema_version").unwrap(),
            "core:schema_version",
            KeyClass::Core,
        ),
        (
            Key::catalog("com.linvo.cinemeta", "top").unwrap(),
            "catalog:com.linvo.cinemeta|top",
            KeyClass::Cache,
        ),
        (
            Key::meta("tt123:1").unwrap(),
            "meta:tt123:1",
            KeyClass::Cache,
        ),
        (Key::pref("theme").unwrap(), "pref:theme", KeyClass::Pref),
    ];
    for (key, expected, class) in cases {
        assert_eq!(key.as_str(), expected);
        assert_eq!(key.to_string(), expected);
        assert_eq!(key.class(), class);
    }
}

#[test]
fn key_components_are_validated_in_every_constructor() {
    let long = "a".repeat(513);
    let multibyte = "é".repeat(257);
    for invalid in [
        "",
        &long,
        &multibyte,
        "a\0b",
        "a\nb",
        "a\u{7f}b",
        "a\u{85}b",
        "https://addon.invalid/token",
        "//addon.invalid/token",
    ] {
        assert!(Key::core(invalid).is_err());
        assert!(Key::meta(invalid).is_err());
        assert!(Key::pref(invalid).is_err());
        assert!(Key::catalog(invalid, "valid").is_err());
        assert!(Key::catalog("valid", invalid).is_err());
    }
    assert!(Key::meta(&"é".repeat(256)).is_ok());
    assert!(Key::catalog(&"a".repeat(512), &"b".repeat(512)).is_ok());
}

#[test]
fn catalog_delimiters_cannot_collide_and_pairs_are_distinct() {
    assert!(Key::catalog("a|b", "c").is_err());
    assert!(Key::catalog("a", "b|c").is_err());
    let components = ["a", "ab", "b", "bc", "a:b", "%7C", "é"];
    let mut keys = std::collections::HashSet::new();
    for addon in components {
        for catalog in components {
            assert!(keys.insert(Key::catalog(addon, catalog).unwrap()));
        }
    }
    assert_eq!(keys.len(), components.len().pow(2));
}

#[test]
fn empty_file_migrates_and_reopening_preserves_schema_and_bytes() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("panorama.db");
    fs::write(&path, []).unwrap();
    let (store, outcome) = Store::open(&path).unwrap();
    assert_eq!(outcome, OpenOutcome::Opened);
    assert_eq!(
        store
            .lock()
            .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
            .unwrap(),
        SCHEMA_VERSION
    );
    let key = Key::core("profile").unwrap();
    store.set(&key, b"saved").unwrap();
    drop(store);
    let bytes = fs::read(&path).unwrap();
    let (store, outcome) = Store::open(&path).unwrap();
    assert_eq!(outcome, OpenOutcome::Opened);
    assert_eq!(store.get(&key).unwrap(), Some(b"saved".to_vec()));
    drop(store);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(version_at(&path), SCHEMA_VERSION);
    assert!(quarantines(&path).is_empty());
}

#[test]
fn schema_has_without_rowid_and_cache_pruning_index() {
    let (store, _) = memory(1);
    let connection = store.lock();
    let sql: String = connection
        .query_row("SELECT sql FROM sqlite_schema WHERE name='kv'", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(sql.contains("WITHOUT ROWID"));
    let columns: Vec<(String, String, i64, i64)> = connection
        .prepare("PRAGMA table_info(kv)")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get(1)?, row.get(2)?, row.get(3)?, row.get(5)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        columns,
        [
            ("key".into(), "TEXT".into(), 1, 1),
            ("value".into(), "BLOB".into(), 1, 0),
            ("updated_at".into(), "INTEGER".into(), 1, 0)
        ]
    );
    let plan: String = connection
        .query_row(
            "EXPLAIN QUERY PLAN SELECT key FROM kv INDEXED BY kv_cache_updated_at
        WHERE (key GLOB 'catalog:*' OR key GLOB 'meta:*') AND updated_at < 100",
            [],
            |row| row.get(3),
        )
        .unwrap();
    assert!(plan.contains("kv_cache_updated_at"), "{plan}");
}

#[test]
fn newer_schema_is_byte_for_byte_untouched() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("newer.db");
    let connection = Connection::open(&path).unwrap();
    connection
        .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
        .unwrap();
    connection
        .execute_batch("CREATE TABLE future(data TEXT); INSERT INTO future VALUES ('keep');")
        .unwrap();
    drop(connection);
    let bytes = fs::read(&path).unwrap();
    assert!(
        matches!(Store::open(&path), Err(StoreError::NewerSchema { found, supported }) if found == SCHEMA_VERSION + 1 && supported == SCHEMA_VERSION)
    );
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert!(quarantines(&path).is_empty());
    assert!(!recovery::sibling(&path, "-wal").exists());
}

#[test]
fn migration_failure_rolls_back_without_quarantine() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("conflict.db");
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch("CREATE TABLE kv(existing TEXT);")
        .unwrap();
    drop(connection);
    assert!(matches!(Store::open(&path), Err(StoreError::Sqlite { .. })));
    assert_eq!(version_at(&path), 0);
    assert!(quarantines(&path).is_empty());
}

#[test]
fn file_open_pragmas_are_configured() {
    let dir = tempdir().unwrap();
    let (store, outcome) = Store::open(&dir.path().join("nested/panorama.db")).unwrap();
    assert_eq!(outcome, OpenOutcome::Created);
    let connection = store.lock();
    assert_eq!(
        connection
            .pragma_query_value::<String, _>(None, "journal_mode", |row| row.get(0))
            .unwrap(),
        "wal"
    );
    for (name, expected) in [
        ("synchronous", 1),
        ("foreign_keys", 1),
        ("busy_timeout", 5000),
        ("secure_delete", 1),
    ] {
        assert_eq!(
            connection
                .pragma_query_value::<i64, _>(None, name, |row| row.get(0))
                .unwrap(),
            expected
        );
    }
}

#[test]
fn random_bytes_are_quarantined_and_replaced() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("random.db");
    let bytes: Vec<u8> = (0..4096).map(|i| ((i * 73 + 19) % 256) as u8).collect();
    fs::write(&path, &bytes).unwrap();
    let (store, outcome) = Store::open_with_clock(&path, || 1000).unwrap();
    let quarantined = recovery::sibling(&path, ".corrupt-1000");
    assert_eq!(
        outcome,
        OpenOutcome::RecoveredFromCorruption {
            quarantined: quarantined.clone()
        }
    );
    assert_eq!(fs::read(quarantined).unwrap(), bytes);
    assert_eq!(store.get(&Key::core("profile").unwrap()).unwrap(), None);
    store.set(&Key::pref("theme").unwrap(), b"dark").unwrap();
}

#[test]
fn truncated_database_is_quarantined() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("truncated.db");
    let (store, _) = Store::open(&path).unwrap();
    store
        .set(&Key::core("profile").unwrap(), &vec![42; 20_000])
        .unwrap();
    drop(store);
    let mut bytes = fs::read(&path).unwrap();
    bytes.truncate(bytes.len() / 2);
    fs::write(&path, &bytes).unwrap();
    let (store, outcome) = Store::open_with_clock(&path, || 1000).unwrap();
    assert!(matches!(
        outcome,
        OpenOutcome::RecoveredFromCorruption { .. }
    ));
    assert_eq!(
        fs::read(recovery::sibling(&path, ".corrupt-1000")).unwrap(),
        bytes
    );
    assert_eq!(store.get(&Key::core("profile").unwrap()).unwrap(), None);
}

#[test]
fn quick_check_failure_is_quarantined() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("check.db");
    let (store, _) = Store::open(&path).unwrap();
    store
        .lock()
        .execute_batch(
            "CREATE TABLE broken(value INTEGER); INSERT INTO broken VALUES (NULL);
        PRAGMA writable_schema=ON;
        UPDATE sqlite_schema SET sql='CREATE TABLE broken(value INTEGER NOT NULL)' WHERE name='broken';",
        )
        .unwrap();
    drop(store);
    let (_, outcome) = Store::open_with_clock(&path, || 1000).unwrap();
    assert!(matches!(
        outcome,
        OpenOutcome::RecoveredFromCorruption { .. }
    ));
}

#[test]
fn quarantine_moves_sidecars_and_keeps_only_two_newest_groups() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("panorama.db");
    fs::write(dir.path().join("other.db.corrupt-1"), b"unrelated").unwrap();
    for timestamp in [100, 200, 300] {
        fs::write(&path, b"not a database").unwrap();
        fs::write(recovery::sibling(&path, "-wal"), b"wal marker").unwrap();
        fs::write(recovery::sibling(&path, "-shm"), b"shm marker").unwrap();
        let (store, outcome) = Store::open_with_clock(&path, move || timestamp).unwrap();
        let quarantined = recovery::sibling(&path, &format!(".corrupt-{timestamp}"));
        assert_eq!(
            outcome,
            OpenOutcome::RecoveredFromCorruption {
                quarantined: quarantined.clone()
            }
        );
        assert_eq!(
            fs::read(recovery::sibling(&quarantined, "-wal")).unwrap(),
            b"wal marker"
        );
        assert_eq!(
            fs::read(recovery::sibling(&quarantined, "-shm")).unwrap(),
            b"shm marker"
        );
        drop(store);
    }
    for suffix in ["", "-wal", "-shm"] {
        assert!(!recovery::sibling(&path, &format!(".corrupt-100{suffix}")).exists());
        assert!(recovery::sibling(&path, &format!(".corrupt-200{suffix}")).exists());
        assert!(recovery::sibling(&path, &format!(".corrupt-300{suffix}")).exists());
    }
    assert!(dir.path().join("other.db.corrupt-1").exists());
}

#[test]
fn quarantine_same_timestamp_does_not_overwrite_existing_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("panorama.db");
    for contents in [
        b"first corruption".as_slice(),
        b"second corruption".as_slice(),
    ] {
        fs::write(&path, contents).unwrap();
        let (store, _) = Store::open_with_clock(&path, || 100).unwrap();
        drop(store);
    }
    assert_eq!(
        fs::read(recovery::sibling(&path, ".corrupt-100")).unwrap(),
        b"first corruption"
    );
    assert_eq!(
        fs::read(recovery::sibling(&path, ".corrupt-101")).unwrap(),
        b"second corruption"
    );
}

#[test]
fn directory_path_failure_quarantines_nothing() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("directory.db");
    fs::create_dir(&path).unwrap();
    assert!(Store::open(&path).is_err());
    assert!(path.is_dir());
    assert!(quarantines(&path).is_empty());
}

#[test]
fn only_corruption_error_codes_allow_recovery() {
    for code in [rusqlite::ffi::SQLITE_CORRUPT, rusqlite::ffi::SQLITE_NOTADB] {
        let error = StoreError::from(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(code),
            None,
        ));
        assert!(error.is_corruption());
    }
    for code in [
        rusqlite::ffi::SQLITE_BUSY,
        rusqlite::ffi::SQLITE_LOCKED,
        rusqlite::ffi::SQLITE_FULL,
        rusqlite::ffi::SQLITE_IOERR,
        rusqlite::ffi::SQLITE_PERM,
    ] {
        let error = StoreError::from(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(code),
            None,
        ));
        assert!(!error.is_corruption());
    }
}

#[test]
fn pruning_by_age_preserves_boundary_core_and_prefs() {
    let (store, time) = memory(0);
    let expired = Key::catalog("addon", "old").unwrap();
    let expired_meta = Key::meta("old").unwrap();
    let core = Key::core("profile").unwrap();
    let pref = Key::pref("theme").unwrap();
    for key in [&expired, &expired_meta, &core, &pref] {
        store.set(key, b"value").unwrap();
    }
    time.store(1, Ordering::SeqCst);
    let boundary = Key::meta("boundary").unwrap();
    store.set(&boundary, b"keep").unwrap();
    time.store(2, Ordering::SeqCst);
    let fresh = Key::catalog("addon", "fresh").unwrap();
    store.set(&fresh, b"keep").unwrap();
    assert_eq!(
        store.prune_cache(CACHE_MAX_AGE_MS + 1).unwrap(),
        PruneReport {
            removed_by_age: 2,
            removed_by_count: 0
        }
    );
    assert_eq!(store.get(&expired).unwrap(), None);
    assert_eq!(store.get(&expired_meta).unwrap(), None);
    for key in [&core, &pref, &boundary, &fresh] {
        assert!(store.get(key).unwrap().is_some());
    }
    assert_eq!(store.prune_cache(i64::MIN).unwrap(), PruneReport::default());
}

#[test]
fn pruning_by_count_removes_the_100_oldest_cache_rows() {
    let (store, time) = memory(0);
    let core = Key::core("profile").unwrap();
    let pref = Key::pref("theme").unwrap();
    store.set(&core, b"keep").unwrap();
    store.set(&pref, b"keep").unwrap();
    let mut keys = Vec::new();
    for index in 0..2100 {
        time.store(index, Ordering::SeqCst);
        let key = if index % 2 == 0 {
            Key::meta(&index.to_string()).unwrap()
        } else {
            Key::catalog("addon", &index.to_string()).unwrap()
        };
        store.set(&key, b"cached").unwrap();
        keys.push(key);
    }
    assert_eq!(
        store.prune_cache(2100).unwrap(),
        PruneReport {
            removed_by_age: 0,
            removed_by_count: 100
        }
    );
    for (index, key) in keys.iter().enumerate() {
        assert_eq!(store.get(key).unwrap().is_some(), index >= 100);
    }
    assert!(store.get(&core).unwrap().is_some());
    assert!(store.get(&pref).unwrap().is_some());
    assert_eq!(store.prune_cache(2100).unwrap(), PruneReport::default());
}

#[test]
fn sign_out_erases_session_bytes_from_database_and_wal() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("panorama.db");
    let (store, _) = Store::open(&path).unwrap();
    let marker = b"RECOGNISABLE_SESSION_KEY_9679535500";
    let value = marker.repeat(1000);
    let core = Key::core("profile").unwrap();
    let meta = Key::meta("tt1").unwrap();
    let catalog = Key::catalog("addon", "top").unwrap();
    let pref = Key::pref("theme").unwrap();
    store.set(&core, &value).unwrap();
    store
        .lock()
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    assert!(
        fs::read(&path)
            .unwrap()
            .windows(marker.len())
            .any(|part| part == marker)
    );
    store.set(&core, &value).unwrap();
    for key in [&meta, &catalog] {
        store.set(key, b"cache").unwrap();
    }
    store.set(&pref, b"dark").unwrap();
    let wal = recovery::sibling(&path, "-wal");
    assert!(
        fs::read(&wal)
            .unwrap()
            .windows(marker.len())
            .any(|part| part == marker)
    );
    store.clear_on_sign_out().unwrap();
    for key in [&core, &meta, &catalog] {
        assert_eq!(store.get(key).unwrap(), None);
    }
    assert_eq!(store.get(&pref).unwrap(), Some(b"dark".to_vec()));
    for file in [&path, &wal] {
        if file.exists() {
            assert!(
                !fs::read(file)
                    .unwrap()
                    .windows(marker.len())
                    .any(|part| part == marker)
            );
        }
    }
    assert_eq!(fs::metadata(wal).unwrap().len(), 0);
    store.clear_on_sign_out().unwrap();
}

#[test]
fn sign_out_deletion_is_atomic_on_sqlite_failure() {
    let (store, _) = memory(1);
    let core = Key::core("profile").unwrap();
    let meta = Key::meta("film").unwrap();
    store.set(&core, b"session").unwrap();
    store.set(&meta, b"cache").unwrap();
    store
        .lock()
        .execute_batch(
            "CREATE TRIGGER fail_delete BEFORE DELETE ON kv WHEN OLD.key='meta:film'
        BEGIN SELECT RAISE(ABORT, 'rejected'); END;",
        )
        .unwrap();
    assert!(store.clear_on_sign_out().is_err());
    assert!(store.get(&core).unwrap().is_some());
    assert!(store.get(&meta).unwrap().is_some());
}

#[test]
fn sign_out_reports_blocked_checkpoint_and_can_be_retried() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("panorama.db");
    let (store, _) = Store::open(&path).unwrap();
    store
        .set(&Key::core("profile").unwrap(), b"session")
        .unwrap();
    let reader = Connection::open(&path).unwrap();
    reader.execute_batch("BEGIN; SELECT * FROM kv;").unwrap();
    store.lock().busy_timeout(Duration::ZERO).unwrap();
    assert!(matches!(
        store.clear_on_sign_out(),
        Err(StoreError::CheckpointBusy)
    ));
    reader.execute_batch("ROLLBACK;").unwrap();
    store.clear_on_sign_out().unwrap();
}

#[test]
fn value_size_limit_accepts_boundary_and_rejects_overflow() {
    let (store, _) = memory(1);
    let key = Key::meta("film").unwrap();
    store.set(&key, &vec![7; MAX_VALUE_BYTES]).unwrap();
    assert_eq!(store.get(&key).unwrap().unwrap().len(), MAX_VALUE_BYTES);
    assert!(
        matches!(store.set(&key, &vec![8; MAX_VALUE_BYTES + 1]), Err(StoreError::ValueTooLarge { size, limit }) if size == MAX_VALUE_BYTES + 1 && limit == MAX_VALUE_BYTES)
    );
    assert_eq!(store.get(&key).unwrap().unwrap()[0], 7);
    store.set(&key, b"").unwrap();
    assert_eq!(store.get(&key).unwrap(), Some(vec![]));
}

#[test]
fn errors_implement_error_and_discard_sqlite_messages() {
    fn assert_error<T: std::error::Error>() {}
    assert_error::<StoreError>();
    let marker = "SENSITIVE_VALUE_IN_SQLITE_ERROR";
    let error = StoreError::from(rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT),
        Some(marker.to_string()),
    ));
    assert!(!error.to_string().contains(marker));
    assert!(!format!("{error:?}").contains(marker));
    assert!(std::error::Error::source(&error).is_none());
}

#[test]
fn default_path_uses_platform_environment_and_handles_missing_base() {
    assert_eq!(default_path_from(|_| None), None);
    assert_eq!(default_path_from(|_| Some(PathBuf::new())), None);
    #[cfg(windows)]
    {
        assert_eq!(
            default_path_from(|name| (name == "LOCALAPPDATA")
                .then(|| PathBuf::from("C:/Users/test/AppData/Local"))),
            Some(PathBuf::from(
                "C:/Users/test/AppData/Local/Panorama/panorama.db"
            ))
        );
        assert_eq!(
            default_path_from(|name| (name == "HOME").then(|| PathBuf::from("C:/Users/test"))),
            None
        );
    }
    #[cfg(target_os = "macos")]
    assert_eq!(
        default_path_from(|name| (name == "HOME").then(|| PathBuf::from("/home/test"))),
        Some(PathBuf::from(
            "/home/test/Library/Application Support/Panorama/panorama.db"
        ))
    );
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        assert_eq!(
            default_path_from(|name| match name {
                "XDG_DATA_HOME" => Some("/data".into()),
                "HOME" => Some("/home/test".into()),
                _ => None,
            }),
            Some(PathBuf::from("/data/panorama/panorama.db"))
        );
        assert_eq!(
            default_path_from(|name| (name == "HOME").then(|| PathBuf::from("/home/test"))),
            Some(PathBuf::from(
                "/home/test/.local/share/panorama/panorama.db"
            ))
        );
    }
}

#[cfg(unix)]
#[test]
fn unix_file_and_new_directories_have_owner_only_modes() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempdir().unwrap();
    let path = dir.path().join("outer/inner/panorama.db");
    let (store, _) = Store::open(&path).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    for directory in [
        path.parent().unwrap(),
        path.parent().unwrap().parent().unwrap(),
    ] {
        assert_eq!(
            fs::metadata(directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    drop(store);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    let (_store, _) = Store::open(&path).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn store_is_send_sync_and_eight_threads_keep_every_write() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Store>();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let threads: Vec<_> = (0..8)
        .map(|worker| {
            let store = Arc::clone(&store);
            thread::spawn(move || {
                for index in 0..100 {
                    let key = Key::meta(&format!("{worker}:{index}")).unwrap();
                    let value = format!("{worker}-{index}").into_bytes();
                    store.set(&key, &value).unwrap();
                    assert_eq!(store.get(&key).unwrap(), Some(value));
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    for worker in 0..8 {
        for index in 0..100 {
            assert_eq!(
                store
                    .get(&Key::meta(&format!("{worker}:{index}")).unwrap())
                    .unwrap(),
                Some(format!("{worker}-{index}").into_bytes())
            );
        }
    }
}

#[test]
fn poisoned_mutex_recovers_the_connection() {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let worker_store = Arc::clone(&store);
    assert!(
        thread::spawn(move || {
            let _guard = worker_store.lock();
            panic!("poison");
        })
        .join()
        .is_err()
    );
    let key = Key::pref("theme").unwrap();
    store.set(&key, b"dark").unwrap();
    assert_eq!(store.get(&key).unwrap(), Some(b"dark".to_vec()));
}

#[test]
fn sign_out_deletes_quarantined_copies_holding_session_bytes() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("panorama.db");
    let marker = b"QUARANTINED_SESSION_KEY_5512";
    // A corrupt file that still contains the old session key bytes.
    let mut bytes = b"not sqlite at all".repeat(64);
    bytes.extend_from_slice(marker);
    fs::write(&path, &bytes).unwrap();
    fs::write(recovery::sibling(&path, "-wal"), marker).unwrap();
    let (store, outcome) = Store::open_with_clock(&path, || 2000).unwrap();
    assert!(matches!(
        outcome,
        OpenOutcome::RecoveredFromCorruption { .. }
    ));
    assert_eq!(quarantines(&path).len(), 2); // database + its -wal
    store.set(&Key::pref("theme").unwrap(), b"dark").unwrap();

    store.clear_on_sign_out().unwrap();

    assert!(quarantines(&path).is_empty());
    for entry in fs::read_dir(dir.path()).unwrap() {
        let entry = entry.unwrap();
        assert!(
            !fs::read(entry.path())
                .unwrap()
                .windows(marker.len())
                .any(|part| part == marker),
            "{} still holds the session marker",
            entry.path().display()
        );
    }
    assert_eq!(
        store.get(&Key::pref("theme").unwrap()).unwrap(),
        Some(b"dark".to_vec())
    );
}
