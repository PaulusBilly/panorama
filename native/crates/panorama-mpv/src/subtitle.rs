use crate::{MpvError, MpvValue, PropertyValue};
use std::path::PathBuf;

/// mpv fallback subtitle preferences from `desktop/main/main.ts` and the shared
/// writable-property contract. Renderer-only CSS geometry belongs to PR 3.3.
#[derive(Clone, Debug)]
pub struct SubtitleStyle {
    /// Font family; Electron uses DM Sans.
    pub font: String,
    /// Packaged font directory, if supplied by the app.
    pub fonts_dir: Option<PathBuf>,
    /// Positive font scale multiplier.
    pub scale: f64,
    /// Vertical mpv position, from 0 to 100.
    pub position: f64,
    /// Signed subtitle delay in seconds.
    pub delay: f64,
    /// mpv color syntax for text (`#AARRGGBB` when including alpha).
    pub color: String,
    /// mpv color syntax for the background box.
    pub background_color: String,
    /// mpv color syntax for outline.
    pub border_color: String,
    /// Bold fallback text.
    pub bold: bool,
    /// Non-negative shadow offset; Electron uses 10.
    pub shadow_offset: f64,
}

impl Default for SubtitleStyle {
    fn default() -> Self {
        Self {
            font: "DM Sans".into(),
            fonts_dir: None,
            scale: 1.0,
            position: 100.0,
            delay: 0.0,
            color: "#FFFFFFFF".into(),
            background_color: "#AD000000".into(),
            border_color: "#00000000".into(),
            bold: false,
            shadow_offset: 10.0,
        }
    }
}

impl SubtitleStyle {
    pub(crate) fn properties(self) -> Result<Vec<(String, PropertyValue)>, MpvError> {
        if !self.scale.is_finite()
            || self.scale <= 0.0
            || !(0.0..=100.0).contains(&self.position)
            || !self.shadow_offset.is_finite()
            || self.shadow_offset < 0.0
        {
            return Err(MpvError::InvalidArgument);
        }
        let mut values = Vec::new();
        if let Some(path) = self.fonts_dir {
            let path = path.to_str().ok_or(MpvError::InvalidArgument)?;
            values.push(("sub-fonts-dir".into(), path.into_mpv_value()?));
        }
        for (name, value) in [
            ("sub-font", self.font.into_mpv_value()?),
            ("sub-border-style", "background-box".into_mpv_value()?),
            ("sub-border-size", 0.0.into_mpv_value()?),
            ("sub-shadow-offset", self.shadow_offset.into_mpv_value()?),
            ("sub-scale", self.scale.into_mpv_value()?),
            ("sub-pos", self.position.into_mpv_value()?),
            ("sub-delay", self.delay.into_mpv_value()?),
            ("sub-color", self.color.into_mpv_value()?),
            ("sub-back-color", self.background_color.into_mpv_value()?),
            ("sub-border-color", self.border_color.into_mpv_value()?),
            ("sub-bold", self.bold.into_mpv_value()?),
        ] {
            values.push((name.into(), value));
        }
        Ok(values)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fallback_matches_electron_and_rejects_nonfinite_values() {
        let values = SubtitleStyle::default().properties().unwrap();
        assert!(values.iter().any(
            |(n, v)| n == "sub-font" && matches!(v, PropertyValue::String(s) if s == "DM Sans")
        ));
        assert!(
            values
                .iter()
                .any(|(n, v)| n == "sub-shadow-offset" && matches!(v, PropertyValue::Double(10.0)))
        );
        assert!(
            SubtitleStyle {
                delay: f64::NAN,
                ..Default::default()
            }
            .properties()
            .is_err()
        );
    }
}
