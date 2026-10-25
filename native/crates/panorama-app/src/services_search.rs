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
        self.search_with(addons, query, false)
    }

    /// Each addon's search results as it answers, for finding one film fast.
    pub fn search_each(
        self: &Arc<Self>,
        addons: Vec<Descriptor>,
        query: String,
    ) -> (
        Job<()>,
        mpsc::UnboundedReceiver<ResourceEvent<Vec<FilmDetails>>>,
    ) {
        self.search_with(addons, query, true)
    }

    fn search_with(
        self: &Arc<Self>,
        addons: Vec<Descriptor>,
        query: String,
        each: bool,
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
            let mut stream = if each {
                client.search_each(&addons, &query)
            } else {
                client.search(&addons, &query)
            };
            while let Some(event) = stream.next().await {
                if sender.unbounded_send(event).is_err() {
                    break;
                }
            }
        });
        (Job(job), receiver)
    }
}
