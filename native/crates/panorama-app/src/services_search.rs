use super::*;

impl Services {
    /// Deliver merged search snapshots from a Tokio producer.
    pub fn search(
        self: &Arc<Self>,
        addons: Vec<Descriptor>,
        query: String,
    ) -> (
        Job<()>,
        mpsc::UnboundedReceiver<ResourceEvent<Vec<FilmDetails>>>,
    ) {
        let (sender, receiver) = mpsc::unbounded();
        let services = self.clone();
        let job = self.runtime.spawn(async move {
            let Some(client) = &services.addons else {
                let _ =
                    sender.unbounded_send(ResourceEvent::Fresh(crate::fixtures::search(&query)));
                return;
            };
            let mut stream = client.search(&addons, &query);
            while let Some(event) = stream.next().await {
                if sender.unbounded_send(event).is_err() {
                    break;
                }
            }
        });
        (Job(job), receiver)
    }
}
