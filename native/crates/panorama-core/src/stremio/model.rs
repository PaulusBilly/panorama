use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use stremio_core::{
    constants::*,
    models::ctx::{Ctx, OtherError},
    runtime::{
        Effects, Env, Update,
        msg::{Action, ActionCtx, Event, Internal, Msg},
    },
    types::{
        addon::Descriptor as CoreDescriptor, events::DismissedEventsBucket, library::LibraryBucket,
        notifications::NotificationsBucket, profile::Profile, search_history::SearchHistoryBucket,
        server_urls::ServerUrlsBucket, streams::StreamsBucket,
    },
};

use super::{CoreError, CoreErrorKind, env::PanoramaEnv};

#[derive(Clone, stremio_core::Model)]
#[model(PanoramaEnv)]
pub(super) struct CoreModel {
    pub(super) ctx: SessionCtx,
}

#[derive(Clone)]
pub(super) struct SessionCtx {
    pub(super) inner: Ctx,
    pub(super) active: Arc<AtomicBool>,
    pub(super) order: Arc<Mutex<Option<Vec<CoreDescriptor>>>>,
    pub(super) replacement: Arc<Mutex<Option<CoreDescriptor>>>,
}

impl Update<PanoramaEnv> for SessionCtx {
    fn update(&mut self, msg: &Msg) -> Effects {
        if self.active.load(Ordering::Acquire) {
            if matches!(msg, Msg::Action(Action::Ctx(ActionCtx::PushAddonsToAPI))) {
                let order = self.order.lock().unwrap_or_else(|p| p.into_inner()).take();
                if let Some(order) = order {
                    self.inner.profile.addons = order;
                    return Update::<PanoramaEnv>::update(&mut self.inner, msg)
                        .join(Effects::msg(Msg::Internal(Internal::ProfileChanged)));
                }
            }
            if let Msg::Internal(Internal::InstallAddon(addon)) = msg {
                let previous = self
                    .replacement
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .take();
                if !self.inner.profile.addons_locked
                    && !addon.manifest.behavior_hints.configuration_required
                {
                    if self.inner.profile.addons.iter().any(|installed| {
                        installed.transport_url == addon.transport_url && installed.flags.protected
                    }) {
                        return reject_install(addon, OtherError::AddonIsProtected);
                    }
                    if let Some(previous) = previous {
                        let Some(index) = self
                            .inner
                            .profile
                            .addons
                            .iter()
                            .position(|installed| installed == &previous)
                        else {
                            return reject_install(addon, OtherError::AddonNotInstalled);
                        };
                        if previous.flags.protected
                            || (previous.flags.official
                                && previous.transport_url != addon.transport_url)
                        {
                            return reject_install(addon, OtherError::AddonIsProtected);
                        }
                        if self
                            .inner
                            .profile
                            .addons
                            .iter()
                            .enumerate()
                            .any(|(other, installed)| {
                                other != index && installed.transport_url == addon.transport_url
                            })
                        {
                            return reject_install(addon, OtherError::AddonAlreadyInstalled);
                        }
                        self.inner.profile.addons[index] = addon.clone();
                        return Update::<PanoramaEnv>::update(
                            &mut self.inner,
                            &Msg::Action(Action::Ctx(ActionCtx::PushAddonsToAPI)),
                        )
                        .join(Effects::msg(Msg::Internal(Internal::ProfileChanged)))
                        .join(Effects::msg(Msg::Event(
                            Event::AddonInstalled {
                                transport_url: addon.transport_url.clone(),
                                id: addon.manifest.id.clone(),
                            },
                        )));
                    }
                }
            }
            Update::<PanoramaEnv>::update(&mut self.inner, msg)
        } else {
            Effects::none().unchanged()
        }
    }
}

fn reject_install(addon: &CoreDescriptor, error: OtherError) -> Effects {
    Effects::msg(Msg::Event(Event::Error {
        error: error.into(),
        source: Box::new(Event::AddonInstalled {
            transport_url: addon.transport_url.clone(),
            id: addon.manifest.id.clone(),
        }),
    }))
    .unchanged()
}

pub(super) async fn rehydrate() -> Result<Ctx, CoreError> {
    PanoramaEnv::migrate_storage_schema()
        .await
        .map_err(|_| CoreError::from(CoreErrorKind::Storage))?;
    let (profile, recent, older, streams, urls, notifications, history, dismissed) =
        futures::try_join!(
            PanoramaEnv::get_storage::<Profile>(PROFILE_STORAGE_KEY),
            PanoramaEnv::get_storage::<LibraryBucket>(LIBRARY_RECENT_STORAGE_KEY),
            PanoramaEnv::get_storage::<LibraryBucket>(LIBRARY_STORAGE_KEY),
            PanoramaEnv::get_storage::<StreamsBucket>(STREAMS_STORAGE_KEY),
            PanoramaEnv::get_storage::<ServerUrlsBucket>(STREAMING_SERVER_URLS_STORAGE_KEY),
            PanoramaEnv::get_storage::<NotificationsBucket>(NOTIFICATIONS_STORAGE_KEY),
            PanoramaEnv::get_storage::<SearchHistoryBucket>(SEARCH_HISTORY_STORAGE_KEY),
            PanoramaEnv::get_storage::<DismissedEventsBucket>(DISMISSED_EVENTS_STORAGE_KEY),
        )
        .map_err(|_| CoreError::from(CoreErrorKind::Storage))?;
    let profile = profile.unwrap_or_default();
    let uid = profile.uid();
    let mut library = LibraryBucket::new(uid.clone(), vec![]);
    if let Some(recent) = recent {
        library.merge_bucket(recent);
    }
    if let Some(older) = older {
        library.merge_bucket(older);
    }
    Ok(Ctx::new(
        profile,
        library,
        streams.unwrap_or_else(|| StreamsBucket::new(uid.clone())),
        urls.unwrap_or_else(|| ServerUrlsBucket::new::<PanoramaEnv>(uid.clone())),
        notifications
            .unwrap_or_else(|| NotificationsBucket::new::<PanoramaEnv>(uid.clone(), vec![])),
        history.unwrap_or_else(|| SearchHistoryBucket::new(uid.clone())),
        dismissed.unwrap_or_else(|| DismissedEventsBucket::new(uid)),
    ))
}

pub(super) fn defaults() -> Ctx {
    Ctx::new(
        Profile::default(),
        LibraryBucket::default(),
        StreamsBucket::default(),
        ServerUrlsBucket::new::<PanoramaEnv>(None),
        NotificationsBucket::new::<PanoramaEnv>(None, vec![]),
        SearchHistoryBucket::default(),
        DismissedEventsBucket::default(),
    )
}
