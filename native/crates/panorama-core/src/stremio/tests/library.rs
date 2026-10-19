use super::{
    TestEnv,
    mock::{EMAIL, MockApi, PASSWORD},
};
use crate::{addons::FilmDetails, stremio::CoreSession};

#[test]
fn library_actions_persist_and_publish_add_remove() {
    let api = MockApi::start();
    let fixture = TestEnv::memory(Some(api.base.clone()));
    fixture.runtime.block_on(async {
        let film: FilmDetails = serde_json::from_value(serde_json::json!({
            "id": "tt1234567", "name": "A film", "release_info": "2022", "runtime": null,
            "genres": [], "description": null, "director": [], "cast": [], "poster": null,
            "background": null, "logo": null, "imdb_rating": null, "links": []
        }))
        .unwrap();
        let mut session = CoreSession::start().await.unwrap();
        assert!(session.set_watchlisted(&film, true).await.is_err());
        session
            .sign_in(EMAIL.into(), PASSWORD.into())
            .await
            .unwrap();
        let mut changes = session.subscribe();
        session.set_watchlisted(&film, true).await.unwrap();
        assert!(session.is_watchlisted(&film.id));
        assert_eq!(
            changes.recv().await.unwrap(),
            crate::stremio::CoreChange::LibraryChanged
        );
        session.set_watchlisted(&film, true).await.unwrap();
        session.set_watchlisted(&film, false).await.unwrap();
        assert!(!session.is_watchlisted(&film.id));
        assert_eq!(
            changes.recv().await.unwrap(),
            crate::stremio::CoreChange::LibraryChanged
        );
        drop(session);
        let session = CoreSession::start().await.unwrap();
        assert!(!session.is_watchlisted(&film.id));
    });
}
