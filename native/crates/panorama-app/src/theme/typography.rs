//! Atomic type styles matching Electron's utilities.
use gpui::{FontWeight, Styled, px, relative};

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

impl TypeStyle {
    /// Apply all type properties together.
    pub fn apply<T: Styled>(self, element: T) -> T {
        element
            .text_size(px(self.size))
            .font_weight(FontWeight(self.weight))
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
            size: (width * 0.05).clamp(36.0, 52.0),
            weight: 500.0,
            line_height: 1.05,
        }
        .apply(self)
    }
}
impl<T: Styled> Typography for T {}
