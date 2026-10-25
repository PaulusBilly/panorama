//! Cast & Crew and Awards & Festivals, the sections under the film page's hero.
use super::*;
use crate::{
    carousel::Carousel,
    theme::{
        Theme, Typography, caps, focus_ring,
        typography::{CAPTION, CREDIT_NAME, CREDIT_PLACE, SECTION, TITLE, TypeStyle},
    },
};
use gpui::{Div, ObjectFit, StyledImage, div, img, prelude::*, px, svg};
use panorama_core::credits::{Award, AwardResult, Event, Person};

const SECTION_GAP: f32 = 72.0;
const HEADING_GAP: f32 = 16.0;
const CARD_GAP: f32 = 6.0;
const CARD_MIN: f32 = 150.0;
/// Portrait height over width.
const PHOTO_RATIO: f32 = 1.16;
/// Room for a two-line name and the role under it.
const CARD_TEXT: f32 = 88.0;
const TILE_GAP: f32 = 40.0;
const TILE_MIN: f32 = 240.0;
/// Festival marks sit in a row this tall above the name, as on MUBI.
const MARK_HEIGHT: f32 = 36.0;
/// Each mark fits inside this box, small and nearly square like MUBI's, so a wide wordmark
/// reads no louder than a narrow emblem.
const MARK: (f32, f32) = (44.0, 32.0);

/// How many `min`-wide items fit across `width`, at least two and at most `max`.
fn columns(width: f32, min: f32, gap: f32, max: usize) -> usize {
    (((width + gap) / (min + gap)).floor() as usize).clamp(2, max)
}

fn span(width: f32, count: usize, gap: f32) -> f32 {
    (width - gap * (count - 1) as f32) / count as f32
}

fn initials(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().collect();
    [words.first(), words.last().filter(|_| words.len() > 1)]
        .into_iter()
        .flatten()
        .filter_map(|word| word.chars().next())
        .flat_map(char::to_uppercase)
        .collect()
}

/// Space after the year and after the result, so the three columns read as evenly set.
const GUTTER: f32 = 14.0;
/// Room for a two-line event name.
const NAME_HEIGHT: f32 = 16.0 * 1.3 * 2.0;
/// Lines a tile shows before "+N More".
const TILE_LINES: usize = 2;

/// Every entry for one event, across all the years the film was there.
struct Ceremony<'a> {
    body: &'a str,
    /// The newest year, which the festival page opens at.
    year: Option<i32>,
    /// Every year, newest first.
    years: Vec<i32>,
    event: Option<&'a Event>,
    /// Newest year first, best result within it.
    awards: Vec<&'a Award>,
}

/// One tile per event, so a festival the film returned to is one tile: the tiles with a win
/// first, then the best-known events, then the newest.
fn ceremonies(awards: &[Award]) -> Vec<Ceremony<'_>> {
    let mut groups: Vec<Ceremony> = vec![];
    for award in awards {
        match groups.iter_mut().find(|group| group.body == award.body) {
            Some(group) => {
                group.year = group.year.max(award.year);
                group.awards.push(award);
            }
            None => groups.push(Ceremony {
                body: &award.body,
                year: award.year,
                years: vec![],
                event: award.event.as_ref(),
                awards: vec![award],
            }),
        }
    }
    for group in &mut groups {
        group
            .awards
            .sort_by(|a, b| b.year.cmp(&a.year).then(a.result.cmp(&b.result)));
        // An unnamed entry beside a named one with the same result and year says nothing more:
        // "Selection · Cannes Classics" and a bare "Selection" are one line.
        let named: Vec<(Option<i32>, AwardResult)> = group
            .awards
            .iter()
            .filter(|award| award.category.is_some())
            .map(|award| (award.year, award.result))
            .collect();
        group.awards.retain(|award| {
            award.category.is_some() || !named.contains(&(award.year, award.result))
        });
        group.years = group.awards.iter().filter_map(|award| award.year).collect();
        group.years.dedup();
    }
    // ponytail: MUBI numbered its events roughly by prominence (Cannes 1, Berlinale 3, Oscars
    // 4), so the id stands in for prestige; Wikidata's awards, which have none, follow.
    let best = |group: &Ceremony| group.awards.iter().map(|award| award.result).min();
    groups.sort_by(|a, b| {
        best(a)
            .cmp(&best(b))
            .then_with(|| {
                let id = |group: &Ceremony| group.event.map_or(u64::MAX, |event| event.id);
                id(a).cmp(&id(b))
            })
            .then(b.year.cmp(&a.year))
    });
    groups
}

/// The laid-out width of `text` in `style`, from the bundled face's advances, rounded up so
/// a column sized to it never wraps.
fn text_width(text: &str, style: TypeStyle, window: &Window) -> f32 {
    let system = window.text_system();
    let mut font = crate::assets::font();
    crate::assets::weighted(&mut font, gpui::FontWeight(style.weight));
    let id = system.resolve_font(&font);
    let width: f32 = text
        .chars()
        .filter_map(|c| system.advance(id, px(style.size), c).ok())
        .map(|advance| f32::from(advance.width))
        .sum();
    width.ceil()
}

/// Room around a rail's items inside its scroll area, which clips, for the outer focus ring.
const RING: f32 = 4.0;

/// A [`Carousel`] row across the full window: the first item sits on the column, items scrolled
/// past stay visible in the gutter, and the last stops where the column does.
fn rail(
    id: &'static str,
    items: Vec<gpui::AnyElement>,
    gap: f32,
    carousel: &Carousel,
    view: ViewSettings,
    on_wheel: impl Fn(&gpui::ScrollWheelEvent, &mut Window, &mut gpui::App) + 'static,
) -> gpui::Stateful<Div> {
    let col = view.column();
    div()
        .id(id)
        .flex()
        .items_start()
        .gap(px(gap))
        .py(px(RING))
        .my(px(-RING))
        .w(px(view.width))
        .overflow_x_scroll()
        // Only sideways gestures move the row, so scrolling down over it moves the page.
        .restrict_scroll_to_axis()
        .track_scroll(&carousel.scroll)
        .on_scroll_wheel(on_wheel)
        .child(div().flex_shrink_0().w(px(col.left - gap)))
        .children(items)
        .child(
            div()
                .flex_shrink_0()
                .w(px((view.width - col.left - col.width - gap).max(0.0))),
        )
}

fn heading(title: &str, theme: Theme) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .min_h(px(36.0))
        .mb(px(HEADING_GAP))
        .child(
            SECTION
                .apply(div())
                .text_color(theme.ink)
                .child(caps(title)),
        )
}

fn source_note(text: &'static str, theme: Theme) -> Div {
    CAPTION
        .apply(div())
        .mt(px(16.0))
        .text_color(theme.ink_muted)
        .child(text)
}

impl Film {
    /// Both sections, or nothing while neither has arrived.
    pub(super) fn credits_sections(
        &mut self,
        view: ViewSettings,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        // Only a TMDB-backed metadata addon sends people; with any other the row stays out.
        let people = self
            .metadata
            .as_ref()
            .map(|meta| meta.people.clone())
            .unwrap_or_default();
        if people.is_empty() && self.awards.is_empty() {
            return None;
        }
        let col = view.column();
        let mut sections = div()
            .w(px(view.width))
            .pt(px(SECTION_GAP))
            .pb(px(SECTION_GAP + 24.0))
            .flex()
            .flex_col()
            .gap(px(SECTION_GAP));
        if !people.is_empty() {
            sections = sections.child(self.people_section(&people, col.width, view, window, cx));
        }
        if !self.awards.is_empty() {
            sections = sections.child(self.awards_section(col.width, view, window, cx));
        }
        Some(sections)
    }

    fn people_section(
        &mut self,
        all: &[Person],
        width: f32,
        view: ViewSettings,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = view.theme;
        let count = columns(width, CARD_MIN, CARD_GAP, 6);
        let card = span(width, count, CARD_GAP);
        self.people_rail.settle(card + CARD_GAP, view.reduced);
        let cards: Vec<gpui::AnyElement> = all
            .iter()
            .map(|person| {
                self.person_card(person, card, view, window, cx)
                    .flex_shrink_0()
                    .into_any_element()
            })
            .collect();
        let col = view.column();
        div()
            .flex()
            .flex_col()
            .child(
                heading("Cast & Crew", theme)
                    .ml(px(col.left))
                    .w(px(col.width)),
            )
            .child(rail(
                "film-people",
                cards,
                CARD_GAP,
                &self.people_rail,
                view,
                cx.listener(|film, _: &gpui::ScrollWheelEvent, _, cx| {
                    film.people_rail.wheel();
                    cx.notify();
                }),
            ))
            .child(source_note("Cast and crew from TMDB", theme).ml(px(col.left)))
    }

    fn person_card(
        &self,
        person: &Person,
        width: f32,
        view: ViewSettings,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = view.theme;
        let height = (width * PHOTO_RATIO).round();
        let image = person
            .photo
            .as_ref()
            .and_then(|url| self.image(url, (width, height), view, window, cx).0);
        let photo = div()
            .w(px(width))
            .h(px(height))
            .overflow_hidden()
            .bg(theme.surface_strong)
            .flex()
            .items_center()
            .justify_center();
        let photo = match image {
            // Black and white, so portraits shot every which way read as one set.
            Some(image) => photo.child(
                img(image.image)
                    .size_full()
                    .object_fit(ObjectFit::Cover)
                    .grayscale(true),
            ),
            None if person.photo.is_some() => photo,
            None => photo.child(
                TITLE
                    .apply(div())
                    .text_color(theme.ink_muted)
                    .child(initials(&person.name)),
            ),
        };
        div().w(px(width)).flex().flex_col().child(photo).child(
            div()
                .h(px(CARD_TEXT))
                .bg(theme.surface)
                .px(px(16.0))
                .pt(px(14.0))
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    CREDIT_NAME
                        .apply(div())
                        .text_size(px(15.0))
                        .text_color(theme.ink)
                        .line_clamp(2)
                        .child(caps(&person.name)),
                )
                .child(
                    CAPTION
                        .apply(div())
                        .text_color(theme.ink_muted)
                        .truncate()
                        .child(caps(&person.role)),
                ),
        )
    }

    fn awards_section(
        &mut self,
        width: f32,
        view: ViewSettings,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = view.theme;
        let count = columns(width, TILE_MIN, TILE_GAP, 4);
        let tile = span(width, count, TILE_GAP);
        let all = ceremonies(&self.awards);
        let shown = all.len();
        let pitch = tile + TILE_GAP;
        self.awards_rail.settle(pitch, view.reduced);
        if self.award_tiles.len() < shown {
            self.award_tiles.resize_with(shown, || cx.focus_handle());
            self.award_more.resize_with(shown, || cx.focus_handle());
        }
        let tiles: Vec<gpui::AnyElement> = all
            .iter()
            .take(shown)
            .enumerate()
            .map(|(index, ceremony)| {
                let open = self.awards_expanded.contains(&index);
                let lines = ceremony.awards.len();
                let visible = if open { lines } else { lines.min(TILE_LINES) };
                // Columns as wide as this tile's widest year and label, every line counted so
                // opening the tile shifts nothing, with the same gutter after each.
                let year_width = ceremony
                    .years
                    .iter()
                    .map(|year| text_width(&year.to_string(), CREDIT_PLACE, window))
                    .fold(0.0, f32::max);
                let label_width = ceremony
                    .awards
                    .iter()
                    .map(|award| {
                        let label = caps(crate::award_marks::result_label(award.result));
                        text_width(&label, CAPTION, window)
                    })
                    .fold(0.0, f32::max);
                let rows = ceremony.awards[..visible]
                    .iter()
                    .enumerate()
                    .map(|(row, award)| {
                        let ink = if award.result == AwardResult::Won {
                            theme.ink
                        } else {
                            theme.ink_muted
                        };
                        // Each year leads its first line, so a festival the film returned to
                        // reads as a short history.
                        let year = (row == 0 || ceremony.awards[row - 1].year != award.year)
                            .then_some(award.year)
                            .flatten();
                        // Top-aligned, with the caption nudged onto the category's first baseline,
                        // so a category that wraps keeps its label on its first line.
                        div()
                            .flex()
                            .items_start()
                            .gap(px(GUTTER))
                            .child(
                                CREDIT_PLACE
                                    .apply(div())
                                    .flex_none()
                                    .w(px(year_width))
                                    .text_color(theme.ink_muted)
                                    .children(year.map(|year| year.to_string())),
                            )
                            .child(
                                CAPTION
                                    .apply(div())
                                    .flex_none()
                                    .w(px(label_width))
                                    .pt(px(2.5))
                                    .text_color(ink)
                                    .child(caps(crate::award_marks::result_label(award.result))),
                            )
                            .when_some(award.category.clone(), |row, category| {
                                row.child(
                                    CREDIT_PLACE
                                        .apply(div())
                                        .flex_1()
                                        .min_w_0()
                                        .text_color(theme.ink)
                                        // Closed, a long category stops at two lines; "+N More"
                                        // or opening the tile shows it whole.
                                        .when(!open, |category| {
                                            category.line_clamp(2).text_ellipsis()
                                        })
                                        .child(category),
                                )
                            })
                    });
                // The rest of a long tile opens in place; only its own row grows.
                let more = (lines > TILE_LINES).then(|| {
                    let label = if open {
                        "Show Less".to_owned()
                    } else {
                        format!("+{} More", lines - TILE_LINES)
                    };
                    focus_ring(
                        div()
                            .id(("award-more", index))
                            .role(gpui::Role::Button)
                            .aria_label(format!("{label} from {}", ceremony.body))
                            .cursor_pointer()
                            .mt(px(10.0))
                            .label()
                            .text_color(theme.ink_muted)
                            .hover(|style| style.text_color(theme.ink))
                            .child(label)
                            .on_click(cx.listener(move |film, _, _, cx| {
                                if !film.awards_expanded.remove(&index) {
                                    film.awards_expanded.insert(index);
                                }
                                cx.stop_propagation();
                                cx.notify();
                            })),
                        &self.award_more[index],
                        theme,
                        view.active,
                        view.keyboard,
                        0.0,
                        window,
                    )
                });
                // MUBI's logo, else a bundled Commons logo, else a generic award mark, so
                // every tile has one. A logo still loading keeps its row empty.
                let logo = ceremony.event.and_then(|event| event.logo.as_ref());
                let (image, failed) = match logo {
                    Some(url) => self.image(url, MARK, view, window, cx),
                    None => (None, true),
                };
                let mark = match (image, failed) {
                    (Some(image), _) => {
                        // Sized to the logo's own shape, so it sits flush with the name below.
                        let size = image.image.size(0);
                        let aspect = size.width.0 as f32 / size.height.0.max(1) as f32;
                        let width = MARK.0.min(MARK.1 * aspect);
                        Some(
                            img(image.image)
                                .w(px(width))
                                .h(px(width / aspect))
                                .opacity(0.9)
                                .into_any_element(),
                        )
                    }
                    (None, false) => None,
                    (None, true) => Some(match crate::award_marks::mark(ceremony.body) {
                        Some(path) => img(path)
                            .h(px(MARK.1))
                            .max_w(px(MARK.0))
                            .object_fit(ObjectFit::Contain)
                            .opacity(0.9)
                            .into_any_element(),
                        None => svg()
                            .path("award.svg")
                            .size(px(MARK.1 * 0.85))
                            .text_color(theme.ink_muted)
                            .into_any_element(),
                    }),
                };
                let body = div()
                    .flex_shrink_0()
                    .w(px(tile))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(px(MARK_HEIGHT))
                            .mb(px(12.0))
                            .flex()
                            .items_center()
                            .children(mark),
                    )
                    // Two lines tall whatever the name, so every tile's lines start level.
                    .child(
                        CREDIT_NAME
                            .apply(div())
                            .h(px(NAME_HEIGHT))
                            .text_color(theme.ink)
                            .line_clamp(2)
                            .child(caps(ceremony.body)),
                    )
                    .child(
                        div()
                            .mt(px(12.0))
                            .flex()
                            .flex_col()
                            .gap(px(6.0))
                            .children(rows),
                    )
                    .children(more);
                // A MUBI event opens its festival page at this edition.
                let Some(event) = ceremony.event else {
                    return body.into_any_element();
                };
                let route = crate::router::Route::Festival {
                    slug: event.slug.clone(),
                    year: ceremony.year,
                };
                focus_ring(
                    div()
                        .id(("award-tile", index))
                        .flex_shrink_0()
                        .role(gpui::Role::Link)
                        .aria_label(format!("Open {}", ceremony.body))
                        .cursor_pointer()
                        .child(body)
                        .on_click(cx.listener(move |film, _, window, cx| {
                            let route = route.clone();
                            let _ = film
                                .shell
                                .update(cx, |shell, cx| shell.navigate(route, window, cx));
                        })),
                    &self.award_tiles[index],
                    theme,
                    view.active,
                    view.keyboard,
                    0.0,
                    window,
                )
                .into_any_element()
            })
            .collect();
        let source = if self.awards.iter().any(|award| award.event.is_some()) {
            "Awards and festivals from MUBI"
        } else {
            "Awards from Wikidata"
        };
        // A tile reached from the keyboard glides onto the column.
        if view.keyboard
            && let Some(index) = self.award_tiles[..shown]
                .iter()
                .chain(&self.award_more[..shown])
                .position(|focus| focus.is_focused(window))
                .map(|position| position % shown.max(1))
        {
            self.awards_rail.follow(index, pitch);
        }
        let col = view.column();
        div()
            .flex()
            .flex_col()
            .child(
                heading("Awards & Festivals", theme)
                    .ml(px(col.left))
                    .w(px(col.width)),
            )
            .child(rail(
                "film-awards",
                tiles,
                TILE_GAP,
                &self.awards_rail,
                view,
                cx.listener(|film, _: &gpui::ScrollWheelEvent, _, cx| {
                    film.awards_rail.wheel();
                    cx.notify();
                }),
            ))
            .child(
                source_note(source, theme)
                    .ml(px(col.left))
                    .mt(px(TILE_GAP * 0.75)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grids_and_groups() {
        assert_eq!(columns(1240.0, CARD_MIN, CARD_GAP, 6), 6);
        assert_eq!(columns(400.0, CARD_MIN, CARD_GAP, 6), 2);
        assert_eq!(initials("Celia Rowlson-Hall"), "CR");
        assert_eq!(initials("Madonna"), "M");
        let award = |body: &str, year, result| Award {
            body: body.into(),
            category: None,
            year: Some(year),
            result,
            event: Some(Event {
                id: body.len() as u64,
                slug: body.into(),
                logo: None,
                cover: None,
            }),
        };
        let awards = [
            award("Cannes", 2022, AwardResult::Selected),
            award("BAFTA", 2023, AwardResult::Nominated),
            award("Spirit", 2022, AwardResult::Won),
            award("BAFTA", 2023, AwardResult::Won),
            award("Gotham", 2021, AwardResult::Won),
        ];
        let groups = ceremonies(&awards);
        let order: Vec<_> = groups.iter().map(|group| group.body).collect();
        // Tiles with a win first, the lower event id (here the shorter name) first among
        // equals; screenings last.
        assert_eq!(order, ["BAFTA", "Spirit", "Gotham", "Cannes"]);
        assert_eq!(groups[0].awards[0].result, AwardResult::Won);
        assert_eq!(groups[0].awards.len(), 2);
        let mut classics = award("Cannes", 2017, AwardResult::Selected);
        classics.category = Some("Cannes Classics".into());
        let twice = [classics, award("Cannes", 2017, AwardResult::Selected)];
        assert_eq!(ceremonies(&twice)[0].awards.len(), 1);
        // A festival the film returned to is one tile, its years newest first.
        let returned = [
            award("Berlin", 1990, AwardResult::Selected),
            award("Berlin", 2010, AwardResult::Selected),
        ];
        let berlin = ceremonies(&returned);
        assert_eq!(berlin.len(), 1);
        assert_eq!(berlin[0].years, [2010, 1990]);
        assert_eq!(berlin[0].year, Some(2010));
    }
}
