//! A sideways row that comes to rest on an item, in the manner of Apple TV: scroll freely, and
//! once the gesture stops the row glides so an item sits exactly on the column's left edge.
//! The pointer can also drag the row, and edge arrows page it while there is more to see.
//! Shared by the home Continue row and the film page's Cast & Crew, Awards & Festivals and
//! Episodes.
use crate::{
    home_layout::ViewSettings,
    theme::{DURATION_DELIBERATE, DURATION_FAST, EASE_EDITORIAL, motion::EASE_DEFAULT},
    transition::Tween,
};
use gpui::{
    Context, DispatchPhase, Div, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ScrollHandle, Stateful, canvas, div, point, prelude::*, px, svg,
};
use std::time::{Duration, Instant};

/// Quiet time after the last wheel event, momentum included, before the row settles.
const SETTLE_AFTER: Duration = Duration::from_millis(140);

#[derive(Default)]
pub struct Carousel {
    pub scroll: ScrollHandle,
    /// When the pointer last scrolled the row; the row settles once it has been quiet.
    wheeled: Option<Instant>,
    /// The glide onto an item, in scroll pixels from the start of the row.
    glide: Option<Tween>,
    /// The item keyboard focus last brought onto the column.
    focused: Option<usize>,
    /// A press on the row that may become a drag.
    drag: Option<Drag>,
    /// Distance between items, from the last [`Carousel::settle`].
    pitch: f32,
    /// Opacity of the back and forward arrows, fading as the row gains or runs out of room.
    arrows: [Option<Tween>; 2],
    /// The pointer is over the row; the arrows show only then.
    hovered: bool,
}

/// Pointer travel before a press on the row counts as a drag rather than a click.
const DRAG_SLOP: f32 = 6.0;

#[derive(Clone, Copy)]
struct Drag {
    /// Pointer x when the press began.
    pointer: f32,
    /// Row position when the press began.
    from: f32,
    /// Past the slop, so releasing must not click the card underneath.
    moved: bool,
}

/// A scroll offset as a position inside `0..=max`.
fn clamped(offset: gpui::Pixels, max: f32) -> f32 {
    (-f32::from(offset)).clamp(0.0, max)
}

/// Where a row left at `position` comes to rest: the nearest item edge, `pitch` apart.
fn rest(position: f32, pitch: f32) -> f32 {
    (position / pitch).round() * pitch
}

/// Where a glide toward `target` ends: inside the row, and on its very end when that is within
/// half an item, since item widths are fractional and the end is rarely a whole step away.
fn landing(target: f32, max: f32, pitch: f32) -> f32 {
    if max - target < pitch * 0.5 {
        max
    } else {
        target.clamp(0.0, max)
    }
}

impl Carousel {
    /// The pointer scrolled the row: let it move freely and settle once it stops.
    pub fn wheel(&mut self) {
        self.wheeled = Some(Instant::now());
        self.glide = None;
    }

    /// Glide so item `index` sits on the column's left edge, or as near as the row allows.
    pub fn show(&mut self, index: usize, pitch: f32) {
        self.glide_to(index as f32 * pitch, Instant::now());
        self.wheeled = None;
    }

    /// Keyboard focus is on item `index`: bring it onto the column once, when it arrives.
    pub fn follow(&mut self, index: usize, pitch: f32) {
        if self.focused != Some(index) {
            self.focused = Some(index);
            self.show(index, pitch);
        }
    }

    /// Frames are needed while the row waits to settle, glides, or fades an arrow.
    pub fn moving(&self, reduced: bool) -> bool {
        let now = Instant::now();
        self.wheeled.is_some()
            || self
                .glide
                .iter()
                .chain(self.arrows.iter().flatten())
                .any(|tween| tween.moving(now, reduced))
    }

    /// The back (`false`) or forward (`true`) arrow's opacity and whether it is showing,
    /// rather than fading out.
    fn arrow(&self, forward: bool, reduced: bool) -> (f32, bool) {
        self.arrows[usize::from(forward)].map_or((0.0, false), |fade| {
            (fade.value(Instant::now(), reduced), fade.target > 0.0)
        })
    }

    /// How far the row has scrolled, within its range. GPUI adds a wheel delta to the offset at
    /// once and clamps it only at the next layout, so between the two a swipe on a row that
    /// cannot move reads as room on one side and would flash an arrow.
    pub fn position(&self) -> f32 {
        clamped(self.scroll.offset().x, self.max())
    }

    fn max(&self) -> f32 {
        f32::from(self.scroll.max_offset().x).max(0.0)
    }

    /// There is more of the row before what is shown.
    pub fn can_go_back(&self) -> bool {
        self.position() > 1.0
    }

    /// There is more of the row after what is shown.
    pub fn can_go_forward(&self) -> bool {
        self.position() < self.max() - 1.0
    }

    /// Page the row by as many whole items as fit in `span`, then rest on an item.
    pub fn page(&mut self, forward: bool, span: f32) {
        let pitch = if self.pitch > 0.0 { self.pitch } else { span };
        let step = (span / pitch).floor().max(1.0) * pitch;
        let target = self.position() + if forward { step } else { -step };
        self.glide_to(rest(target, pitch), Instant::now());
        self.wheeled = None;
    }

    /// The left button went down on the row at pointer `x`.
    pub fn press(&mut self, x: f32) {
        self.drag = Some(Drag {
            pointer: x,
            from: self.position(),
            moved: false,
        });
    }

    /// The pointer is over the row, or has left it; true when that changed and the arrows must
    /// fade.
    pub fn hover(&mut self, inside: bool) -> bool {
        let changed = self.hovered != inside;
        self.hovered = inside;
        changed
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The pointer moved to `x`; true when the view must redraw. A button no longer `held` ends
    /// the drag, as a tap that lands between frames or a release outside the window goes unseen.
    pub fn drag_to(&mut self, x: f32, held: bool) -> bool {
        if !held {
            self.release();
            return true;
        }
        let max = self.max();
        let Some(drag) = &mut self.drag else {
            return false;
        };
        let dx = x - drag.pointer;
        drag.moved |= dx.abs() > DRAG_SLOP;
        if !drag.moved {
            return false;
        }
        let position = (drag.from - dx).clamp(0.0, max);
        self.glide = None;
        self.wheeled = None;
        self.scroll.set_offset(point(px(-position), px(0.0)));
        true
    }

    /// The button came up; rests the row on an item and returns whether it was a drag.
    pub fn release(&mut self) -> bool {
        let moved = self.drag.take().is_some_and(|drag| drag.moved);
        if moved && self.pitch > 0.0 {
            self.glide_to(rest(self.position(), self.pitch), Instant::now());
        }
        moved
    }

    fn glide_to(&mut self, target: f32, now: Instant) {
        let target = landing(target, self.max(), self.pitch);
        let mut glide = Tween::fixed(self.position());
        glide.retarget(target, DURATION_DELIBERATE, Some(EASE_EDITORIAL), now);
        self.glide = Some(glide);
    }

    /// Advance the settle and the glide; call once per frame before layout.
    pub fn settle(&mut self, pitch: f32, reduced: bool) {
        self.pitch = pitch;
        let now = Instant::now();
        if self
            .wheeled
            .is_some_and(|at| now.duration_since(at) >= SETTLE_AFTER)
        {
            self.wheeled = None;
            self.glide_to(rest(self.position(), pitch), now);
        }
        if let Some(glide) = self.glide {
            self.scroll
                .set_offset(point(px(-glide.value(now, reduced)), px(0.0)));
            if !glide.moving(now, reduced) {
                self.glide = None;
            }
        }
        // A drag keeps them up even when the pointer strays off the row mid-gesture.
        let present = self.hovered || self.dragging();
        let room = [self.can_go_back(), self.can_go_forward()].map(|room| room && present);
        for (fade, shown) in self.arrows.iter_mut().zip(room) {
            fade.get_or_insert(Tween::fixed(0.0)).retarget(
                if shown { 1.0 } else { 0.0 },
                DURATION_FAST,
                Some(EASE_DEFAULT),
                now,
            );
        }
    }
}

/// Arrow pill size.
const ARROW: (f32, f32) = (30.0, 52.0);
/// Space between an arrow and the column edge. The same for every row, so arrows line up down
/// the page. It equals the tightest item gap, the cast cards': there the pill's edge lands
/// exactly on the neighbouring card's edge instead of half-covering the gap, and in the award
/// tiles' wider gap it sits wholly inside it.
const ARROW_GAP: f32 = 6.0;
/// Closest an arrow comes to the window edge on a narrow window.
const ARROW_MARGIN: f32 = 8.0;

/// Make a carousel `row` draggable and overlay edge arrows, each shown only while the row can
/// move that way. `col` is the column the row rests on; `rail` finds this row's carousel on the
/// view. `art` is the height of the items' image from the row's top: the arrows centre on it,
/// or on the whole row when the items have none.
#[allow(clippy::too_many_arguments)]
pub fn controls<V: 'static>(
    id: &'static str,
    row: Stateful<Div>,
    carousel: &Carousel,
    col: crate::theme::Column,
    view: ViewSettings,
    art: Option<f32>,
    rail: fn(&mut V) -> &mut Carousel,
    cx: &mut Context<V>,
) -> Div {
    let entity = cx.entity().downgrade();
    let dragging = carousel.dragging();
    let arrows = [false, true].map(|forward| carousel.arrow(forward, view.reduced));
    let (back, forward) = (arrows[0].1, arrows[1].1);
    let scroll = carousel.scroll.clone();
    let row = row
        .capture_any_mouse_down(cx.listener(move |view, event: &MouseDownEvent, _, cx| {
            if event.button == MouseButton::Left {
                rail(view).press(f32::from(event.position.x));
                cx.notify();
            }
        }))
        .when(dragging, |row| row.cursor_grabbing())
        // Last, so cards see the release first and a drag can then keep it from clicking them.
        .child(
            canvas(
                |_, _, _| {},
                move |_, _, window, cx| {
                    // The arrows follow the laid-out row, which can change without a render.
                    let max = f32::from(scroll.max_offset().x).max(0.0);
                    let position = clamped(scroll.offset().x, max);
                    if (position > 1.0, position < max - 1.0) != (back, forward) {
                        let entity = entity.clone();
                        cx.defer(move |cx| {
                            entity.update(cx, |_, cx| cx.notify()).ok();
                        });
                    }
                    if !dragging {
                        return;
                    }
                    let moving = entity.clone();
                    window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                        if phase == DispatchPhase::Capture {
                            moving
                                .update(cx, |view, cx| {
                                    let x = f32::from(event.position.x);
                                    if rail(view).drag_to(x, event.dragging()) {
                                        cx.notify();
                                    }
                                })
                                .ok();
                        }
                    });
                    let releasing = entity.clone();
                    window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                        if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                            let moved = releasing
                                .update(cx, |view, cx| {
                                    cx.notify();
                                    rail(view).release()
                                })
                                .unwrap_or(false);
                            if moved {
                                cx.stop_propagation();
                            }
                        }
                    });
                },
            )
            .absolute()
            .size_0(),
        );
    let span = col.width;
    let theme = view.theme;
    let arrow = |forward: bool, x: f32, cx: &mut Context<V>| {
        let (opacity, shown) = arrows[usize::from(forward)];
        let x = x.clamp(
            ARROW_MARGIN,
            (view.width - ARROW.0 - ARROW_MARGIN).max(ARROW_MARGIN),
        );
        div()
            .absolute()
            .map(|band| match art {
                Some(height) => band.top_0().h(px(height)),
                None => band.top_0().bottom_0(),
            })
            .left(px(x))
            .flex()
            .items_center()
            .opacity(opacity)
            .child(
                div()
                    .id((id, usize::from(forward)))
                    .w(px(ARROW.0))
                    .h(px(ARROW.1))
                    .rounded_full()
                    // GPUI cannot blur what is behind an element, so the pill is a dark frost with
                    // a light rim: it holds against a white portrait and a black one.
                    .bg(theme.arrow)
                    .border_1()
                    .border_color(theme.arrow_rim)
                    .flex()
                    .items_center()
                    .justify_center()
                    // A fading-out arrow lets the pointer through to the cards beneath.
                    .when(shown, |pill| {
                        pill.occlude()
                            .hover(|style| style.bg(theme.arrow_hover))
                            .cursor_pointer()
                            .role(gpui::Role::Button)
                            .aria_label(if forward {
                                "Scroll right"
                            } else {
                                "Scroll left"
                            })
                            .on_click(cx.listener(move |view, _, _, cx| {
                                rail(view).page(forward, span);
                                cx.notify();
                            }))
                    })
                    .child(
                        svg()
                            .path(if forward {
                                "chevron-right.svg"
                            } else {
                                "chevron-left.svg"
                            })
                            .size(px(20.0))
                            .text_color(theme.ink),
                    ),
            )
    };
    // Where the pointer is, by geometry: the arrow pills occlude the row, so GPUI's hover would
    // drop the moment the pointer reached an arrow. Page scrolls move the row under a still
    // pointer, so wheel events count too.
    let hovering = cx.entity().downgrade();
    let pointer = canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let hovering = hovering.clone();
            let track = move |position: gpui::Point<gpui::Pixels>, cx: &mut gpui::App| {
                hovering
                    .update(cx, |view, cx| {
                        if rail(view).hover(bounds.contains(&position)) {
                            cx.notify();
                        }
                    })
                    .ok();
            };
            let moved = track.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble {
                    moved(event.position, cx);
                }
            });
            window.on_mouse_event(move |event: &gpui::ScrollWheelEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble {
                    track(event.position, cx);
                }
            });
        },
    )
    .absolute()
    .inset_0();
    let mut wrap = div().relative().child(row).child(pointer);
    if arrows[0].0 > 0.0 {
        wrap = wrap.child(arrow(false, col.left - ARROW_GAP - ARROW.0, cx));
    }
    if arrows[1].0 > 0.0 {
        wrap = wrap.child(arrow(true, col.left + col.width + ARROW_GAP, cx));
    }
    wrap
}

#[cfg(test)]
mod tests {
    use super::{Carousel, clamped, landing, rest};

    #[test]
    fn a_swipe_past_either_end_reads_as_the_end_until_layout_clamps_it() {
        use gpui::px;
        // A row that cannot move: swiping either way is no room on either side.
        assert_eq!(clamped(px(30.0), 0.0), 0.0);
        assert_eq!(clamped(px(-30.0), 0.0), 0.0);
        // A long row overshot at its ends.
        assert_eq!(clamped(px(12.0), 800.0), 0.0);
        assert_eq!(clamped(px(-830.0), 800.0), 800.0);
        assert_eq!(clamped(px(-400.0), 800.0), 400.0);
    }

    #[test]
    fn arrows_wait_for_the_pointer_and_hover_reports_only_changes() {
        let mut row = Carousel::default();
        row.settle(100.0, true);
        assert_eq!(row.arrow(true, true), (0.0, false));
        assert!(row.hover(true));
        assert!(!row.hover(true));
        assert!(row.hover(false));
        assert!(!row.hover(false));
    }

    #[test]
    fn a_glide_near_the_end_lands_on_it() {
        let (max, pitch) = (1131.7, 288.0);
        assert_eq!(
            landing(864.0, max, pitch),
            864.0,
            "a whole step short stays"
        );
        assert_eq!(landing(1152.0, max, pitch), max, "past the end clamps");
        assert_eq!(
            landing(1128.0, max, pitch),
            max,
            "a few pixels short is the end"
        );
        assert_eq!(landing(-40.0, max, pitch), 0.0);
        assert_eq!(landing(0.0, 0.0, pitch), 0.0, "a row that fits stays put");
    }

    #[test]
    fn the_row_rests_with_the_nearest_item_on_the_column() {
        let pitch = 372.0;
        assert_eq!(rest(0.0, pitch), 0.0);
        assert_eq!(rest(pitch * 0.4, pitch), 0.0);
        assert_eq!(rest(pitch * 0.6, pitch), pitch);
        assert_eq!(rest(pitch * 2.49, pitch), pitch * 2.0);
    }

    #[test]
    fn a_press_is_a_click_until_it_travels_past_the_slop() {
        let mut row = super::Carousel::default();
        row.press(100.0);
        assert!(row.dragging());
        assert!(!row.drag_to(104.0, true), "a wobble does not move the row");
        assert!(!row.release(), "so the card under it still clicks");
        assert!(!row.dragging());

        row.press(100.0);
        assert!(row.drag_to(40.0, true), "a real drag moves the row");
        assert!(
            row.drag_to(98.0, true),
            "and stays a drag back inside the slop"
        );
        assert!(row.release(), "so releasing does not click the card");
    }

    #[test]
    fn a_release_that_went_unseen_ends_the_drag_on_the_next_move() {
        let mut row = super::Carousel::default();
        row.press(100.0);
        assert!(
            row.drag_to(40.0, false),
            "the view redraws without the drag"
        );
        assert!(!row.dragging());
        assert!(!row.drag_to(10.0, true), "and later moves do nothing");
    }

    #[test]
    fn an_unlaid_row_has_nowhere_to_go() {
        let row = super::Carousel::default();
        assert!(!row.can_go_back());
        assert!(!row.can_go_forward());
    }
}
