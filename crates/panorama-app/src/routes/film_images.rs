use super::*;

impl Film {
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
    /// The backdrop (or poster fallback), whether it is the poster, and whether it is still loading.
    pub(super) fn artwork(
        &self,
        view: ViewSettings,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> (Option<CachedImage>, bool, bool) {
        let Some(meta) = &self.metadata else {
            return (None, false, false);
        };
        for (url, poster) in [(&meta.background, false), (&meta.poster, true)] {
            if let Some(url) = url {
                let (image, failed) = self.image(url, (view.width, view.height), view, window, cx);
                if !failed {
                    let pending = image.is_none();
                    return (image, poster, pending);
                }
            }
        }
        (None, false, false)
    }
}
