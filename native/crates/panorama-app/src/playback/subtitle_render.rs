//! Who draws a subtitle track, how cue text is sanitised, and how preferences map to either renderer.
use super::subtitle_prefs::{FontWeight, SubtitlePreferences};
use panorama_mpv::SubtitleStyle;

/// Longest cue text drawn; mpv hands over untrusted text.
pub const MAX_CUE_CHARS: usize = 500;
/// The size the preferences' 38 px default scales mpv's font from.
const REFERENCE_FONT_PX: f64 = 38.0;

/// Why mpv keeps drawing a track.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Limitation {
    /// Image subtitles (PGS, VobSub, DVB).
    Bitmap,
    /// ASS/SSA or another format with its own styling.
    Authored,
}

impl Limitation {
    /// The note the Appearance column shows (`video-engine.ts`).
    pub fn message(self) -> &'static str {
        match self {
            Self::Bitmap => "Image subtitles keep their original appearance.",
            Self::Authored => "This track keeps its original appearance.",
        }
    }
}

/// The renderer of the selected track.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Renderer {
    /// GPUI draws the cue text; mpv's `sub-visibility` is off.
    Gpui,
    /// mpv draws the track, with the preferences it supports.
    Mpv(Limitation),
}

impl Renderer {
    /// The limitation note, if mpv renders.
    pub fn limitation(self) -> Option<Limitation> {
        match self {
            Self::Gpui => None,
            Self::Mpv(limitation) => Some(limitation),
        }
    }
}

/// The codec to renderer table. No selected track counts as GPUI (nothing is drawn either way).
pub fn renderer_for(codec: Option<&str>) -> Renderer {
    let Some(codec) = codec else {
        return Renderer::Gpui;
    };
    match codec.trim().to_ascii_lowercase().as_str() {
        "subrip" | "srt" | "webvtt" | "mov_text" | "text" => Renderer::Gpui,
        "hdmv_pgs_subtitle" | "dvd_subtitle" | "dvb_subtitle" | "pgssub" | "dvdsub" => {
            Renderer::Mpv(Limitation::Bitmap)
        }
        // ass and ssa are authored; any codec the table does not know keeps mpv's own look.
        _ => Renderer::Mpv(Limitation::Authored),
    }
}

/// Whether GPUI shows a cue: only while a text subtitle track is selected. `selected` is the
/// selected track's codec, `None` when no track is selected.
pub fn draws_cue(selected: Option<Option<&str>>) -> bool {
    selected.is_some_and(|codec| codec.is_some() && renderer_for(codec) == Renderer::Gpui)
}

/// What to send mpv when the preferences or the selected track's renderer change: the new
/// state, or `None` while it equals what was last sent. A seek changes neither, so it sends
/// nothing; a new source clears the tracks, which flips a bitmap track back to GPUI.
pub fn plan(
    applied: Option<(SubtitlePreferences, Renderer)>,
    prefs: &SubtitlePreferences,
    renderer: Renderer,
) -> Option<(SubtitlePreferences, Renderer)> {
    let next = (*prefs, renderer);
    (applied != Some(next)).then_some(next)
}

/// Plain cue text: ASS override blocks removed, line breaks normalised, control characters
/// dropped, capped at [`MAX_CUE_CHARS`]. `None` when nothing visible remains.
pub fn sanitize_cue(text: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    let mut count = 0;
    while let Some(c) = chars.next() {
        if c == '{' && chars.peek() == Some(&'\\') {
            // An override block runs to the next closing brace; an unterminated one is dropped.
            for next in chars.by_ref() {
                if next == '}' {
                    break;
                }
            }
            continue;
        }
        let c = match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    continue;
                }
                '\n'
            }
            '\n' | '\t' => c,
            c if c.is_control() => continue,
            c => c,
        };
        if count == MAX_CUE_CHARS {
            break;
        }
        out.push(c);
        count += 1;
    }
    let out = out.trim_matches(|c: char| c.is_whitespace()).to_owned();
    (!out.is_empty()).then_some(out)
}

/// The preferences as the DOM renderer styles them (`subtitleAppearanceCss`), in plain numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Appearance {
    /// Font size in pixels.
    pub font_px: f32,
    /// CSS font weight.
    pub weight: f32,
    /// Unitless line height.
    pub line_height: f32,
    /// Horizontal padding in pixels.
    pub padding_x: f32,
    /// Vertical padding in pixels.
    pub padding_y: f32,
    /// Text colour and its alpha (text opacity).
    pub text: ([u8; 3], f32),
    /// Box colour (black) alpha.
    pub background_alpha: f32,
    /// Corner radius in pixels.
    pub radius: f32,
}

impl Appearance {
    /// Derive the look from the preferences.
    pub fn from(prefs: &SubtitlePreferences) -> Self {
        Self {
            font_px: prefs.font_size as f32,
            weight: prefs.font_weight.css(),
            line_height: prefs.line_height as f32,
            padding_x: prefs.padding_x as f32,
            padding_y: prefs.padding_y as f32,
            text: (prefs.text_color, (prefs.text_opacity / 100.0) as f32),
            background_alpha: (prefs.box_opacity / 100.0) as f32,
            radius: prefs.box_rounding as f32,
        }
    }
}

/// Distance of the cue box's bottom edge from the video bottom (`position()` in the renderer):
/// at least 16 px, the preferred percentage of the height, never pushing the box off the top.
pub fn bottom_inset(height: f32, box_height: f32, vertical_position: f32) -> f32 {
    let inset = (height * vertical_position / 100.0).max(16.0);
    inset.min((height - box_height - 16.0).max(16.0)).max(16.0)
}

/// What the host applies to mpv for the subtitle preferences and the selected track's renderer.
#[derive(Clone, Debug)]
pub struct SubtitleSetup {
    /// `sub-visibility`: yes only when mpv renders the track.
    pub mpv_visible: bool,
    /// Properties mpv supports: scale, position, delay, colours, weight.
    pub style: SubtitleStyle,
}

impl SubtitleSetup {
    /// Build the setup for these preferences and renderer.
    pub fn new(prefs: &SubtitlePreferences, renderer: Renderer) -> Self {
        Self {
            mpv_visible: matches!(renderer, Renderer::Mpv(_)),
            style: mpv_style(prefs),
        }
    }
}

fn argb(color: [u8; 3], alpha: f64) -> String {
    format!(
        "#{:02X}{:02X}{:02X}{:02X}",
        (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
        color[0],
        color[1],
        color[2]
    )
}

/// The mpv subset of the preferences. `sub-pos` counts from the top, so the bottom inset flips.
pub fn mpv_style(prefs: &SubtitlePreferences) -> SubtitleStyle {
    SubtitleStyle {
        font: if prefs.font_weight == FontWeight::Medium {
            "DM Sans Medium"
        } else {
            "DM Sans"
        }
        .into(),
        scale: (prefs.font_size / REFERENCE_FONT_PX).max(0.1),
        position: (100.0 - prefs.vertical_position).clamp(0.0, 100.0),
        delay: prefs.delay,
        color: argb(prefs.text_color, prefs.text_opacity / 100.0),
        background_color: argb([0, 0, 0], prefs.box_opacity / 100.0),
        bold: prefs.font_weight == FontWeight::Bold,
        ..SubtitleStyle::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_codec_table_splits_text_bitmap_and_authored() {
        for codec in ["subrip", "webvtt", "mov_text", "text", "SubRip"] {
            assert_eq!(renderer_for(Some(codec)), Renderer::Gpui, "{codec}");
        }
        for codec in ["hdmv_pgs_subtitle", "dvd_subtitle", "dvb_subtitle"] {
            assert_eq!(
                renderer_for(Some(codec)),
                Renderer::Mpv(Limitation::Bitmap),
                "{codec}"
            );
        }
        for codec in ["ass", "ssa", "microdvd", "unheard_of"] {
            assert_eq!(
                renderer_for(Some(codec)),
                Renderer::Mpv(Limitation::Authored),
                "{codec}"
            );
        }
        assert_eq!(renderer_for(None), Renderer::Gpui);
        assert_eq!(Renderer::Gpui.limitation(), None);
        assert_eq!(
            Renderer::Mpv(Limitation::Bitmap)
                .limitation()
                .map(Limitation::message),
            Some("Image subtitles keep their original appearance.")
        );
        assert_eq!(
            Limitation::Authored.message(),
            "This track keeps its original appearance."
        );
    }

    #[test]
    fn override_tags_are_stripped_and_text_stays_plain() {
        assert_eq!(
            sanitize_cue("{\\an8}Hello {\\i1}there{\\i0}").as_deref(),
            Some("Hello there")
        );
        assert_eq!(sanitize_cue("{\\pos(1,2)\\b1}").as_deref(), None);
        assert_eq!(sanitize_cue("a {note} b").as_deref(), Some("a {note} b"));
        assert_eq!(sanitize_cue("broken {\\i1 text").as_deref(), Some("broken"));
        assert_eq!(
            sanitize_cue("one\r\ntwo\rthree").as_deref(),
            Some("one\ntwo\nthree")
        );
        assert_eq!(sanitize_cue("nul\0l\u{7}ed").as_deref(), Some("nulled"));
        assert_eq!(sanitize_cue("  \n ").as_deref(), None);
        assert_eq!(sanitize_cue("<b>kept</b>").as_deref(), Some("<b>kept</b>"));
    }

    #[test]
    fn a_cue_shows_only_for_a_selected_text_track() {
        assert!(!draws_cue(None), "no track selected");
        assert!(draws_cue(Some(Some("subrip"))));
        assert!(draws_cue(Some(Some("webvtt"))));
        assert!(!draws_cue(Some(Some("hdmv_pgs_subtitle"))));
        assert!(!draws_cue(Some(Some("ass"))));
        assert!(!draws_cue(Some(None)), "an unknown codec is left to mpv");
    }

    #[test]
    fn visibility_follows_track_source_and_seek_changes() {
        let prefs = SubtitlePreferences::default();
        let visible = |state: Option<(SubtitlePreferences, Renderer)>| {
            state.map(|(prefs, renderer)| SubtitleSetup::new(&prefs, renderer).mpv_visible)
        };
        // First sync with nothing selected: GPUI draws, mpv hidden.
        let mut applied = plan(None, &prefs, renderer_for(None));
        assert_eq!(visible(applied), Some(false));
        // A text track is selected: renderer unchanged, nothing to send.
        assert_eq!(plan(applied, &prefs, renderer_for(Some("subrip"))), None);
        // Switching to a PGS track turns mpv rendering on.
        let pgs = plan(applied, &prefs, renderer_for(Some("hdmv_pgs_subtitle")));
        assert_eq!(visible(pgs), Some(true));
        applied = pgs;
        // A seek changes neither the preferences nor the track.
        assert_eq!(
            plan(applied, &prefs, renderer_for(Some("hdmv_pgs_subtitle"))),
            None
        );
        // A new source clears the tracks: back to GPUI, mpv hidden again.
        let cleared = plan(applied, &prefs, renderer_for(None));
        assert_eq!(visible(cleared), Some(false));
        applied = cleared;
        // Delay edits resend; the renderer stays.
        let delayed = SubtitlePreferences {
            delay: 1.5,
            ..prefs
        };
        assert!(plan(applied, &delayed, Renderer::Gpui).is_some());
    }

    #[test]
    fn cues_are_capped_at_500_characters() {
        let long = "é".repeat(2000);
        let cue = sanitize_cue(&long).expect("cue");
        assert_eq!(cue.chars().count(), MAX_CUE_CHARS);
        let tagged = format!("{{\\i1}}{}", "x".repeat(600));
        assert_eq!(
            sanitize_cue(&tagged).map(|cue| cue.len()),
            Some(MAX_CUE_CHARS)
        );
    }

    #[test]
    fn appearance_follows_the_preferences() {
        let prefs = SubtitlePreferences {
            text_opacity: 50.0,
            box_opacity: 0.0,
            font_weight: FontWeight::Bold,
            ..Default::default()
        };
        let look = Appearance::from(&prefs);
        assert_eq!(
            (look.font_px, look.weight, look.padding_x),
            (38.0, 700.0, 20.0)
        );
        assert_eq!((look.text.1, look.background_alpha), (0.5, 0.0));
        assert!((look.line_height - 1.24).abs() < 1e-6);
    }

    #[test]
    fn the_cue_sits_at_the_preferred_height_but_stays_inside_the_video() {
        assert_eq!(bottom_inset(800.0, 50.0, 15.0), 120.0);
        assert_eq!(bottom_inset(800.0, 50.0, 0.0), 16.0);
        assert_eq!(bottom_inset(800.0, 50.0, 100.0), 734.0);
        assert_eq!(bottom_inset(100.0, 200.0, 50.0), 16.0);
    }

    #[test]
    fn mpv_receives_the_supported_subset() {
        let mut prefs = SubtitlePreferences {
            text_color: [0xff, 0xe0, 0x66],
            font_weight: FontWeight::Bold,
            ..Default::default()
        };
        prefs.font_size = 76.0;
        prefs.box_opacity = 0.0;
        prefs.vertical_position = 20.0;
        prefs.delay = -1.5;
        prefs.text_opacity = 50.0;
        let style = mpv_style(&prefs);
        assert_eq!(style.scale, 2.0);
        assert_eq!(style.position, 80.0);
        assert_eq!(style.delay, -1.5);
        assert_eq!(style.color, "#80FFE066");
        assert_eq!(style.background_color, "#00000000");
        assert!(style.bold);
        assert_eq!(
            mpv_style(&SubtitlePreferences::default()).background_color,
            "#AD000000"
        );
        assert!(!SubtitleSetup::new(&prefs, Renderer::Gpui).mpv_visible);
        assert!(SubtitleSetup::new(&prefs, Renderer::Mpv(Limitation::Bitmap)).mpv_visible);
    }
}
