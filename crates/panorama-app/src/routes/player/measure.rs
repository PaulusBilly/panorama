//! Layout the player reads back from the previous frame (panel anchors, range hit areas).
use gpui::{Bounds, Canvas, Pixels, Size, Styled, canvas};
use std::{cell::Cell, rc::Rc};

/// Bounds painted last frame; empty cells mean the element was not rendered.
#[derive(Default)]
pub struct Measures {
    /// The player area under the caption.
    pub root: Cell<Bounds<Pixels>>,
    /// The icon pill the panels anchor above.
    pub pill: Cell<Option<Bounds<Pixels>>>,
    /// The seek pill, the fallback anchor.
    pub seek_pill: Cell<Option<Bounds<Pixels>>>,
    /// The open panel surface.
    pub surface: Cell<Option<Bounds<Pixels>>>,
    /// The volume and seek hit areas.
    pub ranges: [Cell<Bounds<Pixels>>; 2],
    /// The natural size of each panel page, by panel order.
    pub pages: [Cell<Option<Size<Pixels>>>; 4],
    /// The three select triggers of the settings panel, by `SelectId`.
    pub selects: [Cell<Option<Bounds<Pixels>>>; 3],
    /// The open select menu or colour popover.
    pub popup: Cell<Option<Bounds<Pixels>>>,
    /// The picker's saturation square and hue strip, by `picker::Part`.
    pub picker: [Cell<Option<Bounds<Pixels>>>; 2],
    /// The colour wheel swatch the picker opens from.
    pub color_row: Cell<Option<Bounds<Pixels>>>,
    /// The value field being edited.
    pub edit: Cell<Option<Bounds<Pixels>>>,
    /// The subtitle cue box.
    pub cue: Cell<Option<Size<Pixels>>>,
}

/// An invisible element that records its parent's bounds each frame.
pub fn probe(
    measures: &Rc<Measures>,
    record: impl Fn(&Measures, Bounds<Pixels>) + 'static,
) -> Canvas<()> {
    let measures = measures.clone();
    canvas(
        move |bounds, _, _| record(&measures, bounds),
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
}
