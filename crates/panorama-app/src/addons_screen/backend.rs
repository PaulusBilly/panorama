//! Core calls for the Addons screen, always run on the services runtime.
//! Results come back over oneshot channels; the UI thread only awaits them.
use super::{
    fixture,
    model::{
        AddonError, DESCRIPTION_CAP, InstalledRow, MetaChoice, NAME_CAP, PreviewCard, RowPending,
        capabilities, display_text, media_types,
    },
};
use futures::channel::oneshot;
use panorama_core::{
    addons::{
        AddonKey,
        manage::{AddonPreview, AddonUrl, PreviewToken},
    },
    stremio::CoreSession,
};
use std::{
    future::Future,
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};
use tokio::{runtime::Handle, sync::Mutex};
use url::Url;

/// A confirmation the core or the fixture store will accept once.
pub enum Token {
    /// Issued by `CoreSession::preview`.
    Live(PreviewToken),
    /// Issued by the fixture store.
    Fixture,
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token { .. }")
    }
}

/// The list and the Film details choice, read together.
pub struct Snapshot {
    /// Installed addons in account order.
    pub rows: Vec<InstalledRow>,
    /// Addons that can supply film details.
    pub choices: Vec<MetaChoice>,
    /// Saved film details source; `None` is Automatic.
    pub meta: Option<AddonKey>,
}

/// A mutation's outcome plus the list as the core now holds it, when readable.
pub struct Done {
    /// Whether the core accepted the change.
    pub result: Result<(), AddonError>,
    /// Fresh state, also after a failure so the screen can resync.
    pub snapshot: Option<Snapshot>,
}

type Reply<T> = oneshot::Receiver<T>;

/// Mutable in-memory list behind the fixture backend.
pub struct FixtureState {
    rows: Vec<InstalledRow>,
    meta: Option<AddonKey>,
    installs: usize,
}

/// Where addon operations run.
#[derive(Clone)]
pub enum Backend {
    /// The signed-in core session.
    Live(Arc<Mutex<CoreSession>>, Handle),
    /// In-memory addons for `--fixtures`.
    Fixture(Arc<StdMutex<FixtureState>>, Handle),
}

fn run<T: Send + 'static>(
    runtime: &Handle,
    work: impl Future<Output = T> + Send + 'static,
) -> Reply<T> {
    let (sender, receiver) = oneshot::channel();
    runtime.spawn(async move {
        let _ = sender.send(work.await);
    });
    receiver
}

async fn snapshot(session: &mut CoreSession) -> Result<Snapshot, AddonError> {
    let installed = session.installed_addons();
    let mut rows = Vec::with_capacity(installed.len());
    let mut choices = vec![];
    for addon in &installed {
        let key = session.addon_key(addon).await?;
        let summary = addon.summary();
        let mut name = display_text(&summary.name, NAME_CAP);
        if name.is_empty() {
            name = "Untitled addon".into();
        }
        if summary.movie_meta {
            choices.push(MetaChoice {
                key: key.clone(),
                name: name.clone(),
            });
        }
        rows.push(InstalledRow {
            key,
            name,
            version: display_text(&summary.version, 40),
            logo: summary.logo,
            capabilities: capabilities(&summary.resources),
            protected: summary.protected,
            configurable: summary.configurable,
            pending: RowPending::None,
        });
    }
    let meta = session.meta_source().await?;
    Ok(Snapshot {
        rows,
        choices,
        meta,
    })
}

fn card(preview: &AddonPreview) -> PreviewCard {
    let mut name = display_text(&preview.name, NAME_CAP);
    if name.is_empty() {
        name = "Untitled addon".into();
    }
    PreviewCard {
        name,
        version: display_text(&preview.version, 40),
        description: preview
            .description
            .as_deref()
            .map(|text| display_text(text, DESCRIPTION_CAP))
            .filter(|text| !text.is_empty()),
        logo: preview.logo.clone(),
        capabilities: capabilities(&preview.resources),
        types: media_types(&preview.types),
    }
}

async fn done(session: &mut CoreSession, result: Result<(), AddonError>) -> Done {
    Done {
        result,
        snapshot: snapshot(session).await.ok(),
    }
}

async fn pause() {
    tokio::time::sleep(Duration::from_millis(250)).await;
}

impl Backend {
    /// Pick the session when there is one, otherwise the fixture store.
    pub fn new(session: Option<Arc<Mutex<CoreSession>>>, runtime: Handle) -> Self {
        match session {
            Some(session) => Self::Live(session, runtime),
            None => Self::Fixture(
                Arc::new(StdMutex::new(FixtureState {
                    rows: fixture::rows(),
                    meta: None,
                    installs: 0,
                })),
                runtime,
            ),
        }
    }

    fn runtime(&self) -> &Handle {
        match self {
            Self::Live(_, runtime) | Self::Fixture(_, runtime) => runtime,
        }
    }

    fn fixture_snapshot(state: &FixtureState) -> Snapshot {
        Snapshot {
            rows: state.rows.clone(),
            choices: fixture::meta_choices(),
            meta: state.meta.clone(),
        }
    }

    /// Read the installed list and Film details choice.
    pub fn load(&self) -> Reply<Result<Snapshot, AddonError>> {
        match self {
            Self::Live(session, runtime) => {
                let session = session.clone();
                run(runtime, async move {
                    snapshot(&mut *session.lock().await).await
                })
            }
            Self::Fixture(state, runtime) => {
                let state = state.clone();
                run(runtime, async move {
                    let state = state.lock().unwrap_or_else(|p| p.into_inner());
                    Ok(Self::fixture_snapshot(&state))
                })
            }
        }
    }

    /// Fetch a manifest and keep its single-use confirmation.
    pub fn preview(&self, url: AddonUrl) -> Reply<Result<(PreviewCard, Token), AddonError>> {
        match self {
            Self::Live(session, runtime) => {
                let session = session.clone();
                run(runtime, async move {
                    // The session is held only to start and to finish; the download,
                    // which can take its full timeout, runs without it.
                    let fetch = session.lock().await.begin_preview(url).await?;
                    let fetched = fetch.run().await?;
                    let preview = session.lock().await.finish_preview(fetched).await?;
                    let card = card(&preview);
                    let AddonPreview { token, .. } = preview;
                    Ok((card, Token::Live(token)))
                })
            }
            Self::Fixture(_, runtime) => run(runtime, async move {
                let _ = url;
                pause().await;
                Ok((fixture::preview_card(), Token::Fixture))
            }),
        }
    }

    /// Install exactly the previewed manifest.
    pub fn install(&self, token: Token) -> Reply<Done> {
        match (self, token) {
            (Self::Live(session, runtime), Token::Live(token)) => {
                let session = session.clone();
                run(runtime, async move {
                    let mut session = session.lock().await;
                    let result = session.install(token).await.map(|_| ()).map_err(Into::into);
                    done(&mut session, result).await
                })
            }
            (Self::Fixture(state, runtime), _) => {
                let state = state.clone();
                run(runtime, async move {
                    pause().await;
                    let mut state = state.lock().unwrap_or_else(|p| p.into_inner());
                    state.installs += 1;
                    let card = fixture::preview_card();
                    let key = serde_json::from_value(
                        format!("example-{}#fixture", state.installs).into(),
                    );
                    if let Ok(key) = key {
                        state.rows.push(InstalledRow {
                            key,
                            name: card.name,
                            version: card.version,
                            logo: None,
                            capabilities: card.capabilities,
                            protected: false,
                            configurable: false,
                            pending: RowPending::None,
                        });
                    }
                    Done {
                        result: Ok(()),
                        snapshot: Some(Self::fixture_snapshot(&state)),
                    }
                })
            }
            (backend, _) => run(backend.runtime(), async {
                Done {
                    result: Err(AddonError::Core(
                        panorama_core::addons::manage::InstallError::InvalidToken,
                    )),
                    snapshot: None,
                }
            }),
        }
    }

    /// Remove an unprotected addon.
    pub fn remove(&self, key: AddonKey) -> Reply<Done> {
        match self {
            Self::Live(session, runtime) => {
                let session = session.clone();
                run(runtime, async move {
                    let mut session = session.lock().await;
                    let result = session.remove(key).await.map(|_| ()).map_err(Into::into);
                    done(&mut session, result).await
                })
            }
            Self::Fixture(state, runtime) => {
                let state = state.clone();
                run(runtime, async move {
                    pause().await;
                    let mut state = state.lock().unwrap_or_else(|p| p.into_inner());
                    state.rows.retain(|row| row.key != key);
                    Done {
                        result: Ok(()),
                        snapshot: Some(Self::fixture_snapshot(&state)),
                    }
                })
            }
        }
    }

    /// Persist a complete account order.
    pub fn reorder(&self, order: Vec<AddonKey>) -> Reply<Done> {
        match self {
            Self::Live(session, runtime) => {
                let session = session.clone();
                run(runtime, async move {
                    let mut session = session.lock().await;
                    let result = session.reorder(order).await.map(|_| ()).map_err(Into::into);
                    done(&mut session, result).await
                })
            }
            Self::Fixture(state, runtime) => {
                let state = state.clone();
                run(runtime, async move {
                    let mut state = state.lock().unwrap_or_else(|p| p.into_inner());
                    state
                        .rows
                        .sort_by_key(|row| order.iter().position(|key| key == &row.key));
                    Done {
                        result: Ok(()),
                        snapshot: Some(Self::fixture_snapshot(&state)),
                    }
                })
            }
        }
    }

    /// Persist the Film details source; `None` is Automatic.
    pub fn set_meta(&self, source: Option<AddonKey>) -> Reply<Done> {
        match self {
            Self::Live(session, runtime) => {
                let session = session.clone();
                run(runtime, async move {
                    let mut session = session.lock().await;
                    let result = session.set_meta_source(source).await.map_err(Into::into);
                    done(&mut session, result).await
                })
            }
            Self::Fixture(state, runtime) => {
                let state = state.clone();
                run(runtime, async move {
                    let mut state = state.lock().unwrap_or_else(|p| p.into_inner());
                    state.meta = source;
                    Done {
                        result: Ok(()),
                        snapshot: Some(Self::fixture_snapshot(&state)),
                    }
                })
            }
        }
    }

    /// The configuration page for the system browser. The URL may carry secrets.
    pub fn configure(&self, key: AddonKey) -> Reply<Option<Url>> {
        match self {
            Self::Live(session, runtime) => {
                let session = session.clone();
                run(runtime, async move {
                    session.lock().await.configure_url(&key).await
                })
            }
            Self::Fixture(_, runtime) => run(runtime, async {
                Url::parse("https://example.invalid/configure").ok()
            }),
        }
    }
}
