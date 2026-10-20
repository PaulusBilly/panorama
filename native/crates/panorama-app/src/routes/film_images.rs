use super::*;
use crate::image_cache::render_image;

impl Film {
    /// The overlay is a smooth gradient, so it is rasterized at 1/8 logical size and stretched;
    /// that is cheap enough to draw on the first frame instead of arriving after the page.
    pub(super) fn ensure_overlay(
        &mut self,
        view: ViewSettings,
        window: &mut Window,
        cx: &mut gpui::App,
    ) {
        let size = (
            (view.width / 8.0).ceil().clamp(8.0, 512.0) as u32,
            (view.height / 8.0).ceil().clamp(8.0, 512.0) as u32,
        );
        if size == self.overlay_size && self.overlay.is_some() {
            return;
        }
        let Ok(image) = render_image(crate::film_overlay::overlay(size.0, size.1)) else {
            return;
        };
        self.overlay_size = size;
        if let Some(old) = self.overlay.replace(image) {
            cx.drop_image(old, Some(window));
        }
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
