use crate::{
    button::{Button, Icon, Size},
    film_card::fade_in,
    film_grid::{NextCard, PreviousCard},
    home_layout::ViewSettings,
    image_cache::CachedImage,
    routes::home::{Home, lineup::Featured},
    theme::{
        Column, Theme, caps,
        scrim::{Edge, scrim},
        typography::{CREDIT_LEAD, CREDIT_NAME, CREDIT_PLACE, DISPLAY, TypeStyle},
    },
};
use gpui::{
    Context, Div, FocusHandle, FontFeatures, RenderImage, Window, div, img, prelude::*, px,
    relative,
};
use std::sync::Arc;

const TOP: f32 = 72.0;
const ORIGINAL_GAP: f32 = 6.0;
const CREDITS_GAP: f32 = 16.0;
const PLACE_GAP: f32 = 2.0;
const ACTIONS_GAP: f32 = 28.0;
pub const ROW_GAP: f32 = 56.0;
pub const FOLD_PAD: f32 = 48.0;
/// Share of the window the hero takes, leaving the Continue row in view beneath it.
const HERO_SHARE: f32 = 0.72;
/// The film page has more room than the home hero, so its logo cap is larger. The player
/// asks for the same size, so its title reuses the decoded image.
pub const FILM_LOGO: (f32, f32) = (280.0, 112.0);
pub const LOGO: (f32, f32) = (200.0, 80.0);
/// Largest text title; the logo cap keeps image titles to the same weight on the page.
const TITLE_MAX: f32 = 44.0;

pub struct HeroFocus {
    pub primary: FocusHandle,
    pub save: FocusHandle,
}

impl HeroFocus {
    pub fn new(cx: &mut gpui::App) -> Self {
        Self {
            primary: cx.focus_handle(),
            save: cx.focus_handle(),
        }
    }
    /// The last action in Tab order: the watchlist toggle when it is shown.
    pub fn last(&self, signed_in: bool) -> &FocusHandle {
        if signed_in { &self.save } else { &self.primary }
    }
}

pub struct HeroState {
    pub logo: Option<CachedImage>,
    /// A logo is on its way, so the text title is held back rather than swapped.
    pub logo_pending: bool,
    pub signed_in: bool,
    pub saved: bool,
    pub saving: bool,
}

/// Display type scaled down on narrow windows, where large titles crowd the column.
pub fn title_style(width: f32) -> TypeStyle {
    TypeStyle {
        size: (width * 0.032).clamp(32.0, TITLE_MAX),
        ..DISPLAY
    }
}

/// Pill height for the window: compact under 700 px so the actions fit the column.
fn compact(width: f32) -> bool {
    width <= 700.0
}

/// Height of the hero: most of the window, or more when the title block needs it. The title
/// is budgeted at two lines (it is clamped there) and the block sits at the bottom, so a
/// one-line title just leaves more backdrop above it.
pub fn height(view: ViewSettings) -> f32 {
    let title = title_style(view.width);
    let line = |style: TypeStyle| style.size * style.line_height;
    let block = (2.0 * title.size * title.line_height).max(LOGO.1)
        + ORIGINAL_GAP
        + line(CREDIT_NAME)
        + CREDITS_GAP
        + line(CREDIT_NAME)
        + PLACE_GAP
        + line(CREDIT_PLACE)
        + ACTIONS_GAP
        + if compact(view.width) {
            Size::Compact
        } else {
            Size::Regular
        }
        .height();
    (view.height * HERO_SHARE).max(TOP + block + ROW_GAP)
}

/// Height of the first viewport: the hero, then the Continue row on the canvas below it.
pub fn fold(view: ViewSettings, row_height: Option<f32>) -> f32 {
    height(view) + row_height.map_or(0.0, |row| row + FOLD_PAD)
}

/// Full-width artwork behind the hero, from x = 0 so it runs under the sidebar.
/// With no image the canvas shows through and the layout stays put.
pub fn backdrop(art: Option<(CachedImage, bool)>, bounds: (f32, f32), reduced: bool) -> Div {
    let frame = div().absolute().inset_0();
    match art {
        Some((image, poster)) => {
            let fade = fade_in(&image, reduced);
            frame.child(cover(image.image, bounds, poster, 1.0).opacity(fade))
        }
        None => frame,
    }
}

/// Canvas-coloured gradients that keep the title, actions and row legible on any still: a left
/// scrim behind the hero copy, and a bottom scrim that lands exactly on the canvas so the
/// catalog below continues without a seam.
pub fn scrims(theme: Theme, col: Column, width: f32) -> Div {
    // The copy spans up to 760 px from the gutter; the left scrim covers it on narrow windows,
    // where 65% of the width alone would leave the title's end on bare artwork.
    let left = (0.65 * width).max(col.left + col.width.min(760.0) * 0.8);
    div()
        .absolute()
        .inset_0()
        .child(scrim(theme, Edge::Left, px(left), 0.92))
        .child(scrim(theme, Edge::Bottom, relative(0.6), 1.0))
}

pub fn render(
    featured: Featured<'_>,
    state: HeroState,
    focus: &HeroFocus,
    view: ViewSettings,
    window: &Window,
    cx: &mut Context<Home>,
) -> Div {
    let theme = view.theme;
    let film = featured.film();
    let col = view.column();
    let mut title = div().flex().flex_col().max_w(px(col.width.min(760.0)));
    if let Some(logo) = state.logo {
        let size = logo.image.size(0);
        let width = size.width.0 as f32 / window.scale_factor();
        let height = size.height.0 as f32 / window.scale_factor();
        let ratio = (LOGO.0 / width).min(LOGO.1 / height).min(1.0);
        title = title.child(img(logo.image).w(px(width * ratio)).h(px(height * ratio)));
    } else if !state.logo_pending {
        title = title.child(
            title_style(view.width)
                .apply(div())
                .line_clamp(2)
                .child(caps(&film.name)),
        );
    }
    title = title.child(byline(film));

    let compact = compact(view.width);
    let size = |button: Button| if compact { button.compact() } else { button };
    let play = film.clone();
    let resume = featured.resume();
    let left = resume.and_then(|point| {
        crate::film_display::format_time_left(point.offset_secs, Some(point.duration_secs))
    });
    let mut primary = Button::primary(
        "hero-primary",
        left.clone()
            .unwrap_or_else(|| featured.action_label().to_owned()),
    );
    if let (Some(point), Some(_)) = (resume, &left) {
        primary = primary.progress((point.offset_secs / point.duration_secs) as f32);
    }
    let primary = size(
        primary
            .leading(Icon::Play)
            .aria_label(match &left {
                Some(time) => format!("Continue {}, {time} left", film.name),
                None => format!("{} {}", featured.action_label(), film.name),
            })
            .focus(&focus.primary, view.keyboard)
            .tab_stop(view.active)
            .on_click(
                cx.listener(move |home, _, window, cx| home.play(play.clone(), resume, window, cx)),
            ),
    );
    let save = state.signed_in.then(|| {
        let target = film.clone();
        size(
            Button::round(
                "hero-save",
                if state.saved { Icon::Check } else { Icon::Plus },
                if state.saved {
                    format!("Remove {} from watchlist", film.name)
                } else {
                    format!("Add {} to watchlist", film.name)
                },
            )
            .busy(state.saving)
            .reduced_motion(view.reduced)
            .focus(&focus.save, view.keyboard)
            .tab_stop(view.active)
            .on_click(cx.listener(move |home, _, _, cx| home.toggle_saved(&target, cx))),
        )
    });
    let last = |button: Button| {
        div()
            .key_context("PanoramaCard")
            .on_action(
                cx.listener(|home, _: &NextCard, window, cx| home.focus_below_hero(window, cx)),
            )
            .on_action(cx.listener(|_, _: &PreviousCard, window, cx| {
                window.focus_prev(cx);
                cx.stop_propagation();
            }))
            .child(button)
    };
    let actions = div()
        .mt(px(ACTIONS_GAP))
        .flex()
        .items_center()
        .gap(px(12.0));
    let actions = match save {
        Some(save) => actions.child(primary).child(last(save)),
        None => actions.child(last(primary)),
    };
    div()
        .text_color(theme.ink)
        .flex()
        .flex_col()
        .child(title)
        .child(actions)
}

/// Everything under a film's display title, shared by the home hero and the film page: the
/// original-language title when it differs, then director, origin and year. The credit rows keep
/// their height while empty, so details arriving after the catalog card never move the title.
pub fn byline(film: &panorama_core::addons::FilmDetails) -> Div {
    let line = |style: TypeStyle| px(style.size * style.line_height);
    let place = [film.origin_country.as_deref(), film.release_info.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ");
    let original = film
        .original_name
        .as_deref()
        .filter(|original| !original.eq_ignore_ascii_case(&film.name))
        .map(|original| {
            CREDIT_NAME
                .apply(div())
                .mt(px(ORIGINAL_GAP))
                .line_clamp(1)
                .child(caps(original))
        });
    div()
        .flex()
        .flex_col()
        .children(original)
        .font_features(FontFeatures(Arc::new(vec![
            ("tnum".into(), 1),
            ("kern".into(), 1),
        ])))
        .child(
            div()
                .mt(px(CREDITS_GAP))
                .min_h(line(CREDIT_NAME))
                .flex()
                .items_baseline()
                .gap(px(6.0))
                .when(!film.director.is_empty(), |row| {
                    row.child(CREDIT_LEAD.apply(div()).child("Directed by"))
                        .child(CREDIT_NAME.apply(div()).child(film.director.join(", ")))
                }),
        )
        .child(
            CREDIT_PLACE
                .apply(div())
                .mt(px(PLACE_GAP))
                .min_h(line(CREDIT_PLACE))
                .child(place),
        )
}

/// CSS cover with explicit 50% 28% positioning for poster fallback.
pub fn cover(image: Arc<RenderImage>, bounds: (f32, f32), poster: bool, scale: f32) -> Div {
    let size = image.size(0);
    let ratio = (bounds.0 / size.width.0 as f32).max(bounds.1 / size.height.0 as f32) * scale;
    let width = size.width.0 as f32 * ratio;
    let height = size.height.0 as f32 * ratio;
    div().absolute().inset_0().overflow_hidden().child(
        img(image)
            .absolute()
            .w(px(width))
            .h(px(height))
            .left(px((bounds.0 - width) * 0.5))
            .top(px((bounds.1 - height) * if poster { 0.28 } else { 0.5 })),
    )
}
