//! Atomic type styles.
use crate::assets::Weighted;
use gpui::{FontWeight, SharedString, Styled, px, relative};

/// Font size, weight and line height applied as one style.
#[derive(Clone, Copy, Debug)]
pub struct TypeStyle {
    /// Logical pixel size.
    pub size: f32,
    /// Numeric CSS weight.
    pub weight: f32,
    /// Unitless line height.
    pub line_height: f32,
}

/// CSS caption.
pub const CAPTION: TypeStyle = TypeStyle {
    size: 11.0,
    weight: 400.0,
    line_height: 1.4,
};
/// CSS label.
pub const LABEL: TypeStyle = TypeStyle {
    size: 13.0,
    weight: 500.0,
    line_height: 1.4,
};
/// CSS body.
pub const BODY: TypeStyle = TypeStyle {
    size: 16.0,
    weight: 400.0,
    line_height: 1.55,
};
/// CSS heading.
pub const HEADING: TypeStyle = TypeStyle {
    size: 18.0,
    weight: 500.0,
    line_height: 1.3,
};
/// CSS title.
pub const TITLE: TypeStyle = TypeStyle {
    size: 24.0,
    weight: 500.0,
    line_height: 1.15,
};
/// The CSS text-2xl token; display overrides its line height.
pub const TEXT_2XL: TypeStyle = TypeStyle {
    size: 36.0,
    weight: 500.0,
    line_height: 1.1,
};
/// The CSS text-3xl token.
pub const TEXT_3XL: TypeStyle = TypeStyle {
    size: 52.0,
    weight: 500.0,
    line_height: 1.05,
};
/// Film titles over artwork. The interface never goes above medium (500); the 700 face is
/// bundled only for subtitles.
pub const DISPLAY: TypeStyle = TypeStyle {
    size: 52.0,
    weight: 500.0,
    line_height: 1.0,
};
/// Row and section headings.
pub const SECTION: TypeStyle = TypeStyle {
    size: 16.0,
    weight: 500.0,
    line_height: 1.2,
};
/// Director, origin and year lines.
pub const CREDIT: TypeStyle = TypeStyle {
    size: 13.0,
    weight: 500.0,
    line_height: 1.3,
};
/// Lead-in before a credited name: "Directed by".
pub const CREDIT_LEAD: TypeStyle = TypeStyle {
    size: 14.0,
    weight: 400.0,
    line_height: 1.3,
};
/// A credited name, and the original-language title under a display title.
pub const CREDIT_NAME: TypeStyle = TypeStyle {
    size: 16.0,
    weight: 500.0,
    line_height: 1.3,
};
/// Origin and year under a credited name: "Japan, 2023". Regular, so the name leads.
pub const CREDIT_PLACE: TypeStyle = TypeStyle {
    size: 14.0,
    weight: 400.0,
    line_height: 1.3,
};
/// Pill button labels.
pub const BUTTON: TypeStyle = TypeStyle {
    size: 14.0,
    weight: 500.0,
    line_height: 1.0,
};

/// GPUI has no text-transform, so uppercase is applied to the string.
pub fn caps(text: &str) -> SharedString {
    text.to_uppercase().into()
}

impl TypeStyle {
    /// Apply all type properties together.
    pub fn apply<T: Styled>(self, element: T) -> T {
        element
            .text_size(px(self.size))
            .weight(FontWeight(self.weight))
            .line_height(relative(self.line_height))
    }
}

/// Atomic typography helpers shared by route views.
pub trait Typography: Styled + Sized {
    /// Apply caption typography.
    fn caption(self) -> Self {
        CAPTION.apply(self)
    }
    /// Apply label typography.
    fn label(self) -> Self {
        LABEL.apply(self)
    }
    /// Apply body typography.
    fn body(self) -> Self {
        BODY.apply(self)
    }
    /// Apply heading typography.
    fn heading(self) -> Self {
        HEADING.apply(self)
    }
    /// Apply title typography.
    fn title(self) -> Self {
        TITLE.apply(self)
    }
    /// Apply responsive display typography.
    fn display(self, width: f32) -> Self {
        TypeStyle {
            size: (width * 0.05).clamp(36.0, DISPLAY.size),
            ..DISPLAY
        }
        .apply(self)
    }
}
impl<T: Styled> Typography for T {}

#[cfg(test)]
mod tests {
    /// The interface stops at medium (500). Subtitles take their weight from a user preference
    /// at runtime, so they never name a heavy weight in source.
    #[test]
    fn interface_never_uses_bold() {
        let src = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src"));
        let found = heavy_weights(src);
        assert!(found.is_empty(), "weights above medium: {found:?}");
    }

    fn heavy_weights(dir: &std::path::Path) -> Vec<String> {
        // Assembled at runtime so this file does not match itself.
        let banned: Vec<String> = ["SEMIBOLD", "BOLD", "EXTRA_BOLD", "BLACK"]
            .iter()
            .map(|name| format!("FontWeight::{name}"))
            .chain(
                ["600", "700", "800", "900"]
                    .iter()
                    .map(|w| format!("weight: {w}.0")),
            )
            .collect();
        let mut found = vec![];
        for path in std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()) {
            if path.is_dir() {
                found.extend(heavy_weights(&path));
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap_or_default();
            for (number, line) in source.lines().enumerate() {
                if banned.iter().any(|b| line.contains(b.as_str())) {
                    found.push(format!("{}:{}", path.display(), number + 1));
                }
            }
        }
        found
    }
}
