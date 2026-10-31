use std::{
    cell::Cell,
    sync::{
        Arc, Mutex,
        atomic::{AtomicI64, Ordering},
    },
    time::Duration,
};

use serde_json::{Value, json};
use stremio_core::{constants::*, runtime::Env};

use super::TestEnv;
use crate::{
    store::{Key, Store},
    stremio::{
        CoreErrorKind, CoreSession,
        env::{EnvConfig, PanoramaEnv},
    },
};

#[test]
fn env_storage_round_trip_exact_json_and_delete() {
    let fixture = TestEnv::memory(None);
    fixture.runtime.block_on(async {
        let value = json!({"auth": null, "nested": [1, "two"]});
        PanoramaEnv::set_storage("test_bucket", Some(&value))
            .await
            .unwrap();
        assert_eq!(
            fixture
                .state
                .store
                .get(&Key::core("test_bucket").unwrap())
                .unwrap(),
            Some(serde_json::to_vec(&value).unwrap())
        );
        assert_eq!(
            PanoramaEnv::get_storage::<Value>("test_bucket")
                .await
                .unwrap(),
            Some(value)
        );
        PanoramaEnv::set_storage::<Value>("test_bucket", None)
            .await
            .unwrap();
        assert_eq!(
            PanoramaEnv::get_storage::<Value>("test_bucket")
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            fixture
                .state
                .store
                .get(&Key::core("test_bucket").unwrap())
                .unwrap(),
            None
        );
    });
}

#[test]
fn borrowed_non_send_value_is_serialized_before_future_is_returned() {
    let fixture = TestEnv::memory(None);
    let value = Cell::new(7_u32);
    let write = PanoramaEnv::set_storage("borrowed", Some(&value));
    value.set(99);
    fixture.runtime.block_on(async {
        write.await.unwrap();
        assert_eq!(
            PanoramaEnv::get_storage::<u32>("borrowed").await.unwrap(),
            Some(7)
        );
    });
}

#[test]
fn sequential_writes_preserve_submission_order_across_buckets() {
    let clock = Arc::new(AtomicI64::new(0));
    let tick = clock.clone();
    let fixture = TestEnv::new(
        Store::open_in_memory_with_clock(move || tick.fetch_add(1, Ordering::SeqCst)).unwrap(),
        None,
    );
    let order = Arc::new(Mutex::new(vec![]));
    fixture.runtime.block_on(async {
        for index in 0..100_u32 {
            for bucket in ["ordered", "interleaved"] {
                let write = PanoramaEnv::set_storage(bucket, Some(&index));
                let order = order.clone();
                PanoramaEnv::exec_sequential(async move {
                    if index == 0 {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                    write.await.unwrap();
                    order.lock().unwrap().push((bucket, index));
                });
            }
        }
        fixture.state.drain().await.unwrap();
        assert_eq!(
            *order.lock().unwrap(),
            (0..100)
                .flat_map(|index| { [("ordered", index), ("interleaved", index)] })
                .collect::<Vec<_>>()
        );
        for (bucket, timestamp) in [("ordered", 198), ("interleaved", 199)] {
            let entry = fixture
                .state
                .store
                .get_entry(&Key::core(bucket).unwrap())
                .unwrap()
                .unwrap();
            assert_eq!(entry.value, b"99");
            assert_eq!(entry.updated_at, timestamp);
        }
    });
}

#[test]
fn empty_start_runs_all_migrations_and_uses_official_addons() {
    let fixture = TestEnv::memory(None);
    fixture.runtime.block_on(async {
        assert_eq!(
            PanoramaEnv::get_storage::<u32>(SCHEMA_VERSION_STORAGE_KEY)
                .await
                .unwrap(),
            None
        );
        let session = CoreSession::start().await.unwrap();
        assert!(!session.is_signed_in());
        assert_eq!(
            session.profile().as_core(),
            &stremio_core::types::profile::Profile::default()
        );
        assert_eq!(session.installed_addons().len(), OFFICIAL_ADDONS.len());
        assert_eq!(
            PanoramaEnv::get_storage::<u32>(SCHEMA_VERSION_STORAGE_KEY)
                .await
                .unwrap(),
            Some(SCHEMA_VERSION)
        );
    });
}

#[test]
fn migration_transforms_legacy_state_and_rejects_future_schema() {
    let fixture = TestEnv::memory(None);
    fixture.runtime.block_on(async {
        PanoramaEnv::set_storage(PROFILE_STORAGE_KEY, Some(&json!({"obsolete": true})))
            .await
            .unwrap();
        let session = CoreSession::start().await.unwrap();
        assert!(!session.is_signed_in());
        assert_eq!(
            PanoramaEnv::get_storage::<Value>(PROFILE_STORAGE_KEY)
                .await
                .unwrap(),
            None
        );
        drop(session);
        let _lease = fixture.state.session_gate.lock().await;
        PanoramaEnv::set_storage(SCHEMA_VERSION_STORAGE_KEY, Some(&(SCHEMA_VERSION + 1)))
            .await
            .unwrap();
    });
    fixture.runtime.block_on(async {
        assert_eq!(
            CoreSession::start().await.unwrap_err().kind,
            CoreErrorKind::Storage
        );
        assert_eq!(
            PanoramaEnv::get_storage::<u32>(SCHEMA_VERSION_STORAGE_KEY)
                .await
                .unwrap(),
            Some(SCHEMA_VERSION + 1)
        );
    });
}

#[test]
fn migration_version_write_failure_rolls_back_buckets_and_retry_preserves_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("migration.db");
    let fixture = TestEnv::new(Store::open(&path).unwrap().0, None);
    let connection = rusqlite::Connection::open(&path).unwrap();
    fixture.runtime.block_on(async {
        let legacy = json!({"marker": "keep", "settings": {
            "interface_language": "en", "streaming_server_url": "http://localhost:11470",
            "binge_watching": true, "play_in_background": false,
            "play_in_external_player": false, "hardware_decoding": true,
            "subtitles_language": "en", "subtitles_size": 100, "subtitles_font": "Arial",
            "subtitles_bold": false, "subtitles_offset": 0,
            "subtitles_text_color": "white", "subtitles_background_color": "black",
            "subtitles_outline_color": "black"
        }});
        PanoramaEnv::set_storage(SCHEMA_VERSION_STORAGE_KEY, Some(&1_u32))
            .await
            .unwrap();
        PanoramaEnv::set_storage(PROFILE_STORAGE_KEY, Some(&legacy))
            .await
            .unwrap();
        connection
            .execute_batch(
                "CREATE TRIGGER reject_version BEFORE INSERT ON kv
             WHEN NEW.key='core:schema_version'
             BEGIN SELECT RAISE(ABORT, 'injected version write failure'); END;",
            )
            .unwrap();
        assert!(PanoramaEnv::migrate_storage_schema().await.is_err());
        assert_eq!(
            PanoramaEnv::get_storage::<u32>(SCHEMA_VERSION_STORAGE_KEY)
                .await
                .unwrap(),
            Some(1)
        );
        assert_eq!(
            PanoramaEnv::get_storage::<Value>(PROFILE_STORAGE_KEY)
                .await
                .unwrap(),
            Some(legacy)
        );
        connection
            .execute_batch("DROP TRIGGER reject_version")
            .unwrap();
        PanoramaEnv::migrate_storage_schema().await.unwrap();
        let migrated = PanoramaEnv::get_storage::<Value>(PROFILE_STORAGE_KEY)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(migrated["marker"], "keep");
        assert_eq!(migrated["settings"]["interfaceLanguage"], "en");
        assert_eq!(
            PanoramaEnv::get_storage::<u32>(SCHEMA_VERSION_STORAGE_KEY)
                .await
                .unwrap(),
            Some(SCHEMA_VERSION)
        );
    });
}

#[test]
fn all_env_store_calls_use_blocking_workers_not_callers_thread() {
    let fixture = TestEnv::memory(None);
    let caller = std::thread::current().id();
    fixture.runtime.block_on(async {
        PanoramaEnv::set_storage("worker", Some(&json!(5)))
            .await
            .unwrap();
        PanoramaEnv::get_storage::<Value>("worker").await.unwrap();
        PanoramaEnv::set_storage::<Value>("worker", None)
            .await
            .unwrap();
        let mut session = CoreSession::start().await.unwrap();
        session.sign_out().await.unwrap();
    });
    fixture.idle();
    let threads = fixture.state.store_threads.lock().unwrap();
    assert!(threads.len() > 10);
    assert!(threads.iter().all(|thread| *thread != caller));
}

#[test]
fn second_install_is_rejected_without_exposing_config() {
    let fixture = TestEnv::memory(None);
    let config = EnvConfig {
        store: fixture.state.store.clone(),
        api_base: Some(
            "https://email:password@example.test/secret?token=auth-key"
                .parse()
                .unwrap(),
        ),
        runtime: fixture.runtime.handle().clone(),
    };
    assert_eq!(format!("{config:?}"), "EnvConfig { .. }");
    assert_eq!(
        PanoramaEnv::install(config).unwrap_err().kind,
        CoreErrorKind::AlreadyInstalled
    );
}

#[test]
fn sequential_storage_failure_is_reported_by_barrier() {
    let fixture = TestEnv::memory(None);
    fixture.runtime.block_on(async {
        let value = "x".repeat(crate::store::MAX_VALUE_BYTES);
        let write = PanoramaEnv::set_storage("too_large", Some(&value));
        PanoramaEnv::exec_sequential(async move {
            assert!(write.await.is_err());
        });
        assert_eq!(
            fixture.state.drain().await.unwrap_err().kind,
            CoreErrorKind::Storage
        );
        fixture.state.drain().await.unwrap();
    });
}
