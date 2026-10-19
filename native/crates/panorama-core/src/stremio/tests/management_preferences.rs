use std::sync::atomic::Ordering;

use stremio_core::{constants::PROFILE_STORAGE_KEY, runtime::Env, types::profile::Profile};

use super::{
    TestEnv,
    manage::{body, server},
    mock::{EMAIL, MockApi, PASSWORD},
};
use crate::{
    addons::{AddonClient, CoreAddonTransport, manage::InstallError},
    store::Key,
    stremio::{CoreSession, env::PanoramaEnv},
};

#[test]
fn removed_meta_source_falls_back_to_automatic() {
    let api = MockApi::start();
    let fixture = TestEnv::memory(Some(api.base.clone()));
    fixture.runtime.block_on(async {
        let mut session = CoreSession::start().await.unwrap();
        session
            .sign_in(EMAIL.into(), PASSWORD.into())
            .await
            .unwrap();
        let key = session
            .addon_key(&session.installed_addons()[0])
            .await
            .unwrap();
        session.set_meta_source(Some(key.clone())).await.unwrap();
        assert_eq!(session.meta_source().await.unwrap(), Some(key.clone()));
        session.remove(key).await.unwrap();
        assert_eq!(session.meta_source().await.unwrap(), None);
        assert!(
            fixture
                .state
                .store
                .get(&Key::pref("metaAddon").unwrap())
                .unwrap()
                .is_none()
        );
    });
}

#[test]
fn removed_home_catalog_falls_back_to_first() {
    let api = MockApi::start();
    let fixture = TestEnv::memory(Some(api.base.clone()));
    fixture.runtime.block_on(async {
        let mut session = CoreSession::start().await.unwrap();
        session
            .sign_in(EMAIL.into(), PASSWORD.into())
            .await
            .unwrap();
        let client = AddonClient::new(fixture.state.store.clone(), CoreAddonTransport)
            .await
            .unwrap();
        let catalogs = client.catalogs(&session.installed_addons());
        session
            .set_home_catalog(Some(catalogs[0].clone()))
            .await
            .unwrap();
        assert_eq!(
            session.home_catalog().await.unwrap(),
            Some(catalogs[0].clone())
        );
        let removed = catalogs[0].addon.clone();
        let doomed = Key::catalog(removed.as_str(), "movies").unwrap();
        let other = Key::catalog(catalogs[1].addon.as_str(), "movies").unwrap();
        fixture.state.store.set(&doomed, b"cache").unwrap();
        fixture.state.store.set(&other, b"other").unwrap();
        let generation = fixture.state.store.cache_generation();
        session.remove(removed).await.unwrap();
        assert_eq!(
            session.home_catalog().await.unwrap(),
            Some(catalogs[1].clone())
        );
        assert!(
            fixture
                .state
                .store
                .get(&Key::pref("homeCatalog").unwrap())
                .unwrap()
                .is_none()
        );
        assert!(fixture.state.store.get(&doomed).unwrap().is_none());
        assert_eq!(fixture.state.store.get(&other).unwrap().unwrap(), b"other");
        fixture
            .state
            .store
            .set_if_generation(&doomed, b"late", generation)
            .unwrap();
        assert!(fixture.state.store.get(&doomed).unwrap().is_none());
    });
}

#[test]
fn removal_push_failure_still_clears_cache_and_preferences() {
    let api = MockApi::start();
    let fixture = TestEnv::memory(Some(api.base.clone()));
    fixture.runtime.block_on(async {
        let mut session = CoreSession::start().await.unwrap();
        session
            .sign_in(EMAIL.into(), PASSWORD.into())
            .await
            .unwrap();
        let selected = session.home_catalog().await.unwrap().unwrap();
        session
            .set_meta_source(Some(selected.addon.clone()))
            .await
            .unwrap();
        session
            .set_home_catalog(Some(selected.clone()))
            .await
            .unwrap();
        let key = Key::catalog(selected.addon.as_str(), &selected.catalog_id).unwrap();
        fixture.state.store.set(&key, b"cache").unwrap();
        api.reject_push.store(true, Ordering::Relaxed);
        assert_eq!(
            session.remove(selected.addon).await.unwrap_err(),
            InstallError::ApiPush
        );
        assert_eq!(session.installed_addons().len(), 1);
        assert_eq!(session.meta_source().await.unwrap(), None);
        assert_eq!(
            session.home_catalog().await.unwrap().unwrap().name,
            "movies"
        );
        assert!(fixture.state.store.get(&key).unwrap().is_none());
    });
}

#[test]
fn stale_preferences_fall_back_after_external_collection_change() {
    let api = MockApi::start();
    let fixture = TestEnv::memory(Some(api.base.clone()));
    fixture.runtime.block_on(async {
        let mut session = CoreSession::start().await.unwrap();
        session
            .sign_in(EMAIL.into(), PASSWORD.into())
            .await
            .unwrap();
        let selected = session.home_catalog().await.unwrap().unwrap();
        session
            .set_meta_source(Some(selected.addon.clone()))
            .await
            .unwrap();
        session.set_home_catalog(Some(selected)).await.unwrap();
        let mut profile = session.profile().as_core().clone();
        profile.addons.remove(0);
        drop(session);
        drop(fixture.state.session_gate.lock().await);
        PanoramaEnv::set_storage(PROFILE_STORAGE_KEY, Some(&profile))
            .await
            .unwrap();
        let mut session = CoreSession::start().await.unwrap();
        assert_eq!(session.meta_source().await.unwrap(), None);
        assert_eq!(
            session.home_catalog().await.unwrap().unwrap().addon,
            session
                .addon_key(&session.installed_addons()[0])
                .await
                .unwrap()
        );
    });
}

#[test]
fn preference_selection_rejects_missing_catalogs_and_non_movie_meta() {
    let api = MockApi::start();
    let mut fixture = TestEnv::memory(Some(api.base.clone()));
    let (server, url) = server(&mut fixture);
    fixture.runtime.block_on(async {
        let mut session = CoreSession::start().await.unwrap();
        session
            .sign_in(EMAIL.into(), PASSWORD.into())
            .await
            .unwrap();
        let mut value = body("stream-only");
        value["resources"] = serde_json::json!(["stream", {"name":"meta", "types":["series"]}]);
        *server.body.lock().unwrap() = serde_json::to_vec(&value).unwrap();
        let preview = session.preview(url).await.unwrap();
        session.install(preview.token).await.unwrap();
        let key = session
            .addon_key(session.installed_addons().last().unwrap())
            .await
            .unwrap();
        assert_eq!(
            session.set_meta_source(Some(key)).await.unwrap_err(),
            InstallError::InvalidSelection
        );
        let mut catalog = session.home_catalog().await.unwrap().unwrap();
        catalog.catalog_id = "gone".into();
        assert_eq!(
            session.set_home_catalog(Some(catalog)).await.unwrap_err(),
            InstallError::InvalidSelection
        );
        session.set_meta_source(None).await.unwrap();
        session.set_home_catalog(None).await.unwrap();
    });
}

#[test]
fn protected_reorder_allowed_official_unprotected_remove_allowed() {
    let api = MockApi::start();
    let fixture = TestEnv::memory(Some(api.base.clone()));
    fixture.runtime.block_on(async {
        let mut session = CoreSession::start().await.unwrap();
        session
            .sign_in(EMAIL.into(), PASSWORD.into())
            .await
            .unwrap();
        let mut profile = session.profile().as_core().clone();
        profile.addons[0].flags.protected = true;
        profile.addons[1].flags.official = true;
        drop(session);
        drop(fixture.state.session_gate.lock().await);
        PanoramaEnv::set_storage(PROFILE_STORAGE_KEY, Some(&profile))
            .await
            .unwrap();
        let mut session = CoreSession::start().await.unwrap();
        let addons = session.installed_addons();
        let protected = session.addon_key(&addons[0]).await.unwrap();
        let official = session.addon_key(&addons[1]).await.unwrap();
        session
            .reorder(vec![official.clone(), protected.clone()])
            .await
            .unwrap();
        assert_eq!(
            session.remove(protected).await.unwrap_err(),
            InstallError::Protected
        );
        session.remove(official).await.unwrap();
        let profile = PanoramaEnv::get_storage::<Profile>(PROFILE_STORAGE_KEY)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(profile.addons.len(), 1);
        assert!(profile.addons[0].flags.protected);
    });
}
