use super::{CoreChange, env::PanoramaEnv, error::classify, model::CoreModel, session::Progress};
use futures::StreamExt;
use std::sync::Weak;
use stremio_core::{
    runtime::{Runtime, RuntimeEvent, msg::Event},
    types::{library::LibraryBucket, profile::Profile as CoreProfile},
};
use tokio::sync::broadcast;
type CoreRuntime = Runtime<PanoramaEnv, CoreModel>;
pub(super) async fn pump(
    mut events: futures::channel::mpsc::Receiver<RuntimeEvent<PanoramaEnv, CoreModel>>,
    runtime: Weak<CoreRuntime>,
    changes: broadcast::Sender<CoreChange>,
    progress: broadcast::Sender<Progress>,
    mut profile: CoreProfile,
    mut library: LibraryBucket,
) {
    while let Some(event) = events.next().await {
        match event {
            RuntimeEvent::NewState(_) => {
                if let Some(runtime) = runtime.upgrade() {
                    let model = runtime.model().unwrap_or_else(|p| p.into_inner());
                    let ctx = &model.ctx.inner;
                    if profile.addons != ctx.profile.addons {
                        let _ = changes.send(CoreChange::AddonsChanged);
                    }
                    if profile != ctx.profile {
                        profile = ctx.profile.clone();
                        let _ = changes.send(CoreChange::ProfileChanged);
                    }
                    if library != ctx.library {
                        library = ctx.library.clone();
                        let _ = changes.send(CoreChange::LibraryChanged);
                    }
                }
            }
            RuntimeEvent::CoreEvent(event) => {
                let status = match event {
                    Event::UserAuthenticated { auth_request } => {
                        Some(Progress::Authenticated(auth_request))
                    }
                    Event::UserAddonsLocked {
                        addons_locked: false,
                    } => Some(Progress::AddonsReady),
                    Event::UserLibraryMissing {
                        library_missing: false,
                    } => Some(Progress::LibraryReady),
                    Event::AddonsPushedToAPI { transport_urls } => {
                        Some(Progress::Collection(transport_urls, Ok(())))
                    }
                    Event::Error { error, source } => {
                        let kind = classify(&error);
                        let _ = changes.send(CoreChange::Error(kind));
                        match *source {
                            Event::UserAuthenticated { auth_request } => {
                                Some(Progress::AuthFailed(auth_request, kind))
                            }
                            Event::UserAddonsLocked { .. } => Some(Progress::AddonsFailed(kind)),
                            Event::UserLibraryMissing { .. } => Some(Progress::LibraryFailed(kind)),
                            Event::AddonsPushedToAPI { transport_urls } => {
                                Some(Progress::Collection(
                                    transport_urls,
                                    Err(crate::addons::manage::InstallError::ApiPush),
                                ))
                            }
                            Event::AddonInstalled { transport_url, .. }
                            | Event::AddonUpgraded { transport_url, .. }
                            | Event::AddonUninstalled { transport_url, .. } => {
                                Some(Progress::AddonRejected(
                                    transport_url,
                                    crate::addons::manage::InstallError::Core,
                                ))
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                };
                if let Some(status) = status {
                    let _ = progress.send(status);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::SinkExt;
    use stremio_core::models::ctx::CtxError;
    use stremio_core::types::profile::AuthKey;

    #[test]
    fn unrelated_core_error_does_not_enter_auth_progress() {
        futures::executor::block_on(async {
            let (mut events, receiver) = futures::channel::mpsc::channel(4);
            let (changes, _) = broadcast::channel(4);
            let (progress, mut received) = broadcast::channel(4);
            events
                .send(RuntimeEvent::CoreEvent(Event::Error {
                    error: CtxError::Env(stremio_core::runtime::EnvError::Fetch(
                        "unrelated".into(),
                    )),
                    source: Box::new(Event::SessionDeleted {
                        auth_key: AuthKey("old".into()),
                    }),
                }))
                .await
                .unwrap();
            drop(events);
            pump(
                receiver,
                Weak::new(),
                changes,
                progress,
                CoreProfile::default(),
                LibraryBucket::default(),
            )
            .await;
            assert!(received.try_recv().is_err());
        });
    }
}
