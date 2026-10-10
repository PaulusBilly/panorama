use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use stremio_core::{
    models::ctx::Ctx,
    runtime::{
        Env, Runtime, RuntimeAction,
        msg::{Action, ActionCtx},
    },
    types::{addon::Descriptor as CoreDescriptor, api::AuthRequest},
};
use tokio::{
    sync::{OwnedMutexGuard, broadcast},
    task::JoinHandle,
};
use tracing::subscriber::{NoSubscriber, with_default};

use super::{
    CoreChange, CoreError, CoreErrorKind, Descriptor, Profile,
    env::{PanoramaEnv, State},
    fetch::TIMEOUT,
    model::{self, CoreModel, SessionCtx},
};

type CoreRuntime = Runtime<PanoramaEnv, CoreModel>;

#[derive(Clone)]
pub(super) enum Progress {
    Authenticated(AuthRequest),
    AddonsReady,
    LibraryReady,
    AuthFailed(AuthRequest, CoreErrorKind),
    AddonsFailed(CoreErrorKind),
    LibraryFailed(CoreErrorKind),
    Collection(
        Vec<url::Url>,
        Result<(), crate::addons::manage::InstallError>,
    ),
    AddonRejected(url::Url, crate::addons::manage::InstallError),
}

pub(super) struct Running {
    pub(super) runtime: Arc<CoreRuntime>,
    active: Arc<AtomicBool>,
    pub(super) progress: broadcast::Sender<Progress>,
    pub(super) order: Arc<std::sync::Mutex<Option<Vec<CoreDescriptor>>>>,
    pub(super) replacement: Arc<std::sync::Mutex<Option<CoreDescriptor>>>,
    pump: JoinHandle<()>,
}

struct AuthAttempt<'a> {
    running: Option<Running>,
    state: Arc<State>,
    pending: &'a mut Option<JoinHandle<Result<(), CoreError>>>,
    completion: Option<tokio::task::AbortHandle>,
}

impl Drop for AuthAttempt<'_> {
    fn drop(&mut self) {
        let Some(running) = self.running.take() else {
            return;
        };
        if let Some(completion) = &self.completion {
            completion.abort();
        }
        *self.pending = Some(running.logout(Arc::clone(&self.state)));
    }
}

impl Running {
    fn new(ctx: Ctx, state: &State, changes: broadcast::Sender<CoreChange>) -> Self {
        let active = Arc::new(AtomicBool::new(true));
        let previous_profile = ctx.profile.clone();
        let previous_library = ctx.library.clone();
        let order = Arc::new(std::sync::Mutex::new(None));
        let replacement = Arc::new(std::sync::Mutex::new(None));
        let (runtime, events) = Runtime::new(
            CoreModel {
                ctx: SessionCtx {
                    inner: ctx,
                    active: Arc::clone(&active),
                    order: Arc::clone(&order),
                    replacement: Arc::clone(&replacement),
                },
            },
            vec![],
            1024,
        );
        let runtime = Arc::new(runtime);
        let (progress, _) = broadcast::channel(64);
        let pump = state.runtime.spawn(super::events::pump(
            events,
            Arc::downgrade(&runtime),
            changes,
            progress.clone(),
            previous_profile,
            previous_library,
        ));
        Self {
            runtime,
            active,
            progress,
            order,
            replacement,
            pump,
        }
    }

    pub(super) fn dispatch(&self, action: ActionCtx) {
        with_default(NoSubscriber::default(), || {
            self.runtime.dispatch(RuntimeAction {
                field: None,
                action: Action::Ctx(action),
            })
        });
    }

    fn logout(self, state: Arc<State>) -> JoinHandle<Result<(), CoreError>> {
        self.active.store(false, Ordering::Release);
        state.runtime.clone().spawn(async move {
            state.cancel_concurrent().await;
            self.active.store(true, Ordering::Release);
            self.dispatch(ActionCtx::Logout);
            self.active.store(false, Ordering::Release);
            let drained = self.stop(&state, true).await;
            state
                .blocking(|store| store.clear_on_sign_out())
                .await
                .map_err(|_| CoreError::from(CoreErrorKind::Storage))?;
            PanoramaEnv::migrate_storage_schema()
                .await
                .map_err(|_| CoreError::from(CoreErrorKind::Storage))?;
            state
                .blocking(|store| store.checkpoint())
                .await
                .map_err(|_| CoreError::from(CoreErrorKind::Storage))?;
            drained
        })
    }

    async fn stop(self, state: &State, wait_for_logout: bool) -> Result<(), CoreError> {
        self.active.store(false, Ordering::Release);
        drop(self.runtime);
        if wait_for_logout {
            state.finish_concurrent().await;
        } else {
            state.cancel_concurrent().await;
        }
        let result = state.drain().await;
        self.pump
            .await
            .map_err(|_| CoreError::from(CoreErrorKind::Other))?;
        result
    }
}

/// The single active, persisted account session; debug output contains no secrets.
pub struct CoreSession {
    pub(super) sign_in_deadline: std::time::Duration,
    pub(super) running: Option<Running>,
    pending_sign_out: Option<JoinHandle<Result<(), CoreError>>>,
    pub(super) state: Arc<State>,
    changes: broadcast::Sender<CoreChange>,
    lease: Option<Arc<OwnedMutexGuard<()>>>,
    pub(super) management: super::manage::Management,
}

impl CoreSession {
    /// Migrates core storage, rehydrates every Ctx bucket and starts event delivery.
    /// Waits for a previous session's background cleanup before touching storage.
    pub async fn start() -> Result<Self, CoreError> {
        let state =
            PanoramaEnv::state().map_err(|_| CoreError::from(CoreErrorKind::Environment))?;
        let lease = Arc::new(Arc::clone(&state.session_gate).lock_owned().await);
        *state
            .session_lease
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Arc::downgrade(&lease);
        let ctx = model::rehydrate().await?;
        let (changes, _) = broadcast::channel(64);
        let running = Running::new(ctx, &state, changes.clone());
        Ok(Self {
            sign_in_deadline: TIMEOUT,
            running: Some(running),
            pending_sign_out: None,
            state,
            changes,
            lease: Some(lease),
            management: Default::default(),
        })
    }

    /// Returns a cloned, debug-redacted profile without holding a model guard.
    pub fn profile(&self) -> Profile {
        Profile(
            self.running
                .as_ref()
                .map(|running| {
                    running
                        .runtime
                        .model()
                        .unwrap_or_else(|p| p.into_inner())
                        .ctx
                        .inner
                        .profile
                        .clone()
                })
                .unwrap_or_default(),
        )
    }

    /// Returns whether the current profile has an authenticated account.
    pub fn is_signed_in(&self) -> bool {
        self.profile().is_signed_in()
    }

    /// Returns debug-redacted descriptors in account order.
    pub fn installed_addons(&self) -> Vec<Descriptor> {
        self.profile().installed_addons()
    }

    /// Subscribes to sanitized changes; lagging receivers may resnapshot the profile.
    pub fn subscribe(&self) -> broadcast::Receiver<CoreChange> {
        self.changes.subscribe()
    }

    /// Authenticates and waits for account addons, library and durable FIFO writes.
    /// Credentials and raw core errors never enter the returned error.
    /// Cancelling this future cancels the login and resets the in-memory account.
    pub async fn sign_in(&mut self, email: String, password: String) -> Result<Profile, CoreError> {
        let _ = self.settle_addons().await;
        self.management.tokens.clear();
        self.finish_sign_out().await?;
        PanoramaEnv::migrate_storage_schema()
            .await
            .map_err(|_| CoreError::from(CoreErrorKind::Storage))?;
        let running = self.running.take().ok_or(CoreErrorKind::Other)?;
        let mut progress = running.progress.subscribe();
        let request = AuthRequest::Login {
            email,
            password,
            facebook: false,
        };
        running.dispatch(ActionCtx::Authenticate(request.clone()));
        let mut attempt = AuthAttempt {
            running: Some(running),
            state: Arc::clone(&self.state),
            pending: &mut self.pending_sign_out,
            completion: None,
        };
        let state = Arc::clone(&self.state);
        let deadline = self.sign_in_deadline;
        let completion = self.state.runtime.spawn(async move {
            tokio::time::timeout(deadline, async {
                let (mut auth, mut addons, mut library) = (false, false, false);
                loop {
                    match progress.recv().await {
                        Ok(Progress::Authenticated(received)) if received == request => auth = true,
                        Ok(Progress::AddonsReady) if auth => addons = true,
                        Ok(Progress::LibraryReady) if auth => library = true,
                        Ok(Progress::AuthFailed(received, kind)) if received == request => {
                            return Err(CoreError::from(kind));
                        }
                        Ok(Progress::AddonsFailed(kind) | Progress::LibraryFailed(kind))
                            if auth =>
                        {
                            return Err(CoreError::from(kind));
                        }
                        Ok(_) => {}
                        Err(_) => return Err(CoreErrorKind::Other.into()),
                    }
                    if auth && addons && library {
                        return state.drain().await;
                    }
                }
            })
            .await
            .unwrap_or_else(|_| Err(CoreErrorKind::Timeout.into()))
        });
        attempt.completion = Some(completion.abort_handle());
        let result = completion
            .await
            .unwrap_or_else(|_| Err(CoreErrorKind::Other.into()));
        if let Err(error) = result {
            drop(attempt);
            if error.kind != CoreErrorKind::Timeout {
                let _ = self.finish_sign_out().await;
            }
            let _ = self.changes.send(CoreChange::Error(error.kind));
            return Err(error);
        }
        self.running = attempt.running.take();
        drop(attempt);
        Ok(self.profile())
    }

    /// Logs out, drains old effects and removes account data from DB, WAL and quarantine.
    /// The in-memory account is cleared even on storage failure; callers may retry.
    /// Cleanup continues if this future is cancelled or the session is dropped.
    pub async fn sign_out(&mut self) -> Result<(), CoreError> {
        let _ = self.settle_addons().await;
        self.management.tokens.clear();
        if self.pending_sign_out.is_none() {
            let running = self.running.take().ok_or(CoreErrorKind::Other)?;
            self.pending_sign_out = Some(running.logout(Arc::clone(&self.state)));
        }
        self.finish_sign_out().await
    }

    async fn finish_sign_out(&mut self) -> Result<(), CoreError> {
        let Some(pending) = self.pending_sign_out.as_mut() else {
            return Ok(());
        };
        let result = pending
            .await
            .unwrap_or_else(|_| Err(CoreErrorKind::Other.into()));
        self.pending_sign_out = None;
        self.running = Some(Running::new(
            model::defaults(),
            &self.state,
            self.changes.clone(),
        ));
        for change in [
            CoreChange::ProfileChanged,
            CoreChange::AddonsChanged,
            CoreChange::LibraryChanged,
        ] {
            let _ = self.changes.send(change);
        }
        if let Err(error) = result {
            let _ = self.changes.send(CoreChange::Error(error.kind));
        }
        result
    }
}

impl fmt::Debug for CoreSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CoreSession")
            .field("profile", &self.profile())
            .finish_non_exhaustive()
    }
}

impl Drop for CoreSession {
    fn drop(&mut self) {
        let running = self.running.take();
        if let Some(running) = &running {
            running.active.store(false, Ordering::Release);
        }
        let pending = self.pending_sign_out.take();
        let pending_addons = self.management.pending.take();
        let lease = self.lease.take();
        let state = Arc::clone(&self.state);
        self.state.runtime.spawn(async move {
            if let Some(pending_addons) = pending_addons {
                let _ = pending_addons.await;
            }
            if let Some(pending) = pending {
                let _ = pending.await;
            }
            if let Some(running) = running {
                let _ = running.stop(&state, false).await;
            }
            drop(lease);
        });
    }
}
