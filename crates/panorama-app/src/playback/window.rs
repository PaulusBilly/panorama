use crate::app_state::AppState;
use gpui::{App, Entity, Window};

pub fn close(state: Entity<AppState>, window: &mut Window, cx: &mut App) {
    if state.read(cx).closing {
        return;
    }
    let done = state.update(cx, |state, _| {
        state.closing = true;
        state.playback_host.as_ref().map(|host| host.shutdown())
    });
    if let Some(done) = done {
        let handle = window.window_handle();
        cx.spawn(async move |cx| {
            wait_for_shutdown(done, |duration| cx.background_executor().timer(duration)).await;
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        })
        .detach();
    } else {
        window.remove_window();
    }
}

pub async fn wait_for_shutdown<F: std::future::Future<Output = ()>>(
    done: tokio::sync::oneshot::Receiver<()>,
    timer: impl FnOnce(std::time::Duration) -> F,
) {
    let deadline = timer(std::time::Duration::from_secs(3));
    futures::pin_mut!(done, deadline);
    let _ = futures::future::select(done, deadline).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::FutureExt;
    use std::time::Duration;

    #[tokio::test]
    async fn stuck_shutdown_releases_window_at_three_seconds() {
        let (_host, done) = tokio::sync::oneshot::channel();
        let (deadline, elapsed) = tokio::sync::oneshot::channel();
        let wait = wait_for_shutdown(done, |duration| {
            assert_eq!(duration, Duration::from_secs(3));
            elapsed.map(|_| ())
        });
        futures::pin_mut!(wait);
        assert!(futures::poll!(&mut wait).is_pending());
        deadline.send(()).unwrap();
        assert!(futures::poll!(&mut wait).is_ready());
    }

    #[tokio::test]
    async fn finished_shutdown_releases_window_before_deadline() {
        let (host, done) = tokio::sync::oneshot::channel();
        let wait = wait_for_shutdown(done, |_| futures::future::pending());
        futures::pin_mut!(wait);
        assert!(futures::poll!(&mut wait).is_pending());
        host.send(()).unwrap();
        assert!(futures::poll!(&mut wait).is_ready());
    }
}
