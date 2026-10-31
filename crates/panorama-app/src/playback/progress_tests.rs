use super::*;
use panorama_core::{
    store::{Key, Store},
    stremio::{
        CoreSession,
        env::{EnvConfig, PanoramaEnv},
    },
};

#[tokio::test]
async fn signed_out_core_progress_changes_no_library_and_emits_no_actions() {
    let store = Arc::new(Store::open_in_memory().unwrap());
    PanoramaEnv::install(EnvConfig {
        store: store.clone(),
        api_base: None,
        runtime: tokio::runtime::Handle::current(),
    })
    .unwrap();
    let session = CoreSession::start().await.unwrap();
    assert!(!session.is_signed_in());
    let mut changes = session.subscribe();
    let keys = [
        Key::core("library").unwrap(),
        Key::core("library_recent").unwrap(),
    ];
    let before = keys.each_ref().map(|key| store.get(key).unwrap());
    let session = Arc::new(tokio::sync::Mutex::new(session));
    let progress = CoreProgress(Arc::new(Services {
        data_dir: Default::default(),
        discord: None,
        runtime: tokio::runtime::Handle::current(),
        session: Some(session.clone()),
        addons: Some(
            panorama_core::addons::AddonClient::new(
                store.clone(),
                panorama_core::addons::CoreAddonTransport,
            )
            .await
            .unwrap(),
        ),
        images: None,
        store: None,
        prefs_writer: None,
        fixture_file: None,
        credits: None,
        mubi: None,
    }));
    let args = crate::args::Args::parse(["--fixtures".into()]).unwrap();
    let selection = crate::playback::debug::selection("tmdb:100", &args).unwrap();
    let id = selection.film.id.clone();
    let sample = PlaybackSample {
        time_secs: 500.0,
        duration_secs: 600.0,
        paused: false,
    };
    for event in [
        Progress::Begin(Box::new(selection), 90.0),
        Progress::Sample(id.clone(), sample),
        Progress::Seek(id.clone(), sample),
        Progress::Finish(id.clone()),
        Progress::Stop(id.clone()),
    ] {
        progress.send(event).await;
    }
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), changes.recv())
            .await
            .is_err()
    );
    let session = session.lock().await;
    assert!(!session.in_library(&id));
    assert!(session.resume(&id).is_none());
    assert_eq!(keys.each_ref().map(|key| store.get(key).unwrap()), before);
    assert!(matches!(
        changes.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
}
