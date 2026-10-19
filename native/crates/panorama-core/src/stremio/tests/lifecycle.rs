use std::{
    fs,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use tracing::{
    Subscriber,
    span::{Attributes, Id, Record},
};

use super::{
    TestEnv,
    mock::{AUTH_KEY, EMAIL, MockApi, PASSWORD},
};
use crate::{
    store::Store,
    stremio::{CoreErrorKind, CoreSession},
};

#[test]
fn session_api_runs_without_a_tokio_context_on_callers_thread() {
    let api = MockApi::start();
    let fixture = TestEnv::memory(Some(api.base.clone()));
    futures::executor::block_on(async {
        assert!(tokio::runtime::Handle::try_current().is_err());
        let mut session = CoreSession::start().await.unwrap();
        session
            .sign_in(EMAIL.into(), PASSWORD.into())
            .await
            .unwrap();
        assert!(session.is_signed_in());
        session.sign_out().await.unwrap();
        assert!(!session.is_signed_in());
    });
    fixture.idle();
}

#[test]
fn cancelled_sign_in_cannot_authenticate_later_and_allows_retry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cancelled.db");
    let api = MockApi::start();
    api.stall_login.store(true, Ordering::Relaxed);
    let fixture = TestEnv::new(Store::open(&path).unwrap().0, Some(api.base.clone()));
    fixture.runtime.block_on(async {
        let mut session = CoreSession::start().await.unwrap();
        let mut login = Box::pin(session.sign_in(EMAIL.into(), PASSWORD.into()));
        tokio::select! {
            _ = &mut login => panic!("stalled login completed"),
            _ = wait_for_login(&api) => {}
        }
        drop(login);
        assert!(!session.is_signed_in());
        api.stall_login.store(false, Ordering::Relaxed);
        tokio::time::timeout(Duration::from_millis(500), async {
            while api.released_logins.load(Ordering::Relaxed) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        drop(session);
        let mut session = CoreSession::start().await.unwrap();
        assert!(!session.is_signed_in());
        for file in [&path, &dir.path().join("cancelled.db-wal")] {
            assert!(
                !fs::read(file)
                    .unwrap()
                    .windows(AUTH_KEY.len())
                    .any(|bytes| bytes == AUTH_KEY.as_bytes())
            );
        }
        session
            .sign_in(EMAIL.into(), PASSWORD.into())
            .await
            .unwrap();
        assert!(session.is_signed_in());
    });
}

#[test]
fn cancelled_login_quiesces_running_poll_before_logout_and_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cancelled.db");
    let api = MockApi::start();
    api.stall_login.store(true, Ordering::Relaxed);
    let fixture = TestEnv::new(Store::open(&path).unwrap().0, Some(api.base.clone()));
    fixture.runtime.block_on(async {
        let mut session = CoreSession::start().await.unwrap();
        let armed = Arc::new(AtomicBool::new(true));
        let (entered, received) = tokio::sync::oneshot::channel();
        let entered = Mutex::new(Some(entered));
        let (release, resume) = std::sync::mpsc::channel();
        let resume = Mutex::new(resume);
        *fixture.state.before_sequential.lock().unwrap() = Some(Arc::new(move || {
            if armed.swap(false, Ordering::SeqCst) {
                entered.lock().unwrap().take().unwrap().send(()).unwrap();
                resume.lock().unwrap().recv().unwrap();
            }
        }));
        let mut login = Box::pin(session.sign_in(EMAIL.into(), PASSWORD.into()));
        tokio::select! {
            _ = &mut login => panic!("stalled login completed"),
            _ = wait_for_login(&api) => {}
        }
        api.stall_login.store(false, Ordering::Relaxed);
        tokio::select! {
            _ = &mut login => panic!("blocked worker completed"),
            _ = received => {}
        }
        drop(login);
        assert!(!session.is_signed_in());
        drop(session);
        let mut restart = Box::pin(CoreSession::start());
        let early = tokio::time::timeout(Duration::from_millis(10), &mut restart).await;
        let waited_for_cleanup = early.is_err();
        release.send(()).unwrap();
        *fixture.state.before_sequential.lock().unwrap() = None;
        let session = match early {
            Ok(result) => result.unwrap(),
            Err(_) => restart.await.unwrap(),
        };
        assert!(waited_for_cleanup);
        assert!(!session.is_signed_in());
        for file in [&path, &dir.path().join("cancelled.db-wal")] {
            assert!(
                !fs::read(file)
                    .unwrap()
                    .windows(AUTH_KEY.len())
                    .any(|bytes| bytes == AUTH_KEY.as_bytes())
            );
        }
    });
}

#[test]
fn failed_collection_cleanup_finishes_before_immediate_healthy_retry() {
    let api = MockApi::start();
    api.reject_addons.store(true, Ordering::Relaxed);
    api.stall_logout.store(true, Ordering::Relaxed);
    api.reject_logout.store(true, Ordering::Relaxed);
    let fixture = TestEnv::memory(Some(api.base.clone()));
    fixture.runtime.block_on(async {
        let mut session = CoreSession::start().await.unwrap();
        let mut first = Box::pin(session.sign_in(EMAIL.into(), PASSWORD.into()));
        tokio::select! {
            _ = &mut first => panic!("failed login returned before logout cleanup finished"),
            _ = wait_for_call(&api, "/api/logout") => {}
        }
        api.reject_addons.store(false, Ordering::Relaxed);
        api.stall_logout.store(false, Ordering::Relaxed);
        assert_eq!(first.await.unwrap_err().kind, CoreErrorKind::Other);
        let profile = session
            .sign_in(EMAIL.into(), PASSWORD.into())
            .await
            .unwrap();
        assert!(profile.is_signed_in());
    });
}

#[test]
fn thirty_second_sign_in_deadline_is_sanitized_and_cancels_authentication() {
    let api = MockApi::start();
    api.stall_login.store(true, Ordering::Relaxed);
    let fixture = TestEnv::memory(Some(api.base.clone()));
    fixture.runtime.block_on(async {
        let mut session = CoreSession::start().await.unwrap();
        session.sign_in_deadline = Duration::from_millis(50);
        assert_eq!(
            tokio::time::timeout(
                Duration::from_millis(250),
                session.sign_in(EMAIL.into(), PASSWORD.into())
            )
            .await
            .expect("session deadline must beat 30-second transport timeout")
            .unwrap_err()
            .kind,
            CoreErrorKind::Timeout
        );
        assert!(!session.is_signed_in());
    });
}

#[test]
fn api_credential_codes_are_distinct_from_other_api_errors() {
    let api = MockApi::start();
    let fixture = TestEnv::memory(Some(api.base.clone()));
    fixture.runtime.block_on(async {
        let mut session = CoreSession::start().await.unwrap();
        for (code, kind) in [
            (1, CoreErrorKind::WrongCredentials),
            (2, CoreErrorKind::WrongCredentials),
            (500, CoreErrorKind::Other),
        ] {
            api.login_error.store(code, Ordering::Relaxed);
            assert_eq!(
                session
                    .sign_in(EMAIL.into(), PASSWORD.into())
                    .await
                    .unwrap_err()
                    .kind,
                kind
            );
        }
    });
}

#[derive(Clone)]
struct CountEvents(Arc<AtomicUsize>);

impl Subscriber for CountEvents {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, _: &Record<'_>) {}
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        if event.metadata().target().starts_with("stremio_core")
            || event.metadata().target().starts_with("panorama_core")
        {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
    fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
        Some(tracing::level_filters::LevelFilter::TRACE)
    }
}

#[test]
fn upstream_diagnostics_are_suppressed_even_with_enabled_app_subscriber() {
    let api = MockApi::start();
    let fixture = TestEnv::memory(Some(api.base.clone()));
    let count = Arc::new(AtomicUsize::new(0));
    tracing::subscriber::set_global_default(CountEvents(count.clone())).unwrap();
    fixture.runtime.block_on(async {
        tracing::trace!("test subscriber active");
        assert_eq!(count.load(Ordering::SeqCst), 1);
        let mut session = CoreSession::start().await.unwrap();
        session
            .sign_in(EMAIL.into(), PASSWORD.into())
            .await
            .unwrap();
        session.sign_out().await.unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 1);
    });
}

async fn wait_for_login(api: &MockApi) {
    wait_for_call(api, "/api/login").await;
}

async fn wait_for_call(api: &MockApi, expected: &str) {
    tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            if api
                .calls
                .lock()
                .unwrap()
                .iter()
                .any(|path| path == expected)
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}
