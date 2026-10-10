use std::sync::atomic::Ordering;

use serde_json::{Value, json};
use stremio_core::{constants::PROFILE_STORAGE_KEY, runtime::Env};

use super::{
    TestEnv,
    mock::{EMAIL, MockApi, PASSWORD},
    tls::TlsMock,
};
use crate::{
    addons::manage::{AddonUrl, InstallError, PreviewToken},
    stremio::{CoreSession, env::PanoramaEnv},
};

pub(super) fn body(id: &str) -> Value {
    json!({"id":id, "version":"1.2.3", "name":"SECRET name", "description":"SECRET description",
        "types":["movie"], "resources":["catalog", "meta"],
        "catalogs":[{"id":"movies", "type":"movie", "name":"Movies"}],
        "behaviorHints":{"configurable":true}})
}

pub(super) fn server(fixture: &mut TestEnv) -> (TlsMock, AddonUrl) {
    let server = fixture.runtime.block_on(TlsMock::start());
    let mut base = server.base.clone();
    base.set_host(Some("addon.test")).unwrap();
    let client = crate::stremio::fetch::client_builder(fixture.state.api_base.as_ref())
        .unwrap()
        .add_root_certificate(server.certificate.clone())
        .no_proxy()
        .resolve("addon.test", server.base.socket_addrs(|| None).unwrap()[0])
        .build()
        .unwrap();
    PanoramaEnv::swap_client_for_test(client, &fixture.guard);
    fixture.state = PanoramaEnv::state().unwrap();
    *server.body.lock().unwrap() = serde_json::to_vec(&body("new-addon")).unwrap();
    let url = AddonUrl::parse(
        base.join("secret-path/manifest.json?token=SECRET")
            .unwrap()
            .as_str(),
    )
    .unwrap();
    (server, url)
}

#[test]
fn preview_install_binds_manifest_and_pushes_without_refetch() {
    let api = MockApi::start();
    let mut fixture = TestEnv::memory(Some(api.base.clone()));
    let (server, url) = server(&mut fixture);
    fixture.runtime.block_on(async {
        let mut session = CoreSession::start().await.unwrap();
        session
            .sign_in(EMAIL.into(), PASSWORD.into())
            .await
            .unwrap();
        let preview = session.preview(url).await.unwrap();
        assert!(!preview.already_installed);
        assert!(preview.replaces.is_none());
        assert_eq!(preview.resources, ["catalog", "meta"]);
        assert_eq!(preview.types, ["movie"]);
        assert_eq!(preview.catalog_names, ["Movies"]);
        for secret in ["secret-path", "SECRET", "token=", "https://"] {
            assert!(!format!("{preview:?}").contains(secret));
        }
        *server.body.lock().unwrap() = serde_json::to_vec(&body("server-swapped")).unwrap();
        let installed = session.install(preview.token).await.unwrap();
        assert_eq!(installed.last().unwrap().as_core().manifest.id, "new-addon");
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        assert!(
            !server.requests.lock().unwrap()[0]
                .to_lowercase()
                .contains("referer:")
        );
        assert_eq!(
            api.pushed.lock().unwrap().last().unwrap()[2]["manifest"]["id"],
            "new-addon"
        );
        let key = session.addon_key(installed.last().unwrap()).await.unwrap();
        let configure = session.configure_url(&key).await.unwrap();
        assert_eq!(configure.path(), "/secret-path/configure");
        assert!(configure.query().is_none());
        assert_eq!(configure.scheme(), "https");
    });
}

#[test]
fn preview_limits_declared_streamed_json_shape_and_redirects() {
    let api = MockApi::start();
    let mut fixture = TestEnv::memory(Some(api.base.clone()));
    let (server, url) = server(&mut fixture);
    fixture.runtime.block_on(async {
        let mut session = CoreSession::start().await.unwrap();
        session
            .sign_in(EMAIL.into(), PASSWORD.into())
            .await
            .unwrap();
        let mut boundary = serde_json::to_vec(&body("boundary")).unwrap();
        boundary.resize(256 * 1024, b' ');
        *server.body.lock().unwrap() = boundary;
        assert!(session.preview(url.clone()).await.is_ok());
        for chunked in [false, true] {
            server.chunked.store(chunked, Ordering::Relaxed);
            *server.body.lock().unwrap() = vec![b' '; 256 * 1024 + 1];
            assert_eq!(
                session.preview(url.clone()).await.unwrap_err(),
                InstallError::TooLarge
            );
        }
        server.chunked.store(false, Ordering::Relaxed);
        for value in [
            json!({}),
            json!({"id":"missing"}),
            json!({"id":"bad","version":"bogus"}),
        ] {
            *server.body.lock().unwrap() = serde_json::to_vec(&value).unwrap();
            assert_eq!(
                session.preview(url.clone()).await.unwrap_err(),
                InstallError::InvalidManifest
            );
        }
        *server.body.lock().unwrap() = b"{invalid SECRET".to_vec();
        assert_eq!(
            session.preview(url.clone()).await.unwrap_err(),
            InstallError::InvalidManifest
        );
        let mut value = body("new-addon");
        value["catalogs"] = json!(
            (0..201)
                .map(|i| json!({"id":i.to_string(),"type":"movie"}))
                .collect::<Vec<_>>()
        );
        *server.body.lock().unwrap() = serde_json::to_vec(&value).unwrap();
        assert_eq!(
            session.preview(url.clone()).await.unwrap_err(),
            InstallError::InvalidManifest
        );
        value = body("new-addon");
        value["logo"] = json!("http://addon.test/SECRET");
        *server.body.lock().unwrap() = serde_json::to_vec(&value).unwrap();
        assert!(session.preview(url.clone()).await.unwrap().logo.is_none());
        *server.redirect.lock().unwrap() = Some("http://127.0.0.1/SECRET".parse().unwrap());
        assert_eq!(
            session.preview(url.clone()).await.unwrap_err(),
            InstallError::Network
        );
        *server.redirect.lock().unwrap() = Some(server.base.join("SECRET").unwrap());
        assert_eq!(
            session.preview(url).await.unwrap_err(),
            InstallError::Network
        );
    });
}

#[test]
fn signed_out_management_is_read_only_and_tokens_cannot_be_forged() {
    let fixture = TestEnv::memory(None);
    fixture.runtime.block_on(async {
        let mut session = CoreSession::start().await.unwrap();
        assert_eq!(
            session
                .preview(AddonUrl::parse("https://addon.test/manifest.json").unwrap())
                .await
                .unwrap_err(),
            InstallError::SignedOut
        );
        assert_eq!(
            session.install(PreviewToken([0; 32])).await.unwrap_err(),
            InstallError::SignedOut
        );
        assert_eq!(
            session.reorder(vec![]).await.unwrap_err(),
            InstallError::SignedOut
        );
        let key = session
            .addon_key(&session.installed_addons()[0])
            .await
            .unwrap();
        assert_eq!(
            session.remove(key.clone()).await.unwrap_err(),
            InstallError::SignedOut
        );
        assert_eq!(
            session.set_meta_source(Some(key)).await.unwrap_err(),
            InstallError::SignedOut
        );
        assert_eq!(
            session.set_home_catalog(None).await.unwrap_err(),
            InstallError::SignedOut
        );
    });
}

#[test]
fn confirmation_expires_is_single_use_and_rejects_forgery() {
    let api = MockApi::start();
    let guard = crate::stremio::env::TEST_MUTEX
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    PanoramaEnv::swap_for_test(
        crate::stremio::env::EnvConfig {
            store: std::sync::Arc::new(crate::store::Store::open_in_memory().unwrap()),
            api_base: Some(api.base.clone()),
            runtime: runtime.handle().clone(),
        },
        &guard,
    );
    let mut fixture = TestEnv {
        runtime,
        state: PanoramaEnv::state().unwrap(),
        guard,
    };
    let (_server, url) = server(&mut fixture);
    fixture.runtime.block_on(async {
        let mut session = CoreSession::start().await.unwrap();
        session
            .sign_in(EMAIL.into(), PASSWORD.into())
            .await
            .unwrap();
        assert_eq!(
            session.install(PreviewToken([0; 32])).await.unwrap_err(),
            InstallError::InvalidToken
        );
        let preview = session.preview(url.clone()).await.unwrap();
        let replay = PreviewToken(preview.token.0);
        session.install(preview.token).await.unwrap();
        assert_eq!(
            session.install(replay).await.unwrap_err(),
            InstallError::InvalidToken
        );
        let expired = session.preview(url.clone()).await.unwrap();
        tokio::time::pause();
        tokio::time::advance(std::time::Duration::from_secs(600)).await;
        assert_eq!(
            session.install(expired.token).await.unwrap_err(),
            InstallError::InvalidToken
        );
        tokio::time::resume();
        let foreign = session.preview(url.clone()).await.unwrap();
        drop(session);
        let mut session = CoreSession::start().await.unwrap();
        assert_eq!(
            session.install(foreign.token).await.unwrap_err(),
            InstallError::InvalidToken
        );
        let tampered = session.preview(url).await.unwrap();
        let saved = session
            .management
            .tokens
            .get_mut(&tampered.token.0)
            .unwrap();
        let mut core = saved.descriptor.as_core().clone();
        core.manifest.name = "tampered".into();
        saved.descriptor = crate::stremio::Descriptor::from_core(core);
        assert_eq!(
            session.install(tampered.token).await.unwrap_err(),
            InstallError::InvalidToken
        );
    });
}

#[test]
fn same_id_replaces_in_position_and_protected_official_impersonation_is_rejected() {
    let api = MockApi::start();
    let mut fixture = TestEnv::memory(Some(api.base.clone()));
    let (server, url) = server(&mut fixture);
    fixture.runtime.block_on(async {
        let mut session = CoreSession::start().await.unwrap();
        session
            .sign_in(EMAIL.into(), PASSWORD.into())
            .await
            .unwrap();
        let old = session.installed_addons()[0].clone();
        let old_key = session.addon_key(&old).await.unwrap();
        *server.body.lock().unwrap() = serde_json::to_vec(&body("second")).unwrap();
        let preview = session.preview(url.clone()).await.unwrap();
        assert_eq!(preview.replaces, Some(old_key));
        let installed = session.install(preview.token).await.unwrap();
        assert_eq!(installed.len(), 2);
        assert_eq!(installed[0].as_core().manifest.id, "second");
        assert_eq!(installed[0].as_core().transport_url.as_str(), url.as_str());
        assert_eq!(
            api.pushed.lock().unwrap().last().unwrap()[0]["manifest"]["id"],
            "second"
        );
    });
    fixture.idle();
    for official in [false, true] {
        fixture.runtime.block_on(async {
            let mut profile = PanoramaEnv::get_storage::<stremio_core::types::profile::Profile>(
                PROFILE_STORAGE_KEY,
            )
            .await
            .unwrap()
            .unwrap();
            profile.addons[0].flags.protected = !official;
            profile.addons[0].flags.official = official;
            profile.addons[0].transport_url = "https://trusted.test/manifest.json".parse().unwrap();
            PanoramaEnv::set_storage(PROFILE_STORAGE_KEY, Some(&profile))
                .await
                .unwrap();
            let mut session = CoreSession::start().await.unwrap();
            assert_eq!(
                session.preview(url.clone()).await.unwrap_err(),
                InstallError::ImpersonatesProtected
            );
            if !official {
                let key = session
                    .addon_key(&session.installed_addons()[0])
                    .await
                    .unwrap();
                assert_eq!(
                    session.remove(key).await.unwrap_err(),
                    InstallError::Protected
                );
            }
        });
        fixture.idle();
    }
}

#[test]
fn push_failure_is_sanitized_and_core_keeps_local_install() {
    let api = MockApi::start();
    let mut fixture = TestEnv::memory(Some(api.base.clone()));
    let (_server, url) = server(&mut fixture);
    fixture.runtime.block_on(async {
        let mut session = CoreSession::start().await.unwrap();
        session
            .sign_in(EMAIL.into(), PASSWORD.into())
            .await
            .unwrap();
        let preview = session.preview(url.clone()).await.unwrap();
        api.reject_push.store(true, Ordering::Relaxed);
        let error = session.install(preview.token).await.unwrap_err();
        assert_eq!(error, InstallError::ApiPush);
        assert_eq!(session.installed_addons().len(), 3);
        let saved =
            PanoramaEnv::get_storage::<stremio_core::types::profile::Profile>(PROFILE_STORAGE_KEY)
                .await
                .unwrap()
                .unwrap();
        assert_eq!(saved.addons.len(), 3);
        assert!(!format!("{error:?} {error}").contains("SECRET"));
        api.reject_push.store(false, Ordering::Relaxed);
        let retry = session.preview(url).await.unwrap();
        assert!(retry.already_installed);
        session.install(retry.token).await.unwrap();
        assert_eq!(api.pushed.lock().unwrap().len(), 2);
    });
}

#[test]
fn reorder_checks_permutation_and_persists_across_session_restart() {
    let api = MockApi::start();
    let fixture = TestEnv::memory(Some(api.base.clone()));
    fixture.runtime.block_on(async {
        let mut session = CoreSession::start().await.unwrap();
        session
            .sign_in(EMAIL.into(), PASSWORD.into())
            .await
            .unwrap();
        let addons = session.installed_addons();
        let first = session.addon_key(&addons[0]).await.unwrap();
        let second = session.addon_key(&addons[1]).await.unwrap();
        for order in [
            vec![],
            vec![first.clone()],
            vec![first.clone(), first.clone()],
        ] {
            assert_eq!(
                session.reorder(order).await.unwrap_err(),
                InstallError::InvalidOrder
            );
        }
        session.reorder(vec![second, first]).await.unwrap();
        assert_eq!(
            api.pushed.lock().unwrap().last().unwrap()[0]["manifest"]["id"],
            "first"
        );
        drop(session);
        let session = CoreSession::start().await.unwrap();
        assert_eq!(session.installed_addons()[0].as_core().manifest.id, "first");
        assert_eq!(
            session.installed_addons()[1].as_core().manifest.id,
            "second"
        );
    });
}

#[test]
fn configuration_required_is_not_installed_and_same_url_upgrade_preserves_flags() {
    let api = MockApi::start();
    let mut fixture = TestEnv::memory(Some(api.base.clone()));
    let (server, url) = server(&mut fixture);
    fixture.runtime.block_on(async {
        let mut session = CoreSession::start().await.unwrap();
        session
            .sign_in(EMAIL.into(), PASSWORD.into())
            .await
            .unwrap();
        let mut manifest = body("new-addon");
        manifest["behaviorHints"]["configurationRequired"] = json!(true);
        *server.body.lock().unwrap() = serde_json::to_vec(&manifest).unwrap();
        let preview = session.preview(url.clone()).await.unwrap();
        assert!(preview.configuration_required);
        assert_eq!(
            session.install(preview.token).await.unwrap_err(),
            InstallError::ConfigurationRequired
        );
        assert!(api.pushed.lock().unwrap().is_empty());
        manifest["behaviorHints"]["configurationRequired"] = json!(false);
        *server.body.lock().unwrap() = serde_json::to_vec(&manifest).unwrap();
        let preview = session.preview(url.clone()).await.unwrap();
        session.install(preview.token).await.unwrap();
        let mut profile = session.profile().as_core().clone();
        profile.addons[2].flags.official = true;
        drop(session);
        drop(fixture.state.session_gate.lock().await);
        PanoramaEnv::set_storage(PROFILE_STORAGE_KEY, Some(&profile))
            .await
            .unwrap();
        let mut session = CoreSession::start().await.unwrap();
        manifest["version"] = json!("2.0.0");
        *server.body.lock().unwrap() = serde_json::to_vec(&manifest).unwrap();
        let preview = session.preview(url.clone()).await.unwrap();
        assert!(preview.already_installed);
        session.install(preview.token).await.unwrap();
        let addons = session.installed_addons();
        assert_eq!(addons.len(), 3);
        assert_eq!(addons[2].as_core().manifest.version.to_string(), "2.0.0");
        assert!(addons[2].as_core().flags.official);
        let mut profile = session.profile().as_core().clone();
        profile.addons[2].flags.protected = true;
        drop(session);
        drop(fixture.state.session_gate.lock().await);
        PanoramaEnv::set_storage(PROFILE_STORAGE_KEY, Some(&profile))
            .await
            .unwrap();
        let mut session = CoreSession::start().await.unwrap();
        manifest["version"] = json!("3.0.0");
        *server.body.lock().unwrap() = serde_json::to_vec(&manifest).unwrap();
        let preview = session.preview(url).await.unwrap();
        assert_eq!(
            session.install(preview.token).await.unwrap_err(),
            InstallError::Protected
        );
    });
}
