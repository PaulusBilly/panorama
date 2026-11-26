use super::*;

/// The card's artwork comes first so the banner never changes under the page, whatever the
/// details bring. Backdrops outrank posters, and the flag marks a poster.
fn banners<'a>(
    card: Option<&'a FilmDetails>,
    details: Option<&'a FilmDetails>,
) -> impl Iterator<Item = (&'a url::Url, bool)> {
    let backdrop = |film: Option<&'a FilmDetails>| film?.background.as_ref();
    let poster = |film: Option<&'a FilmDetails>| film?.poster.as_ref();
    [
        (backdrop(card), false),
        (backdrop(details), false),
        (poster(card), true),
        (poster(details), true),
    ]
    .into_iter()
    .filter_map(|(url, poster)| Some((url?, poster)))
}

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
        for (url, poster) in banners(self.card.as_ref(), self.metadata.as_ref()) {
            let (image, failed) = self.image(url, (view.width, view.height), view, window, cx);
            if !failed {
                return (image, poster, Some(url.clone()));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_card_banner_outlives_the_details() {
        let url = |path: &str| url::Url::parse(&format!("https://example.com/{path}")).unwrap();
        let mut card = crate::fixtures::films().remove(0);
        card.background = Some(url("card"));
        card.poster = Some(url("card-poster"));
        let mut details = card.clone();
        details.background = Some(url("details"));
        details.poster = Some(url("details-poster"));
        fn order(card: Option<&FilmDetails>, details: Option<&FilmDetails>) -> Vec<(String, bool)> {
            banners(card, details)
                .map(|(url, poster)| (url.path().to_owned(), poster))
                .collect()
        }
        assert_eq!(
            order(Some(&card), Some(&details))[0],
            ("/card".into(), false)
        );
        assert_eq!(order(None, Some(&details))[0], ("/details".into(), false));
        card.background = None;
        assert_eq!(
            order(Some(&card), Some(&details))[0],
            ("/details".into(), false)
        );
        details.background = None;
        assert_eq!(
            order(Some(&card), Some(&details))[0],
            ("/card-poster".into(), true)
        );
    }
}
