//! The hover preview's details, set like an admission ticket: labelled fields in two rows over
//! the perforation, then the stub with Watch and the synopsis as its fine print.
use crate::{
    assets::Weighted,
    film_display::Quality,
    source_lookup::SourceLookup,
    theme::{DURATION_DELIBERATE, Theme, Typography, motion::EASE_OUT},
    transition::bezier,
};
use gpui::{Animation, AnimationExt, AnyElement, Div, FontWeight, div, prelude::*, px, relative};
use panorama_core::addons::{FilmDetails, Kind};

/// Field label: small capitals over each value.
const LABEL_SIZE: f32 = 11.0;
const LABEL_LINE: f32 = 1.4;
/// Field value.
const VALUE_SIZE: f32 = 15.0;
const VALUE_LINE: f32 = 1.35;
/// Space between a label and its value.
const LABEL_GAP: f32 = 4.0;
/// Space between the two rows of fields.
const ROW_GAP: f32 = 18.0;
/// Above the first row: none, as the title's caption already pads the artwork's bottom edge.
const TOP: f32 = 0.0;
/// Below the second row, down to the perforation.
const BOTTOM: f32 = 24.0;
/// Space between columns.
const COLUMN_GAP: f32 = 40.0;
/// The narrowest the date column gets: both quality badges and the space between them.
const QUALITY_WIDTH: f32 = 96.0;

/// Height of the fields block, from the artwork's bottom edge to the perforation. Fixed, as
/// every value holds to one line, so the perforation can be drawn before text is laid out.
pub const FIELDS_HEIGHT: f32 =
    TOP + 2.0 * (LABEL_SIZE * LABEL_LINE + LABEL_GAP + VALUE_SIZE * VALUE_LINE) + ROW_GAP + BOTTOM;

/// What the source lookup has said about a film's best quality.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceQuality {
    /// No lookup for this film: a show, whose sources belong to its episodes.
    Unasked,
    Checking,
    Found(Quality),
    /// The lookup finished without a playable source, or failed.
    Missing,
}

impl SourceQuality {
    /// The best resolution any playable source offers, and surround if any offers it.
    pub fn of(lookup: Option<&SourceLookup>) -> Self {
        lookup.map_or(Self::Unasked, |lookup| {
            Self::from_groups(&lookup.groups, lookup.finished || lookup.failed)
        })
    }

    /// What `groups` say so far; `settled` once the lookup has finished or failed.
    fn from_groups(groups: &[panorama_core::addons::StreamGroup], settled: bool) -> Self {
        let quality = crate::film_display::playable(groups)
            .map(Quality::from_source)
            .fold(None, |best: Option<Quality>, quality| {
                let best = best.unwrap_or_default();
                Some(Quality {
                    video: match (best.video, quality.video) {
                        (Some("4K"), _) | (_, Some("4K")) => Some("4K"),
                        (Some(video), _) | (_, Some(video)) => Some(video),
                        _ => None,
                    },
                    surround: best.surround || quality.surround,
                })
            });
        match quality {
            Some(quality) if quality.video.is_some() || quality.surround => Self::Found(quality),
            // Nothing playable, or no source names its resolution.
            _ if settled => Self::Missing,
            _ => Self::Checking,
        }
    }
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The release date when the addon sends one, as `Nov 6, 2026`, else its year.
pub fn released(film: &FilmDetails) -> Option<String> {
    let date = film.released.as_deref().and_then(|date| {
        let mut parts = date.get(..10)?.split('-');
        let year: u32 = parts.next()?.parse().ok()?;
        let month: usize = parts.next()?.parse().ok()?;
        let day: u32 = parts.next()?.parse().ok()?;
        let month = MONTHS.get(month.checked_sub(1)?)?;
        (1..=31)
            .contains(&day)
            .then(|| format!("{month} {day}, {year}"))
    });
    date.or_else(|| film.release_info.clone())
}

/// `102 min` or `1h42min` as `1h 42m`, the way the hero reads; anything else as the addon
/// wrote it.
pub fn runtime(film: &FilmDetails) -> Option<String> {
    let text = film.runtime.as_deref()?.trim();
    let compact = text.to_lowercase().replace(' ', "");
    let (hours, rest) = compact.split_once('h').unwrap_or(("0", &compact));
    let rest = rest.trim_end_matches("in").trim_end_matches('m');
    let minutes = hours.parse::<u32>().ok().zip(if rest.is_empty() {
        Some(0)
    } else {
        rest.parse::<u32>().ok()
    });
    let minutes = minutes
        .map(|(hours, minutes)| hours * 60 + minutes)
        .filter(|&minutes| minutes > 0);
    Some(match minutes {
        Some(minutes) if minutes >= 60 && minutes % 60 == 0 => format!("{}h", minutes / 60),
        Some(minutes) if minutes >= 60 => format!("{}h {}m", minutes / 60, minutes % 60),
        Some(minutes) => format!("{minutes}m"),
        None => text.to_owned(),
    })
}

/// A field's text value; a missing one keeps its place with a dash, so the columns never shift
/// between films.
fn text(value: Option<String>, theme: Theme) -> Div {
    let (text, ink) = match value {
        Some(value) => (value, theme.accent_text),
        None => ("—".to_owned(), theme.ink_muted),
    };
    div()
        .text_size(px(VALUE_SIZE))
        .line_height(relative(VALUE_LINE))
        .text_color(ink)
        .whitespace_nowrap()
        .child(text)
}

/// One field: its label in small capitals over a single-line value.
fn field(label: &str, value: Div, theme: Theme) -> Div {
    div()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(LABEL_GAP))
        .child(
            div()
                .text_size(px(LABEL_SIZE))
                .line_height(relative(LABEL_LINE))
                .weight(FontWeight::MEDIUM)
                .text_color(theme.ink_muted)
                .whitespace_nowrap()
                .child(label.to_uppercase()),
        )
        .child(value)
}

/// The fields, in three columns of two rows.
pub fn fields(film: &FilmDetails, quality: SourceQuality, instant: bool, theme: Theme) -> Div {
    let director = (!film.director.is_empty()).then(|| film.director.join(", "));
    let genre = (!film.genres.is_empty()).then(|| {
        film.genres
            .iter()
            .take(2)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    });
    // A show's quality depends on the episode, so its column says what it is instead. A film's
    // quality stays out of sight until the lookup finds one, then fades in whole unless `instant`.
    let third: AnyElement = match (film.kind == Kind::Movie, quality) {
        (false, _) => field("Type", text(Some("Series".into()), theme), theme).into_any_element(),
        (true, SourceQuality::Found(quality)) => {
            let badges = field(
                "Quality",
                // The badges sit in the value's line box, so the row height never changes.
                div()
                    .h(px(VALUE_SIZE * VALUE_LINE))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .children(quality.badges(theme.accent_text)),
                theme,
            );
            if instant {
                badges.into_any_element()
            } else {
                badges
                    .with_animation(
                        gpui::SharedString::from(format!("ticket-quality-{}", film.id)),
                        Animation::new(DURATION_DELIBERATE)
                            .with_easing(|time| bezier(time, EASE_OUT)),
                        |field, delta| field.opacity(delta),
                    )
                    .into_any_element()
            }
        }
        (true, _) => div().into_any_element(),
    };
    // Columns run top to bottom, each as wide as its widest value, so the two rows align and the
    // gaps between columns stay even. Only the director's column gives way to a long name.
    let column = || div().flex().flex_col().gap(px(ROW_GAP));
    div()
        .h(px(FIELDS_HEIGHT))
        .pt(px(TOP))
        .flex()
        .gap(px(COLUMN_GAP))
        .child(
            column()
                .min_w_0()
                .child(field("Director", text(director, theme).truncate(), theme))
                .child(field("Genre", text(genre, theme).truncate(), theme)),
        )
        .child(
            column()
                .flex_none()
                // Room for both badges, so late quality never pushes Runtime aside.
                .min_w(px(QUALITY_WIDTH))
                .child(field("Date", text(released(film), theme), theme))
                .child(third),
        )
        .child(
            column()
                .flex_none()
                .child(field("Runtime", text(runtime(film), theme), theme)),
        )
}

/// The synopsis set as the stub's fine print, beside Watch.
pub fn fine_print(film: &FilmDetails, theme: Theme) -> Option<Div> {
    film.description.clone().map(|text| {
        div()
            .flex_1()
            .min_w_0()
            .caption()
            .text_size(px(12.0))
            .line_height(relative(1.45))
            .text_color(theme.ink_muted)
            .line_clamp(4)
            .child(text)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn film(released: Option<&str>, info: Option<&str>, runtime: Option<&str>) -> FilmDetails {
        serde_json::from_value(serde_json::json!({
            "id": "tt1", "name": "Film", "release_info": info, "released": released,
            "runtime": runtime, "genres": [], "description": null, "director": [], "cast": [],
            "poster": null, "background": null, "logo": null, "imdb_rating": null, "links": []
        }))
        .expect("film")
    }

    #[test]
    fn a_release_date_reads_in_full_and_falls_back_to_the_year() {
        assert_eq!(
            released(&film(Some("2026-11-06T00:00:00.000Z"), Some("2026"), None)).as_deref(),
            Some("Nov 6, 2026")
        );
        assert_eq!(
            released(&film(None, Some("2024–"), None)).as_deref(),
            Some("2024–")
        );
        assert_eq!(
            released(&film(Some("2026-13-01"), Some("2026"), None)).as_deref(),
            Some("2026")
        );
    }

    #[test]
    fn minutes_read_as_hours_and_minutes() {
        let runtime_of = |text| runtime(&film(None, None, Some(text)));
        assert_eq!(runtime_of("102 min").as_deref(), Some("1h 42m"));
        assert_eq!(runtime_of("120 min").as_deref(), Some("2h"));
        assert_eq!(runtime_of("45min").as_deref(), Some("45m"));
        assert_eq!(runtime_of("2h 10m").as_deref(), Some("2h 10m"));
        assert_eq!(runtime_of("2h17min").as_deref(), Some("2h 17m"));
        assert_eq!(runtime_of("2h").as_deref(), Some("2h"));
        assert_eq!(runtime_of("N/A").as_deref(), Some("N/A"));
    }

    #[test]
    fn the_fields_block_fits_its_rows() {
        assert!((FIELDS_HEIGHT - 121.3).abs() < 0.01, "{FIELDS_HEIGHT}");
    }

    #[test]
    fn quality_is_the_best_any_playable_source_offers() {
        use panorama_core::addons::{StreamGroup, StreamSource, StreamState};
        let source = |url: &str, name: &str| StreamSource {
            stream: serde_json::from_value(serde_json::json!({ "url": url })).expect("stream"),
            name: Some(name.into()),
            title: None,
            description: None,
        };
        let group = |sources| StreamGroup {
            addon: serde_json::from_value("a#0".into()).expect("key"),
            name: "A".into(),
            state: StreamState::Ready(sources),
        };
        let groups = [group(vec![
            source("https://a.test/1", "1080p DD5.1"),
            source("https://a.test/2", "2160p"),
        ])];
        assert_eq!(
            SourceQuality::from_groups(&groups, false),
            SourceQuality::Found(Quality {
                video: Some("4K"),
                surround: true
            })
        );
        assert_eq!(
            SourceQuality::from_groups(&[], false),
            SourceQuality::Checking
        );
        assert_eq!(
            SourceQuality::from_groups(&[], true),
            SourceQuality::Missing
        );
        assert_eq!(SourceQuality::of(None), SourceQuality::Unasked);
    }
}
