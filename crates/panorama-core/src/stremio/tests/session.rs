use std::{fs, sync::atomic::Ordering};

use serde_json::json;
use stremio_core::{
    constants::*,
    runtime::Env,
    types::{
        library::{LibraryBucket, LibraryItem, LibraryItemState},
        profile::Profile as CoreProfile,
    },
};
use tempfile::tempdir;

use super::{
    TestEnv,
    mock::{AUTH_KEY, EMAIL, MockApi, PASSWORD, addon},
};
use crate::{
    store::{Key, Store},
    stremio::{CoreChange, CoreErrorKind, CoreSession, env::PanoramaEnv, model},
};

#[test]
fn mock_sign_in_persists_account_order_across_file_reopen_without_network() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("account.db");
    let api = MockApi::start();
    let mut fixture = TestEnv::new(Store::open(&path).unwrap().0, Some(api.base.clone()));
    fixture.runtime.block_on(async {
        let mut session = CoreSession::start().await.unwrap();
        let profile = session
            .sign_in(EMAIL.into(), PASSWORD.into())
            .await
            .unwrap();
        assert!(session.is_signed_in());
        assert_eq!(profile.as_core().auth.as_ref().unwrap().user.email, EMAIL);
        assert_eq!(
            session
                .installed_addons()
                .iter()
                .map(|addon| addon.as_core().manifest.id.clone())
                .collect::<Vec<_>>(),
            ["second", "first"]
        );
        assert_eq!(
            PanoramaEnv::get_storage::<CoreProfile>(PROFILE_STORAGE_KEY)
                .await
                .unwrap()
                .unwrap(),
            *profile.as_core()
        );
        let calls = api.calls.lock().unwrap();
        assert_eq!(calls[0], "/api/login");
        assert!(calls.iter().any(|path| path == "/api/addonCollectionGet"));
        assert!(calls.iter().any(|path| path == "/api/datastoreGet"));
    });
    fixture.replace(Store::open_in_memory().unwrap(), Some(api.base.clone()));
    fixture.replace(Store::open(&path).unwrap().0, Some(api.base.clone()));
    let calls = api.calls.lock().unwrap().len();
    fixture.runtime.block_on(async {
        let session = CoreSession::start().await.unwrap();
        assert!(session.is_signed_in());
        assert_eq!(
            session
                .installed_addons()
                .iter()
                .map(|addon| addon.as_core().manifest.id.clone())
                .collect::<Vec<_>>(),
            ["second", "first"]
        );
        assert_eq!(api.calls.lock().unwrap().len(), calls);
    });
    fixture.idle();
    assert_eq!(api.calls.lock().unwrap().len(), calls);
}

#[test]
fn failed_collection_fetch_never_succeeds_with_fallback_account_addons() {
    let api = MockApi::start();
    api.reject_addons.store(true, Ordering::Relaxed);
    let fixture = TestEnv::memory(Some(api.base.clone()));
    fixture.runtime.block_on(async {
        let mut session = CoreSession::start().await.unwrap();
        let error = session
            .sign_in(EMAIL.into(), PASSWORD.into())
            .await
            .unwrap_err();
        assert_eq!(error.kind, CoreErrorKind::Other);
        assert!(!session.is_signed_in());
        assert_eq!(session.profile().as_core(), &CoreProfile::default());
    });
}

#[test]
fn sign_out_then_sign_in_same_session_survives_restart_with_library() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("account.db");
    let api = MockApi::start();
    let mut fixture = TestEnv::new(Store::open(&path).unwrap().0, Some(api.base.clone()));
    fixture.runtime.block_on(async {
        let mut session = CoreSession::start().await.unwrap();
        session
            .sign_in(EMAIL.into(), PASSWORD.into())
            .await
            .unwrap();
        session.sign_out().await.unwrap();
        session
            .sign_in(EMAIL.into(), PASSWORD.into())
            .await
            .unwrap();
        let uid = session.profile().as_core().uid();
        PanoramaEnv::set_storage(
            LIBRARY_RECENT_STORAGE_KEY,
            Some(&LibraryBucket::new(
                uid,
                vec![item("saved", "saved", "2025-01-01T00:00:00Z")],
            )),
        )
        .await
        .unwrap();
    });
    fixture.replace(Store::open_in_memory().unwrap(), Some(api.base.clone()));
    fixture.replace(Store::open(&path).unwrap().0, Some(api.base.clone()));
    fixture.runtime.block_on(async {
        let session = CoreSession::start().await.unwrap();
        assert!(session.is_signed_in());
        assert_eq!(
            session.installed_addons()[0].as_core().manifest.id,
            "second"
        );
        assert!(
            model::rehydrate()
                .await
                .unwrap()
                .library
                .items
                .contains_key("saved")
        );
    });
}

#[test]
fn sign_out_removes_auth_bytes_from_database_wal_and_quarantines() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("account.db");
    let api = MockApi::start();
    let fixture = TestEnv::new(Store::open(&path).unwrap().0, Some(api.base.clone()));
    fixture.runtime.block_on(async {
        let mut session = CoreSession::start().await.unwrap();
        session
            .sign_in(EMAIL.into(), PASSWORD.into())
            .await
            .unwrap();
        let wal = dir.path().join("account.db-wal");
        assert!(
            fs::read(&wal)
                .unwrap()
                .windows(AUTH_KEY.len())
                .any(|part| part == AUTH_KEY.as_bytes())
        );
        let reader = rusqlite::Connection::open(&path).unwrap();
        reader
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .unwrap();
        assert!(
            fs::read(&path)
                .unwrap()
                .windows(AUTH_KEY.len())
                .any(|part| part == AUTH_KEY.as_bytes())
        );
        drop(reader);
        PanoramaEnv::set_storage(PROFILE_STORAGE_KEY, Some(session.profile().as_core()))
            .await
            .unwrap();
        for suffix in [".corrupt-100", ".corrupt-100-wal", ".corrupt-100-shm"] {
            fs::write(dir.path().join(format!("account.db{suffix}")), AUTH_KEY).unwrap();
        }
        session.sign_out().await.unwrap();
        assert!(
            api.calls
                .lock()
                .unwrap()
                .iter()
                .any(|path| path == "/api/logout")
        );
        assert!(!session.is_signed_in());
        assert_eq!(session.profile().as_core(), &CoreProfile::default());
        for file in [&path, &wal] {
            assert!(
                !fs::read(file)
                    .unwrap()
                    .windows(AUTH_KEY.len())
                    .any(|part| part == AUTH_KEY.as_bytes())
            );
        }
        assert_eq!(fs::metadata(&wal).unwrap().len(), 0);
        assert!(!dir.path().join("account.db.corrupt-100").exists());
        assert!(!dir.path().join("account.db.corrupt-100-wal").exists());
        assert!(!dir.path().join("account.db.corrupt-100-shm").exists());
        assert_eq!(
            fixture
                .state
                .store
                .get(&Key::core(PROFILE_STORAGE_KEY).unwrap())
                .unwrap(),
            None
        );
        drop(session);
        let session = CoreSession::start().await.unwrap();
        assert!(!session.is_signed_in());
    });
}

#[test]
fn failed_sign_out_clears_memory_and_can_be_retried() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("account.db");
    let api = MockApi::start();
    let fixture = TestEnv::new(Store::open(&path).unwrap().0, Some(api.base.clone()));
    fixture.runtime.block_on(async {
        let mut session = CoreSession::start().await.unwrap();
        session
            .sign_in(EMAIL.into(), PASSWORD.into())
            .await
            .unwrap();
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TRIGGER reject_clear BEFORE DELETE ON kv
            WHEN OLD.key='core:profile' BEGIN SELECT RAISE(ABORT, 'private failure'); END;",
            )
            .unwrap();
        assert_eq!(
            session.sign_out().await.unwrap_err().kind,
            CoreErrorKind::Storage
        );
        assert!(!session.is_signed_in());
        assert_eq!(session.profile().as_core(), &CoreProfile::default());
        connection
            .execute_batch("DROP TRIGGER reject_clear")
            .unwrap();
        session.sign_out().await.unwrap();
        assert_eq!(
            PanoramaEnv::get_storage::<CoreProfile>(PROFILE_STORAGE_KEY)
                .await
                .unwrap(),
            None
        );
    });
}

#[test]
fn profile_session_addons_errors_and_changes_have_redacted_debug() {
    let api = MockApi::start();
    let fixture = TestEnv::memory(Some(api.base.clone()));
    fixture.runtime.block_on(async {
        let mut session = CoreSession::start().await.unwrap();
        let mut changes = session.subscribe();
        let profile = session
            .sign_in(EMAIL.into(), PASSWORD.into())
            .await
            .unwrap();
        let mut seen = vec![];
        while let Ok(change) = changes.try_recv() {
            seen.push(change);
        }
        assert!(seen.contains(&CoreChange::ProfileChanged));
        assert!(seen.contains(&CoreChange::AddonsChanged));
        assert!(seen.contains(&CoreChange::LibraryChanged));
        let signed_in = format!(
            "{profile:?} {session:?} {:?} {seen:?}",
            session.installed_addons()
        );
        api.login_error.store(1, Ordering::Relaxed);
        let error = session
            .sign_in(EMAIL.into(), PASSWORD.into())
            .await
            .unwrap_err();
        assert_eq!(error.kind, CoreErrorKind::WrongCredentials);
        for output in [
            signed_in,
            format!("{error:?} {error} {:?}", changes.try_recv()),
        ] {
            for secret in [
                AUTH_KEY,
                EMAIL,
                PASSWORD,
                "mock-secret",
                "secret-path",
                "mock-token",
                "https://",
            ] {
                assert!(!output.contains(secret));
            }
        }
    });
}

#[test]
fn network_failure_is_sanitized_and_session_can_sign_in_again() {
    let api = MockApi::start();
    api.network_error.store(true, Ordering::Relaxed);
    let fixture = TestEnv::memory(Some(api.base.clone()));
    fixture.runtime.block_on(async {
        let mut session = CoreSession::start().await.unwrap();
        assert_eq!(
            session
                .sign_in(EMAIL.into(), PASSWORD.into())
                .await
                .unwrap_err()
                .kind,
            CoreErrorKind::Network
        );
        api.network_error.store(false, Ordering::Relaxed);
        session
            .sign_in(EMAIL.into(), PASSWORD.into())
            .await
            .unwrap();
        assert!(session.is_signed_in());
    });
}

#[test]
fn rehydration_loads_all_ctx_buckets_and_merges_library_by_uid_and_mtime() {
    let fixture = TestEnv::memory(None);
    fixture.runtime.block_on(async {
        let mut ctx = model::defaults();
        ctx.profile.addons = vec![serde_json::from_value(addon("saved")).unwrap()];
        ctx.search_history
            .items
            .insert("persisted search".into(), PanoramaEnv::now());
        ctx.streaming_server_urls.items.clear();
        ctx.streaming_server_urls
            .add_url::<PanoramaEnv>("https://saved.example.test/".parse().unwrap());
        ctx.notifications.last_updated = Some(PanoramaEnv::now());
        let recent = LibraryBucket::new(None, vec![item("overlap", "new", "2025-02-01T00:00:00Z")]);
        let older = LibraryBucket::new(
            None,
            vec![
                item("overlap", "old", "2025-01-01T00:00:00Z"),
                item("older", "older", "2025-01-01T00:00:00Z"),
            ],
        );
        PanoramaEnv::set_storage(SCHEMA_VERSION_STORAGE_KEY, Some(&SCHEMA_VERSION))
            .await
            .unwrap();
        PanoramaEnv::set_storage(PROFILE_STORAGE_KEY, Some(&ctx.profile))
            .await
            .unwrap();
        PanoramaEnv::set_storage(LIBRARY_RECENT_STORAGE_KEY, Some(&recent))
            .await
            .unwrap();
        PanoramaEnv::set_storage(LIBRARY_STORAGE_KEY, Some(&older))
            .await
            .unwrap();
        PanoramaEnv::set_storage(STREAMS_STORAGE_KEY, Some(&ctx.streams))
            .await
            .unwrap();
        PanoramaEnv::set_storage(
            STREAMING_SERVER_URLS_STORAGE_KEY,
            Some(&ctx.streaming_server_urls),
        )
        .await
        .unwrap();
        PanoramaEnv::set_storage(NOTIFICATIONS_STORAGE_KEY, Some(&ctx.notifications))
            .await
            .unwrap();
        PanoramaEnv::set_storage(SEARCH_HISTORY_STORAGE_KEY, Some(&ctx.search_history))
            .await
            .unwrap();
        PanoramaEnv::set_storage(DISMISSED_EVENTS_STORAGE_KEY, Some(&ctx.dismissed_events))
            .await
            .unwrap();
        let loaded = model::rehydrate().await.unwrap();
        assert_eq!(loaded.profile, ctx.profile);
        assert_eq!(loaded.library.items.len(), 2);
        assert_eq!(loaded.library.items["overlap"].name, "new");
        assert_eq!(loaded.streams, ctx.streams);
        assert_eq!(loaded.streaming_server_urls, ctx.streaming_server_urls);
        assert_eq!(loaded.notifications, ctx.notifications);
        assert_eq!(loaded.search_history, ctx.search_history);
        assert_eq!(loaded.dismissed_events, ctx.dismissed_events);
        PanoramaEnv::set_storage(
            LIBRARY_STORAGE_KEY,
            Some(&LibraryBucket::new(
                Some(stremio_core::types::profile::UserId("other-user".into())),
                vec![item("foreign", "foreign", "2025-01-01T00:00:00Z")],
            )),
        )
        .await
        .unwrap();
        assert!(
            !model::rehydrate()
                .await
                .unwrap()
                .library
                .items
                .contains_key("foreign")
        );
    });
}

fn item(id: &str, name: &str, mtime: &str) -> LibraryItem {
    serde_json::from_value(
        json!({"_id": id, "name": name, "type": "movie", "removed": false,
        "temp": false, "_mtime": mtime, "state": LibraryItemState::default()}),
    )
    .unwrap()
}

#[test]
#[ignore = "requires real account credentials and public network access"]
fn real_sign_in_from_environment_without_printing_credentials() {
    let fixture = TestEnv::memory(None);
    fixture.runtime.block_on(async {
        let email = std::env::var("PANORAMA_STREMIO_EMAIL")
            .unwrap_or_else(|_| panic!("email variable required"));
        let password = std::env::var("PANORAMA_STREMIO_PASSWORD")
            .unwrap_or_else(|_| panic!("password variable required"));
        let mut session = CoreSession::start().await.unwrap();
        session.sign_in(email, password).await.unwrap();
        assert!(session.is_signed_in());
        session.sign_out().await.unwrap();
    });
}
