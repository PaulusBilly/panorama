//! Debug capture and post-window diagnostics.
// Desktop capture needs HWND and GDI calls; unsafe is confined to this boundary.
#[allow(unsafe_code)]
pub mod screenshot;

/// Print diagnostics only after the UI loop has closed.
pub fn report(error: &str) {
    eprintln!("{error}");
}

pub(crate) fn check_default_font(window: &gpui::Window) -> Result<(), String> {
    let text_system = window.text_system();
    let mut requested = window.text_style().font();
    for (weight, advance) in [(400.0, 44.512), (500.0, 45.344), (700.0, 46.956)] {
        crate::assets::weighted(&mut requested, gpui::FontWeight(weight));
        let id = text_system.resolve_font(&requested);
        let resolved = text_system
            .get_font_for_id(id)
            .ok_or("Resolved default font has no family mapping")?;
        let actual = f32::from(
            text_system
                .advance(id, gpui::px(52.0), 'M')
                .map_err(|error| error.to_string())?
                .width,
        );
        let family = crate::assets::family(requested.weight);
        if resolved.family != family
            || resolved.weight != requested.weight
            || (actual - advance).abs() > 0.001
        {
            return Err(format!(
                "Default font check failed: requested {} {weight}, resolved {} {}, M advance {actual:.3}; expected bundled {family} {weight}, advance {advance:.3}",
                requested.family, resolved.family, resolved.weight.0,
            ));
        }
    }
    Ok(())
}

mod logger;
pub mod performance;
pub use logger::{Logger, log, logger};
