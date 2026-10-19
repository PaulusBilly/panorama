use super::*;
use crate::image_cache::render_image;
use futures::{StreamExt, channel::mpsc};

impl Film {
    pub(super) fn ensure_overlay(
        &mut self,
        view: ViewSettings,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let scale = window.scale_factor();
        let size = (
            (view.width * scale).ceil().min(4096.0) as u32,
            (view.height * scale).ceil().min(4096.0) as u32,
        );
        if size == self.overlay_size {
            return;
        }
        let Some(services) = self.state.read(cx).services.clone() else {
            return;
        };
        self.overlay_size = size;
        let (sender, mut receiver) = mpsc::unbounded();
        let job = Job(services.runtime.spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                render_image(crate::film_overlay::overlay(size.0, size.1))
            })
            .await;
            let _ = sender.unbounded_send(result.ok().and_then(Result::ok));
        }));
        self.overlay_task = Some(cx.spawn_in(window, async move |entity, cx| {
            let _job = job;
            if let Some(image) = receiver.next().await {
                let _ = cx.update(|window, cx| {
                    entity.update(cx, |film, cx| {
                        if let Some(old) = film.overlay.take() {
                            cx.drop_image(old, Some(window));
                        }
                        film.overlay = image;
                        cx.notify();
                    })
                });
            }
        }));
    }
    pub(super) fn image(
        &self,
        url: &url::Url,
        bounds: (f32, f32),
        view: ViewSettings,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> (Option<CachedImage>, bool) {
        if !view.active {
            return (None, false);
        }
        let failed = self.images.update(cx, |images, _| {
            images.failed(url.as_str(), bounds, window.scale_factor())
        });
        if failed {
            return (None, true);
        }
        let image = self.state.read(cx).services.clone().and_then(|services| {
            self.images.update(cx, |images, cx| {
                images.request(url.as_str(), bounds, &services, window, cx)
            })
        });
        (image, false)
    }
    pub(super) fn artwork(
        &self,
        view: ViewSettings,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> (Option<CachedImage>, bool) {
        let Some(meta) = &self.metadata else {
            return (None, false);
        };
        for (url, poster) in [(&meta.background, false), (&meta.poster, true)] {
            if let Some(url) = url {
                let (image, failed) = self.image(url, (view.width, view.height), view, window, cx);
                if !failed {
                    return (image, poster);
                }
            }
        }
        (None, false)
    }
}
