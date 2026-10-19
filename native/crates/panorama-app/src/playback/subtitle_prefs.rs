//! Subtitle appearance preferences: defaults, limits and normalisation of
//! `runtime/subtitle-preferences.ts` and `runtime/subtitle-appearance.ts`.
use serde_json::{Value, json};

/// Store key (under `pref:`) of the persisted preferences.
pub const KEY: &str = "subtitlePreferences";

/// Font weight choices of the Appearance column.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FontWeight {
    /// 400.
    Regular,
    /// 500; the TSX also maps the legacy `semibold` here.
    Medium,
    /// 700.
    Bold,
}

impl FontWeight {
    /// Every choice in display order.
    pub const ALL: [Self; 3] = [Self::Regular, Self::Medium, Self::Bold];

    /// The stored and displayed name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Regular => "regular",
            Self::Medium => "medium",
            Self::Bold => "bold",
        }
    }

    /// Numeric CSS weight.
    pub fn css(self) -> f32 {
        match self {
            Self::Regular => 400.0,
            Self::Medium => 500.0,
            Self::Bold => 700.0,
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "regular" => Some(Self::Regular),
            "medium" | "semibold" => Some(Self::Medium),
            "bold" => Some(Self::Bold),
            _ => None,
        }
    }
}

/// An sRGB colour; subtitle text colours are opaque, opacity is a separate field.
pub type Rgb = [u8; 3];

/// `#rrggbb` in lower case.
pub fn hex(color: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2])
}

/// Parse `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`; any alpha is dropped.
pub fn parse_hex(value: &str) -> Option<Rgb> {
    let digits = value.trim().strip_prefix('#')?;
    if !digits.is_ascii() || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let pair = |range: std::ops::Range<usize>| u8::from_str_radix(&digits[range], 16).ok();
    let short = |index: usize| {
        u8::from_str_radix(&digits[index..=index], 16)
            .ok()
            .map(|v| v * 17)
    };
    match digits.len() {
        3 | 4 => Some([short(0)?, short(1)?, short(2)?]),
        6 | 8 => Some([pair(0..2)?, pair(2..4)?, pair(4..6)?]),
        _ => None,
    }
}

/// Strict `#rrggbb`, the custom colour input's validation.
pub fn parse_strict_hex(value: &str) -> Option<Rgb> {
    let value = value.trim();
    (value.len() == 7).then(|| parse_hex(value)).flatten()
}

/// JavaScript `Math.round`: halves go toward positive infinity.
fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

/// `finite(value, fallback, min, max, step)` of `subtitle-appearance.ts`.
fn stepped(value: f64, min: f64, max: f64, step: f64) -> f64 {
    let value = (js_round(value / step) * step).clamp(min, max);
    // Remove binary noise such as 1.2400000000000002.
    (value * 1e6).round() / 1e6
}

/// One stepper of the Appearance column, in the TSX order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Field {
    /// Text opacity, percent.
    TextOpacity,
    /// Subtitle delay in seconds; stepped only.
    Delay,
    /// Font size in pixels.
    FontSize,
    /// Vertical position, percent of the video height from the bottom.
    VerticalPosition,
    /// Horizontal padding in pixels.
    PaddingX,
    /// Vertical padding in pixels.
    PaddingY,
    /// Unitless line height.
    LineHeight,
    /// Box opacity, percent.
    BoxOpacity,
    /// Box rounding in pixels.
    BoxRounding,
}

impl Field {
    /// Every stepper in display order.
    pub const ALL: [Self; 9] = [
        Self::TextOpacity,
        Self::Delay,
        Self::FontSize,
        Self::VerticalPosition,
        Self::PaddingX,
        Self::PaddingY,
        Self::LineHeight,
        Self::BoxOpacity,
        Self::BoxRounding,
    ];

    /// The stepper label.
    pub fn label(self) -> &'static str {
        match self {
            Self::TextOpacity => "Text opacity",
            Self::Delay => "Delay",
            Self::FontSize => "Font size",
            Self::VerticalPosition => "Vertical Position",
            Self::PaddingX => "Horizontal Padding",
            Self::PaddingY => "Vertical Padding",
            Self::LineHeight => "Line height",
            Self::BoxOpacity => "Box opacity",
            Self::BoxRounding => "Box rounding",
        }
    }

    /// The accessible name of the minus and plus buttons.
    pub fn action_label(self, increase: bool) -> String {
        let verb = if increase { "Increase" } else { "Decrease" };
        format!("{verb} {}", self.label().to_lowercase())
    }

    /// Inclusive limits.
    pub fn range(self) -> (f64, f64) {
        match self {
            Self::TextOpacity | Self::BoxOpacity | Self::VerticalPosition => (0.0, 100.0),
            Self::Delay => (-10.0, 10.0),
            Self::FontSize => (12.0, 96.0),
            Self::PaddingX | Self::PaddingY => (0.0, 64.0),
            Self::LineHeight => (1.0, 2.0),
            Self::BoxRounding => (0.0, 32.0),
        }
    }

    /// The minus and plus button step.
    pub fn step(self) -> f64 {
        match self {
            Self::Delay => 0.5,
            Self::VerticalPosition => 5.0,
            Self::LineHeight => 0.05,
            _ => 1.0,
        }
    }

    /// Whether the value can be typed (Delay is stepped only).
    pub fn editable(self) -> bool {
        self != Self::Delay
    }

    /// Suffix beside the value and the edit field.
    pub fn suffix(self) -> &'static str {
        match self {
            Self::TextOpacity | Self::BoxOpacity | Self::VerticalPosition => "%",
            Self::FontSize | Self::PaddingX | Self::PaddingY | Self::BoxRounding => "px",
            Self::Delay | Self::LineHeight => "",
        }
    }

    /// The value as the stepper shows it (`valueText`).
    pub fn text(self, value: f64) -> String {
        match self {
            Self::Delay => format!("{value:.1}s"),
            Self::LineHeight => format!("{value:.2}"),
            _ => format!("{}{}", value, self.suffix()),
        }
    }

    /// Delay and Vertical Position stay editable under an authored or bitmap track.
    pub fn works_when_limited(self) -> bool {
        matches!(self, Self::Delay | Self::VerticalPosition)
    }

    /// Clamp, round and snap a value to this field's grid.
    pub fn normalise(self, value: f64) -> f64 {
        let (min, max) = self.range();
        let step = match self {
            Self::Delay => 0.5,
            Self::LineHeight => 0.01,
            _ => 1.0,
        };
        stepped(value, min, max, step)
    }
}

/// Everything the Appearance column edits, plus the delay.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SubtitlePreferences {
    /// Text colour.
    pub text_color: Rgb,
    /// Text opacity, percent.
    pub text_opacity: f64,
    /// Font size in pixels.
    pub font_size: f64,
    /// Font weight.
    pub font_weight: FontWeight,
    /// Horizontal padding in pixels.
    pub padding_x: f64,
    /// Vertical padding in pixels.
    pub padding_y: f64,
    /// Unitless line height.
    pub line_height: f64,
    /// Box (background) opacity, percent.
    pub box_opacity: f64,
    /// Box corner radius in pixels.
    pub box_rounding: f64,
    /// Delay in seconds. Session only, like the Electron runtime: it is not persisted.
    pub delay: f64,
    /// Percent of the video height between the bottom edge and the cue.
    pub vertical_position: f64,
}

impl Default for SubtitlePreferences {
    fn default() -> Self {
        Self {
            text_color: [0xff, 0xff, 0xff],
            text_opacity: 100.0,
            font_size: 38.0,
            font_weight: FontWeight::Regular,
            padding_x: 20.0,
            padding_y: 0.0,
            line_height: 1.24,
            box_opacity: 68.0,
            box_rounding: 0.0,
            delay: 0.0,
            vertical_position: 15.0,
        }
    }
}

impl SubtitlePreferences {
    /// The value of a stepper field.
    pub fn get(&self, field: Field) -> f64 {
        match field {
            Field::TextOpacity => self.text_opacity,
            Field::Delay => self.delay,
            Field::FontSize => self.font_size,
            Field::VerticalPosition => self.vertical_position,
            Field::PaddingX => self.padding_x,
            Field::PaddingY => self.padding_y,
            Field::LineHeight => self.line_height,
            Field::BoxOpacity => self.box_opacity,
            Field::BoxRounding => self.box_rounding,
        }
    }

    /// Set a stepper field, clamped and rounded as the runtime does. Non-finite input is ignored.
    pub fn set(&mut self, field: Field, value: f64) {
        if !value.is_finite() {
            return;
        }
        let value = field.normalise(value);
        match field {
            Field::TextOpacity => self.text_opacity = value,
            Field::Delay => self.delay = value,
            Field::FontSize => self.font_size = value,
            Field::VerticalPosition => self.vertical_position = value,
            Field::PaddingX => self.padding_x = value,
            Field::PaddingY => self.padding_y = value,
            Field::LineHeight => self.line_height = value,
            Field::BoxOpacity => self.box_opacity = value,
            Field::BoxRounding => self.box_rounding = value,
        }
    }

    /// One minus or plus press.
    pub fn step(&mut self, field: Field, direction: i32) {
        self.set(field, self.get(field) + field.step() * f64::from(direction));
    }

    /// `normalizeSubtitlePreferences`: any invalid field falls back to its default on its own.
    pub fn normalise(value: &Value) -> Self {
        let defaults = Self::default();
        let Some(record) = value.as_object() else {
            return defaults;
        };
        let number = |name: &str, field: Field| {
            record
                .get(name)
                .and_then(Value::as_f64)
                .filter(|value| value.is_finite())
                .map_or_else(|| defaults.get(field), |value| field.normalise(value))
        };
        Self {
            text_color: record
                .get("textColor")
                .and_then(Value::as_str)
                .and_then(parse_hex)
                .unwrap_or(defaults.text_color),
            text_opacity: number("textOpacity", Field::TextOpacity),
            font_size: number("fontSizePx", Field::FontSize),
            font_weight: record
                .get("fontWeight")
                .and_then(Value::as_str)
                .and_then(FontWeight::parse)
                .unwrap_or(defaults.font_weight),
            padding_x: number("paddingX", Field::PaddingX),
            padding_y: number("paddingY", Field::PaddingY),
            line_height: number("lineHeight", Field::LineHeight),
            box_opacity: number("backgroundOpacity", Field::BoxOpacity),
            box_rounding: number("borderRadius", Field::BoxRounding),
            delay: 0.0,
            vertical_position: number("verticalPosition", Field::VerticalPosition),
        }
    }

    /// Parse stored bytes; corrupt input is the defaults.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        serde_json::from_slice::<Value>(bytes)
            .map_or_else(|_| Self::default(), |value| Self::normalise(&value))
    }

    /// The persisted JSON: the Electron field names, without the session-only delay.
    pub fn to_bytes(&self) -> Vec<u8> {
        let value = json!({
            "textColor": hex(self.text_color),
            "textOpacity": self.text_opacity,
            "fontSizePx": self.font_size,
            "fontWeight": self.font_weight.name(),
            "paddingX": self.padding_x,
            "paddingY": self.padding_y,
            "lineHeight": self.line_height,
            "backgroundOpacity": self.box_opacity,
            "borderRadius": self.box_rounding,
            "verticalPosition": self.vertical_position,
        });
        serde_json::to_vec(&value).unwrap_or_default()
    }
}

#[cfg(test)]
#[path = "subtitle_prefs_tests.rs"]
mod tests;
