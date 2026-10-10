use stremio_core::{constants::PROFILE_STORAGE_KEY, runtime::Env};

use super::{
    TestEnv,
    manage::{body, server},
    mock::{EMAIL, MockApi, PASSWORD},
};
use crate::{
    store::Key,
    stremio::{CoreSession, Descriptor, env::PanoramaEnv},
};

async fn configured_session(fixture: &TestEnv, urls: &[(&str, &str)]) -> CoreSession {
    let mut session = CoreSession::start().await.unwrap();
    session
        .sign_in(EMAIL.into(), PASSWORD.into())
        .await
        .unwrap();
    let mut profile = session.profile().as_core().clone();
    profile.addons = urls
        .iter()
        .map(|(id, url)| {
            let mut addon = profile.addons[0].clone();
            addon.manifest = serde_json::from_value(body(id)).unwrap();
            addon.transport_url = url.parse().unwrap();
            addon
        })
        .collect();
    drop(session);
    drop(fixture.state.session_gate.lock().await);
    PanoramaEnv::set_storage(PROFILE_STORAGE_KEY, Some(&profile))
        .await
        .unwrap();
    CoreSession::start().await.unwrap()
}

fn assert_addons(actual: &[Descriptor], expected: &[stremio_core::types::addon::Descriptor]) {
    assert_eq!(
        actual.iter().map(Descriptor::as_core).collect::<Vec<_>>(),
        expected.iter().collect::<Vec<_>>()
    );
}

#[test]
fn reinstall_shared_id_updates_matching_transport_and_only_its_cache_after_restart() {
    let api = MockApi::start();
    let mut fixture = TestEnv::memory(Some(api.base.clone()));
    let (server, url) = server(&mut fixture);
    fixture.runtime.block_on(async {
        let mut session = configured_session(
            &fixture,
            &[
                ("X", "https://addon.test/u1/manifest.json"),
                ("X", url.as_str()),
            ],
        )
        .await;
        let before = session.installed_addons();
        let first = session.addon_key(&before[0]).await.unwrap();
        let second = session.addon_key(&before[1]).await.unwrap();
        let kept = Key::catalog(first.as_str(), "movies").unwrap();
        let deleted = Key::catalog(second.as_str(), "movies").unwrap();
        fixture.state.store.set(&kept, b"first").unwrap();
        fixture.state.store.set(&deleted, b"second").unwrap();
        *server.body.lock().unwrap() = serde_json::to_vec(&body("X")).unwrap();
        let unchanged = session.preview(url.clone()).await.unwrap();
        assert!(unchanged.already_installed);
        assert_eq!(unchanged.replaces, Some(second.clone()));
        let unchanged_expected = before
            .iter()
            .map(|addon| addon.as_core().clone())
            .collect::<Vec<_>>();
        assert_addons(
            &session.install(unchanged.token).await.unwrap(),
            &unchanged_expected,
        );
        assert_eq!(fixture.state.store.get(&kept).unwrap().unwrap(), b"first");
        assert_eq!(
            fixture.state.store.get(&deleted).unwrap().unwrap(),
            b"second"
        );
        drop(session);
        let mut session = CoreSession::start().await.unwrap();
        assert_addons(&session.installed_addons(), &unchanged_expected);
        let mut manifest = body("X");
        manifest["version"] = serde_json::json!("2.0.0");
        *server.body.lock().unwrap() = serde_json::to_vec(&manifest).unwrap();
        let preview = session.preview(url).await.unwrap();
        assert!(preview.already_installed);
        assert_eq!(preview.replaces, Some(second));
        let mut expected = before
            .iter()
            .map(|addon| addon.as_core().clone())
            .collect::<Vec<_>>();
        expected[1].manifest = serde_json::from_value(manifest).unwrap();
        assert_addons(&session.install(preview.token).await.unwrap(), &expected);
        assert_eq!(fixture.state.store.get(&kept).unwrap().unwrap(), b"first");
        assert!(fixture.state.store.get(&deleted).unwrap().is_none());
        drop(session);
        let session = CoreSession::start().await.unwrap();
        assert_addons(&session.installed_addons(), &expected);
        assert_eq!(fixture.state.store.get(&kept).unwrap().unwrap(), b"first");
        assert!(fixture.state.store.get(&deleted).unwrap().is_none());
    });
}

#[test]
fn third_transport_with_shared_id_adds_installation_and_keeps_existing_caches_after_restart() {
    let api = MockApi::start();
    let mut fixture = TestEnv::memory(Some(api.base.clone()));
    let (server, url) = server(&mut fixture);
    fixture.runtime.block_on(async {
        let mut session = configured_session(
            &fixture,
            &[
                ("X", "https://addon.test/u1/manifest.json"),
                ("X", "https://addon.test/u2/manifest.json"),
            ],
        )
        .await;
        let before = session.installed_addons();
        let first = session.addon_key(&before[0]).await.unwrap();
        let second = session.addon_key(&before[1]).await.unwrap();
        let kept_first = Key::catalog(first.as_str(), "movies").unwrap();
        let kept_second = Key::catalog(second.as_str(), "movies").unwrap();
        fixture.state.store.set(&kept_first, b"first").unwrap();
        fixture.state.store.set(&kept_second, b"second").unwrap();
        *server.body.lock().unwrap() = serde_json::to_vec(&body("X")).unwrap();
        let preview = session.preview(url.clone()).await.unwrap();
        assert!(!preview.already_installed);
        assert!(preview.replaces.is_none());
        let mut expected = before
            .iter()
            .map(|addon| addon.as_core().clone())
            .collect::<Vec<_>>();
        let mut additional = expected[0].clone();
        additional.transport_url = url.as_str().parse().unwrap();
        expected.push(additional);
        assert_addons(&session.install(preview.token).await.unwrap(), &expected);
        assert_eq!(
            fixture.state.store.get(&kept_first).unwrap().unwrap(),
            b"first"
        );
        assert_eq!(
            fixture.state.store.get(&kept_second).unwrap().unwrap(),
            b"second"
        );
        drop(session);
        let session = CoreSession::start().await.unwrap();
        assert_addons(&session.installed_addons(), &expected);
        assert_eq!(
            fixture.state.store.get(&kept_first).unwrap().unwrap(),
            b"first"
        );
        assert_eq!(
            fixture.state.store.get(&kept_second).unwrap().unwrap(),
            b"second"
        );
    });
}

#[test]
fn protected_transport_collision_is_rejected_and_reorder_keeps_descriptors_caches_and_restart() {
    let api = MockApi::start();
    let mut fixture = TestEnv::memory(Some(api.base.clone()));
    let (server, url) = server(&mut fixture);
    fixture.runtime.block_on(async {
        let session = configured_session(
            &fixture,
            &[
                ("X", "https://addon.test/u1/manifest.json"),
                ("Y", url.as_str()),
            ],
        )
        .await;
        let mut profile = session.profile().as_core().clone();
        profile.addons[1].flags.protected = true;
        profile.addons[1].flags.official = true;
        drop(session);
        drop(fixture.state.session_gate.lock().await);
        PanoramaEnv::set_storage(PROFILE_STORAGE_KEY, Some(&profile))
            .await
            .unwrap();
        let mut session = CoreSession::start().await.unwrap();
        let before = session.installed_addons();
        let first = session.addon_key(&before[0]).await.unwrap();
        let second = session.addon_key(&before[1]).await.unwrap();
        let kept_first = Key::catalog(first.as_str(), "movies").unwrap();
        let kept_second = Key::catalog(second.as_str(), "movies").unwrap();
        fixture.state.store.set(&kept_first, b"first").unwrap();
        fixture.state.store.set(&kept_second, b"second").unwrap();
        *server.body.lock().unwrap() = serde_json::to_vec(&body("X")).unwrap();
        let preview = session.preview(url).await.unwrap();
        assert!(preview.already_installed);
        assert_eq!(preview.replaces, Some(second.clone()));
        assert_eq!(
            session.install(preview.token).await.unwrap_err(),
            crate::addons::manage::InstallError::Protected
        );
        assert_addons(&session.installed_addons(), &profile.addons);
        let expected = vec![profile.addons[1].clone(), profile.addons[0].clone()];
        assert_addons(
            &session.reorder(vec![second, first]).await.unwrap(),
            &expected,
        );
        assert_eq!(
            fixture.state.store.get(&kept_first).unwrap().unwrap(),
            b"first"
        );
        assert_eq!(
            fixture.state.store.get(&kept_second).unwrap().unwrap(),
            b"second"
        );
        drop(session);
        let session = CoreSession::start().await.unwrap();
        assert_addons(&session.installed_addons(), &expected);
        assert_eq!(
            fixture.state.store.get(&kept_first).unwrap().unwrap(),
            b"first"
        );
        assert_eq!(
            fixture.state.store.get(&kept_second).unwrap().unwrap(),
            b"second"
        );
    });
}

#[test]
fn existing_transport_cannot_claim_another_protected_manifest_id() {
    let api = MockApi::start();
    let mut fixture = TestEnv::memory(Some(api.base.clone()));
    let (server, url) = server(&mut fixture);
    fixture.runtime.block_on(async {
        let session = configured_session(
            &fixture,
            &[
                ("X", "https://addon.test/u1/manifest.json"),
                ("Y", url.as_str()),
            ],
        )
        .await;
        let mut profile = session.profile().as_core().clone();
        profile.addons[0].flags.protected = true;
        drop(session);
        drop(fixture.state.session_gate.lock().await);
        PanoramaEnv::set_storage(PROFILE_STORAGE_KEY, Some(&profile))
            .await
            .unwrap();
        let mut session = CoreSession::start().await.unwrap();
        *server.body.lock().unwrap() = serde_json::to_vec(&body("X")).unwrap();
        assert_eq!(
            session.preview(url).await.unwrap_err(),
            crate::addons::manage::InstallError::ImpersonatesProtected
        );
        assert_addons(&session.installed_addons(), &profile.addons);
    });
}

#[test]
fn confirmation_does_not_retarget_after_a_shared_id_installation_is_removed() {
    let api = MockApi::start();
    let mut fixture = TestEnv::memory(Some(api.base.clone()));
    let (server, url) = server(&mut fixture);
    fixture.runtime.block_on(async {
        let mut session = configured_session(
            &fixture,
            &[
                ("X", "https://addon.test/u1/manifest.json"),
                ("X", "https://addon.test/u2/manifest.json"),
            ],
        )
        .await;
        let before = session.installed_addons();
        let first = session.addon_key(&before[0]).await.unwrap();
        let second = session.addon_key(&before[1]).await.unwrap();
        let kept = Key::catalog(first.as_str(), "movies").unwrap();
        fixture.state.store.set(&kept, b"first").unwrap();
        *server.body.lock().unwrap() = serde_json::to_vec(&body("X")).unwrap();
        let preview = session.preview(url).await.unwrap();
        assert!(preview.replaces.is_none());
        session.remove(second).await.unwrap();
        assert_eq!(
            session.install(preview.token).await.unwrap_err(),
            crate::addons::manage::InstallError::InvalidToken
        );
        let expected = vec![before[0].as_core().clone()];
        assert_addons(&session.installed_addons(), &expected);
        assert_eq!(fixture.state.store.get(&kept).unwrap().unwrap(), b"first");
        drop(session);
        let session = CoreSession::start().await.unwrap();
        assert_addons(&session.installed_addons(), &expected);
        assert_eq!(fixture.state.store.get(&kept).unwrap().unwrap(), b"first");
    });
}
