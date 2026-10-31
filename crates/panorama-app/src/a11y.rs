use gpui::{Div, Stateful, div, prelude::*, px};

/// Expose polite status copy without adding visible layout space.
pub fn status(id: &'static str, copy: String) -> Stateful<Div> {
    div()
        .id(id)
        .role(gpui::Role::Status)
        .aria_label(copy)
        .absolute()
        .size(px(0.0))
        .a11y_synthetic_children(|tree| tree.parent_node().set_live(gpui::accesskit::Live::Polite))
}
