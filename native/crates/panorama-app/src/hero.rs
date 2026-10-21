//! Home's first viewport: the featured film's backdrop, its scrims and the title block over them.
use crate::{
    button::{Button, Icon},
    film_card::fade_in,
    film_grid::{GridHost, NextCard, PreviousCard},
    home_layout::ViewSettings,
    image_cache::CachedImage,
    routes::home::{Home, lineup::Featured},
    theme::{
        Column, Theme, caps, credits,
        scrim::{Edge, scrim},
        typography::{DISPLAY, HEADING, TypeStyle},
    },
};
use gpui::{Context, Div, FocusHandle, RenderImage, Window, div, img, prelude::*, px, relative};
use std::sync::Arc;

/// Top of the hero block at its tallest, clear of the titlebar.
const TOP: f32 = 72.0;
/// Title to credits.
const CREDITS_GAP: f32 = 12.0;
/// Credits to actions.
const ACTIONS_GAP: f32 = 32.0;
/// Hero block to the Continue row.
pub const ROW_GAP: f32 = 56.0;
/// Hero block to the fold when there is no Continue row.
const BARE_GAP: f32 = 96.0;
/// Continue row to the fold.
pub const FOLD_PAD: f32 = 48.0;
/// Largest logo image, in logical pixels.
pub const LOGO: (f32, f32) = (310.0, 128.0);

/// Focus handles for the hero's three actions; one set per Home entry.
pub struct HeroFocus {
    pub primary: FocusHandle,
    pub info: FocusHandle,
    pub save: FocusHandle,
}

impl HeroFocus {
    pub fn new(cx: &mut gpui::App) -> Self {
        Self {
            primary: cx.focus_handle(),
            info: cx.focus_handle(),
            save: cx.focus_handle(),
        }
    }
    /// The last action in Tab order: the watchlist toggle when it is shown.
    pub fn last(&self, signed_in: bool) -> &FocusHandle {
        if signed_in { &self.save } else { &self.info }
    }
}

/// What the hero shows besides the film.
pub struct HeroState {
    /// The film's logo, once loaded.
    pub logo: Option<CachedImage>,
    /// A logo is on its way, so the text title is held back rather than swapped.
    pub logo_pending: bool,
    pub signed_in: bool,
    pub saved: bool,
    pub saving: bool,
}

/// Display type scaled down on narrow windows, where 52 px titles crowd the column.
fn title_style(width: f32) -> TypeStyle {
    TypeStyle {
        size: (width * 0.04).clamp(36.0, DISPLAY.size),
        ..DISPLAY
    }
}

/// Pill height for the window: compact under 700 px so three actions fit the column.
fn compact(width: f32) -> bool {
    width <= 700.0
}

/// Height of the first viewport: the window, or more when the hero and row need it. The title
/// is budgeted at two lines (it is clamped there) and the block sits at the bottom, so a
/// one-line title just leaves more backdrop above it.
pub fn fold(view: ViewSettings, row_height: Option<f32>) -> f32 {
    let title = title_style(view.width);
    let block = (2.0 * title.size * title.line_height).max(LOGO.1)
        + CREDITS_GAP
        + HEADING.size * HEADING.line_height
        + ACTIONS_GAP
        + if compact(view.width) { 40.0 } else { 52.0 };
    view.height.max(TOP + block + below(row_height))
}

/// Space under the hero block: the Continue row and its padding, or a bare gap.
pub fn below(row_height: Option<f32>) -> f32 {
    row_height.map_or(BARE_GAP, |row| ROW_GAP + row + FOLD_PAD)
}

/// Full-width artwork behind the first viewport, from x = 0 so it runs under the sidebar.
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

/// Title (logo if present, else display caps), credits and the action row, left on the column.
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
    // Reserved even while empty, so credits arriving with details never move the title.
    title = title.child(
        HEADING
            .apply(credits(film, theme))
            .min_h(px(HEADING.size * HEADING.line_height))
            .mt(px(CREDITS_GAP)),
    );

    let compact = compact(view.width);
    let size = |button: Button| if compact { button.compact() } else { button };
    let open = film.clone();
    let play = film.clone();
    let resume = featured.resume();
    let primary = size(
        Button::primary("hero-primary", featured.action_label())
            .leading(Icon::Play)
            .aria_label(format!("{} {}", featured.action_label(), film.name))
            .focus(&focus.primary, view.keyboard)
            .tab_stop(view.active)
            .on_click(cx.listener(move |home, _, window, cx| home.play(play.clone(), resume, window, cx))),
    );
    let info = size(
        Button::glass("hero-info", "More info")
            .aria_label(format!("More info about {}", film.name))
            .focus(&focus.info, view.keyboard)
            .tab_stop(view.active)
            .on_click(
                cx.listener(move |home, _, window, cx| home.open_film(open.clone(), window, cx)),
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
    // Tab leaves the hero from its last action into the Continue row or the catalog.
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
        .gap(px(12.0))
        .child(primary);
    let actions = match save {
        Some(save) => actions.child(info).child(last(save)),
        None => actions.child(last(info)),
    };
    div()
        .text_color(theme.ink)
        .flex()
        .flex_col()
        .child(title)
        .child(actions)
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
