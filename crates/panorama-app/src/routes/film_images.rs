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
    /// The backdrop (or poster fallback), whether it is the poster, and where it came from.
    pub(super) fn artwork(
        &self,
        view: ViewSettings,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> (Option<CachedImage>, bool, Option<url::Url>) {
        let Some(meta) = &self.metadata else {
            return (None, false, None);
        };
        for (url, poster) in [(&meta.background, false), (&meta.poster, true)] {
            if let Some(url) = url {
                let (image, failed) = self.image(url, (view.width, view.height), view, window, cx);
                if !failed {
                    return (image, poster, Some(url.clone()));
                }
            }
        }
        (None, false, None)
    }
    pub(super) fn blurred(
        &self,
        url: &url::Url,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<CachedImage> {
        let services = self.state.read(cx).services.clone()?;
        self.images.update(cx, |images, cx| {
            images.request_blurred(url.as_str(), &services, window, cx)
        })
    }
}
