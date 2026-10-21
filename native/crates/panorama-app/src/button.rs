use crate::theme::{Theme, caps, focus_ring, typography::BUTTON};
use gpui::{
    Animation, AnimationExt, App, ClickEvent, ElementId, FocusHandle, IntoElement, RenderOnce,
    SharedString, Transformation, Window, div, prelude::*, px, svg,
};
use std::{rc::Rc, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    /// Accent fill: the one main action of a screen.
    Primary,
    Glass,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    Regular,
    Compact,
}
impl Size {
    fn height(self) -> f32 {
        match self {
            Self::Regular => 52.0,
            Self::Compact => 40.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Play,
    Plus,
    Check,
    ArrowLeft,
    ArrowUp,
    ArrowDown,
    Search,
    Home,
    Addons,
    Person,
    Retry,
    Close,
}
impl Icon {
    pub fn path(self) -> &'static str {
        match self {
            Self::Play => "player-play-filled.svg",
            Self::Plus => "plus.svg",
            Self::Check => "check.svg",
            Self::ArrowLeft => "arrow-left.svg",
            Self::ArrowUp => "arrow-up.svg",
            Self::ArrowDown => "arrow-down.svg",
            Self::Search => "search.svg",
            Self::Home => "home.svg",
            Self::Addons => "addons.svg",
            Self::Person => "person.svg",
            Self::Retry => "refresh.svg",
            Self::Close => "x.svg",
        }
    }
}

enum Face {
    Label { leading: Option<Icon> },
    Round(Icon),
}

type OnClick = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// One pill or round control. Construction requires a label; the round variant takes it as the
/// accessible label because nothing is visible.
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    variant: Variant,
    size: Size,
    face: Face,
    label: SharedString,
    aria_label: Option<SharedString>,
    disabled: bool,
    busy: bool,
    progress: Option<f32>,
    focus: Option<(FocusHandle, bool)>,
    tab_stop: bool,
    reduced_motion: bool,
    on_click: Option<OnClick>,
}

impl Button {
    fn new(
        id: impl Into<ElementId>,
        variant: Variant,
        face: Face,
        label: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            variant,
            size: Size::Regular,
            face,
            label: label.into(),
            aria_label: None,
            disabled: false,
            busy: false,
            progress: None,
            focus: None,
            tab_stop: true,
            reduced_motion: false,
            on_click: None,
        }
    }
    /// An accent pill. Pass the label in sentence case; it is shown in caps.
    pub fn primary(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self::new(id, Variant::Primary, Face::Label { leading: None }, label)
    }
    pub fn glass(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self::new(id, Variant::Glass, Face::Label { leading: None }, label)
    }
    /// A translucent circle showing only `icon`; `label` is its accessible name.
    pub fn round(id: impl Into<ElementId>, icon: Icon, label: impl Into<SharedString>) -> Self {
        Self::new(id, Variant::Glass, Face::Round(icon), label)
    }
    /// Put `icon` before the label. Ignored by round buttons.
    pub fn leading(mut self, icon: Icon) -> Self {
        if let Face::Label { leading } = &mut self.face {
            *leading = Some(icon);
        }
        self
    }
    /// Name the control for assistive technology when the visible label lacks context.
    pub fn aria_label(mut self, label: impl Into<SharedString>) -> Self {
        self.aria_label = Some(label.into());
        self
    }
    pub fn compact(mut self) -> Self {
        self.size = Size::Compact;
        self
    }
    /// Dim the control and ignore clicks.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    /// Swap the leading icon for a spinner and ignore clicks; the label stays.
    pub fn busy(mut self, busy: bool) -> Self {
        self.busy = busy;
        self
    }
    /// Append a played-fraction bar after the label, for resume actions.
    pub fn progress(mut self, fraction: f32) -> Self {
        self.progress = Some(fraction.clamp(0.0, 1.0));
        self
    }
    /// Track `handle` and draw the keyboard focus ring while `keyboard` navigation is active.
    pub fn focus(mut self, handle: &FocusHandle, keyboard: bool) -> Self {
        self.focus = Some((handle.clone(), keyboard));
        self
    }
    /// Whether the button takes part in Tab order, false while its route is inactive.
    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.tab_stop = tab_stop;
        self
    }
    pub fn reduced_motion(mut self, reduced: bool) -> Self {
        self.reduced_motion = reduced;
        self
    }
    /// Run `f` on activation, unless disabled or busy.
    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        let theme = Theme::default();
        let height = self.size.height();
        let regular = self.size == Size::Regular;
        let enabled = !self.disabled && !self.busy;
        let (fill, hover, pressed) = match self.variant {
            Variant::Primary => (theme.accent, theme.accent_hover, theme.accent_pressed),
            Variant::Glass => (theme.glass, theme.glass_hover, theme.glass),
        };
        let glyph = |path: &'static str, size: f32| {
            svg()
                .path(path)
                .size(px(size))
                .flex_shrink_0()
                .text_color(theme.ink)
        };
        let icon_size = if regular { 20.0 } else { 16.0 };
        let spinner = self.busy.then(|| {
            let spinner = glyph("loader-2.svg", icon_size);
            if self.reduced_motion {
                spinner.into_any_element()
            } else {
                spinner
                    .with_animation(
                        "spin",
                        Animation::new(Duration::from_secs(1)).repeat(),
                        |icon, delta| {
                            icon.with_transformation(Transformation::rotate(gpui::radians(
                                delta * std::f32::consts::TAU,
                            )))
                        },
                    )
                    .into_any_element()
            }
        });
        let mut button = div()
            .id(self.id)
            .role(gpui::Role::Button)
            .aria_label(self.aria_label.unwrap_or_else(|| self.label.clone()))
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .h(px(height))
            .rounded_full()
            .bg(fill)
            .text_color(theme.ink)
            .when(!enabled, |button| button.opacity(0.5))
            .when(enabled, |button| {
                button
                    .cursor_pointer()
                    .hover(|style| style.bg(hover))
                    .active(|style| style.bg(pressed))
            });
        button = match self.face {
            Face::Round(icon) => button
                .w(px(height))
                .child(spinner.unwrap_or_else(|| glyph(icon.path(), icon_size).into_any_element())),
            Face::Label { leading } => BUTTON
                .apply(button)
                .px(px(if regular { 28.0 } else { 20.0 }))
                .gap(px(if regular { 10.0 } else { 8.0 }))
                .children(spinner.or_else(|| {
                    leading.map(|icon| glyph(icon.path(), icon_size).into_any_element())
                }))
                .child(caps(&self.label))
                .when_some(self.progress, |button, fraction| {
                    button.child(
                        div()
                            .ml(px(4.0))
                            .w(px(64.0))
                            .h(px(4.0))
                            .rounded_full()
                            .overflow_hidden()
                            .bg(theme.progress_track)
                            .child(
                                div()
                                    .h_full()
                                    .w(px((64.0 * fraction).max(4.0)))
                                    .rounded_full()
                                    .bg(theme.progress_fill),
                            ),
                    )
                }),
        };
        if let Some(on_click) = self.on_click.filter(|_| enabled) {
            button = button.on_click(move |event, window, cx| on_click(event, window, cx));
        }
        match self.focus {
            Some((handle, keyboard)) => focus_ring(
                button,
                &handle,
                theme,
                self.tab_stop && enabled,
                keyboard,
                height * 0.5,
                window,
            ),
            None => button,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::AssetSource;
    #[test]
    fn every_icon_is_bundled() {
        for icon in [
            Icon::Play,
            Icon::Plus,
            Icon::Check,
            Icon::ArrowLeft,
            Icon::ArrowUp,
            Icon::ArrowDown,
            Icon::Search,
            Icon::Home,
            Icon::Addons,
            Icon::Person,
            Icon::Retry,
            Icon::Close,
        ] {
            assert!(
                crate::assets::Assets.load(icon.path()).unwrap().is_some(),
                "{icon:?}"
            );
        }
    }
    #[test]
    fn labels_are_shown_in_caps() {
        assert_eq!(caps("Load more").as_ref(), "LOAD MORE");
    }
}
