use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use stremio_core::{
    constants::*,
    models::ctx::Ctx,
    runtime::{Effects, Env, Update, msg::Msg},
    types::{
        events::DismissedEventsBucket, library::LibraryBucket, notifications::NotificationsBucket,
        profile::Profile, search_history::SearchHistoryBucket, server_urls::ServerUrlsBucket,
        streams::StreamsBucket,
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
}

impl Update<PanoramaEnv> for SessionCtx {
    fn update(&mut self, msg: &Msg) -> Effects {
        if self.active.load(Ordering::Acquire) {
            Update::<PanoramaEnv>::update(&mut self.inner, msg)
        } else {
            Effects::none().unchanged()
        }
    }
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
