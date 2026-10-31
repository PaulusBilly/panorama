use std::{sync::atomic::Ordering, time::Duration};

use stremio_core::{constants::*, runtime::Env, types::profile::Profile};

use super::{
    TestEnv,
    mock::{EMAIL, MockApi, PASSWORD},
};
use crate::stremio::{CoreSession, env::PanoramaEnv};

#[test]
fn cancelled_start_retains_gate_until_commit_and_later_session_wins() {
    for sign_out in [false, true] {
        let api = MockApi::start();
        let fixture = TestEnv::memory(Some(api.base.clone()));
        fixture.runtime.block_on(async {
            let mut session = CoreSession::start().await.unwrap();
            session
                .sign_in(EMAIL.into(), PASSWORD.into())
                .await
                .unwrap();
            drop(session);
        });
        fixture.idle();
        fixture.runtime.block_on(async {
            PanoramaEnv::set_storage(SCHEMA_VERSION_STORAGE_KEY, Some(&(SCHEMA_VERSION - 1)))
                .await
                .unwrap();
            let (entered, received) = tokio::sync::oneshot::channel();
            let (release, resume) = std::sync::mpsc::channel();
            let (finished, committed) = tokio::sync::oneshot::channel();
            *fixture.state.before_migration_commit.lock().unwrap() = Some(Box::new(move || {
                entered.send(()).unwrap();
                resume.recv().unwrap();
                finished
            }));
            let mut start = Box::pin(CoreSession::start());
            tokio::select! {
                _ = &mut start => panic!("blocked migration completed"),
                result = received => result.unwrap(),
            }
            drop(start);
            let mut restart = Box::pin(CoreSession::start());
            let early = tokio::time::timeout(Duration::from_millis(10), &mut restart).await;
            let waited = early.is_err();
            let mut later = match early {
                Ok(result) => Some(result.unwrap()),
                Err(_) => None,
            };
            if let Some(session) = &mut later {
                later_write(session, sign_out).await;
            }
            release.send(()).unwrap();
            committed.await.unwrap();
            let mut later = match later {
                Some(session) => session,
                None => {
                    let mut session = restart.await.unwrap();
                    later_write(&mut session, sign_out).await;
                    session
                }
            };
            let stored = PanoramaEnv::get_storage::<Profile>(PROFILE_STORAGE_KEY)
                .await
                .unwrap();
            if sign_out {
                assert!(
                    stored.is_none(),
                    "cancelled migration restored authenticated profile"
                );
            } else {
                assert_eq!(stored.unwrap().settings.interface_language, "fr");
            }
            assert!(
                waited,
                "cancelled start released gate before commit finished"
            );
            later.sign_out().await.unwrap();
        });
    }
}

async fn later_write(session: &mut CoreSession, sign_out: bool) {
    if sign_out {
        session.sign_out().await.unwrap();
    } else {
        let mut profile = session.profile().as_core().clone();
        profile.settings.interface_language = "fr".into();
        PanoramaEnv::set_storage(PROFILE_STORAGE_KEY, Some(&profile))
            .await
            .unwrap();
    }
}

#[test]
fn timed_out_authenticated_login_returns_on_deadline_and_retry_waits_for_cleanup() {
    let api = MockApi::start();
    let fixture = TestEnv::memory(Some(api.base.clone()));
    fixture.runtime.block_on(async {
        let mut session = CoreSession::start().await.unwrap();
        session
            .sign_in(EMAIL.into(), PASSWORD.into())
            .await
            .unwrap();
        api.stall_login.store(true, Ordering::Relaxed);
        api.stall_logout.store(true, Ordering::Relaxed);
        session.sign_in_deadline = Duration::from_millis(50);
        let started = tokio::time::Instant::now();
        let result = tokio::time::timeout(
            Duration::from_millis(150),
            session.sign_in(EMAIL.into(), PASSWORD.into()),
        )
        .await;
        if result.is_err() {
            api.stall_login.store(false, Ordering::Relaxed);
            api.stall_logout.store(false, Ordering::Relaxed);
            session.sign_out().await.unwrap();
        }
        let error = result
            .expect("authentication error must return at deadline")
            .unwrap_err();
        assert_eq!(error.kind, crate::stremio::CoreErrorKind::Timeout);
        assert!(started.elapsed() < Duration::from_millis(150));
        assert!(!session.is_signed_in());
        wait_for_logout(&api).await;
        api.stall_login.store(false, Ordering::Relaxed);
        session.sign_in_deadline = Duration::from_millis(500);
        let mut retry = Box::pin(session.sign_in(EMAIL.into(), PASSWORD.into()));
        let early = tokio::time::timeout(Duration::from_millis(10), &mut retry).await;
        let login_calls = api
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|path| *path == "/api/login")
            .count();
        api.stall_logout.store(false, Ordering::Relaxed);
        let waited = early.is_err();
        let result = match early {
            Ok(result) => result,
            Err(_) => retry.await,
        };
        assert!(waited, "retry bypassed background logout");
        assert_eq!(
            login_calls, 2,
            "retry submitted login before cleanup finished"
        );
        assert!(result.unwrap().is_signed_in());
        let profile = PanoramaEnv::get_storage::<Profile>(PROFILE_STORAGE_KEY)
            .await
            .unwrap()
            .unwrap();
        assert!(profile.auth.is_some());
    });
}

#[test]
fn dropping_timed_out_session_keeps_restart_behind_background_logout() {
    let api = MockApi::start();
    let fixture = TestEnv::memory(Some(api.base.clone()));
    fixture.runtime.block_on(async {
        let mut session = CoreSession::start().await.unwrap();
        session
            .sign_in(EMAIL.into(), PASSWORD.into())
            .await
            .unwrap();
        api.stall_login.store(true, Ordering::Relaxed);
        api.stall_logout.store(true, Ordering::Relaxed);
        session.sign_in_deadline = Duration::from_millis(50);
        let result = tokio::time::timeout(
            Duration::from_millis(150),
            session.sign_in(EMAIL.into(), PASSWORD.into()),
        )
        .await;
        if result.is_err() {
            api.stall_login.store(false, Ordering::Relaxed);
            api.stall_logout.store(false, Ordering::Relaxed);
            session.sign_out().await.unwrap();
        }
        assert_eq!(
            result.unwrap().unwrap_err().kind,
            crate::stremio::CoreErrorKind::Timeout
        );
        wait_for_logout(&api).await;
        drop(session);
        let mut restart = Box::pin(CoreSession::start());
        let early = tokio::time::timeout(Duration::from_millis(10), &mut restart).await;
        api.stall_login.store(false, Ordering::Relaxed);
        api.stall_logout.store(false, Ordering::Relaxed);
        let waited = early.is_err();
        let mut session = match early {
            Ok(result) => result.unwrap(),
            Err(_) => restart.await.unwrap(),
        };
        assert!(waited, "restart bypassed background logout");
        assert!(!session.is_signed_in());
        assert!(
            PanoramaEnv::get_storage::<Profile>(PROFILE_STORAGE_KEY)
                .await
                .unwrap()
                .is_none()
        );
        session
            .sign_in(EMAIL.into(), PASSWORD.into())
            .await
            .unwrap();
        assert!(session.is_signed_in());
    });
}

async fn wait_for_logout(api: &MockApi) {
    tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            if api
                .calls
                .lock()
                .unwrap()
                .iter()
                .any(|path| path == "/api/logout")
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
