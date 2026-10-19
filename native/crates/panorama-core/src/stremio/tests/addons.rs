use futures::StreamExt;
use serde_json::json;

use super::{TestEnv, tls::TlsMock};
use crate::{
    addons::{AddonClient, CoreAddonTransport, FailureKind, ResourceEvent},
    stremio::Descriptor,
};

#[test]
fn core_addon_transport_encodes_opaque_ids_preserves_credits_and_enforces_fetch_cap() {
    let mut fixture = TestEnv::memory(None);
    let server = fixture.runtime.block_on(TlsMock::start());
    fixture.trust_tls(&[&server]);
    fixture.runtime.block_on(async {
        let id = "opaque:雪%2F";
        *server.body.lock().unwrap() = serde_json::to_vec(&json!({"meta":{
            "id":id,"type":"movie","name":"Film","director":["Director"],"cast":["Actor"],
            "genres":["Drama"],"imdbRating":"8.1"
        }})).unwrap();
        let addon = Descriptor::from_core(serde_json::from_value(json!({
            "transportUrl":server.base.join("private-key/manifest.json").unwrap(),
            "manifest":{"id":"local","name":"Local","version":"1.0.0","types":["movie"],"resources":["meta"]}
        })).unwrap());
        let client = AddonClient::new(fixture.state.store.clone(), CoreAddonTransport).await.unwrap();
        let events = client.details(std::slice::from_ref(&addon), id, None).collect::<Vec<_>>().await;
        let ResourceEvent::Fresh(film) = &events[0] else { panic!("expected metadata: {events:?}"); };
        assert_eq!(film.id, id);
        assert_eq!(film.director, ["Director"]);
        assert_eq!(film.cast, ["Actor"]);
        assert_eq!(film.genres, ["Drama"]);
        assert_eq!(film.imdb_rating.as_deref(), Some("8.1"));
        {
            let requests = server.requests.lock().unwrap();
            assert!(requests[0].starts_with("GET /private-key/meta/movie/opaque%3A%E9%9B%AA%252F.json "));
        }
        *server.body.lock().unwrap() = vec![b' '; 8 * 1024 * 1024 + 1];
        assert_eq!(client.details(&[addon], "opaque:large", None).collect::<Vec<_>>().await, [ResourceEvent::Failed(FailureKind::TooLarge)]);
    });
}
