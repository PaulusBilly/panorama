use super::*;

impl AppShell {
    /// Open a fresh sign-in form above all route chrome.
    pub fn open_login(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu.set_open(false);
        if self.login.is_none() {
            let shell = cx.weak_entity();
            let state = self.state.clone();
            self.login = Some(cx.new(|cx| LoginDialog::new(shell, state, window, cx)));
        }
        cx.notify();
    }
    /// Release the closed dialog and restore focus to its account trigger.
    pub fn finish_login(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.login = None;
        if let Some(entry) = self
            .cache
            .iter()
            .find(|entry| entry.id == self.history.current_id())
        {
            entry.header.account.focus(window, cx);
        }
        cx.notify();
    }
    /// Animate the menu trigger's active scale.
    pub fn press_trigger(&mut self, pressed: bool) {
        self.trigger.retarget(
            if pressed { 0.96 } else { 1.0 },
            HEADER_COLOR,
            Some(EASE_OUT),
            Instant::now(),
        );
    }
    /// Return keyboard navigation from the final card to visible header controls.
    pub(crate) fn focus_header(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(entry) = self
            .cache
            .iter()
            .find(|entry| entry.id == self.history.current_id())
        {
            entry.header.focus_search(window, cx);
        }
    }
    /// Apply an actual Home scroll sample without touching core or storage.
    pub fn update_home_header(
        &mut self,
        id: u64,
        top: f32,
        height: f32,
        ready: bool,
        cx: &mut Context<Self>,
    ) {
        if !self.is_current(id) {
            return;
        }
        let previous = (self.sticky.hidden, self.sticky.over_hero);
        self.sticky.scroll(top, 60.0, height);
        self.header_slide.retarget(
            if self.sticky.hidden {
                -(60.0 + (f32::from(self.viewport.height) - height))
            } else {
                0.0
            },
            HEADER_SLIDE,
            Some(EASE_IN_OUT),
            Instant::now(),
        );
        let color = HeaderColor::at(self.sticky.over_hero, ready);
        if color != self.header_color {
            self.header_from = (self.header_foreground(), self.header_background());
            self.header_color = color;
            self.logo_mix.retarget(
                if color == HeaderColor::HeroReady {
                    1.0
                } else {
                    0.0
                },
                crate::theme::DURATION_DELIBERATE,
                Some(crate::theme::EASE_EDITORIAL),
                Instant::now(),
            );
            self.header_mix = Tween::fixed(0.0);
            self.header_mix
                .retarget(1.0, HEADER_COLOR, None, Instant::now());
            cx.notify();
        }
        if previous != (self.sticky.hidden, self.sticky.over_hero) {
            self.menu.dismiss();
            cx.notify();
        }
    }
    pub(super) fn header_foreground(&self) -> gpui::Rgba {
        mix(
            self.header_from.0,
            self.header_color.foreground(self.theme),
            self.header_mix.value(Instant::now(), self.reduced_motion),
        )
    }
    pub(super) fn header_background(&self) -> gpui::Rgba {
        mix(
            self.header_from.1,
            self.header_color.background(self.theme),
            self.header_mix.value(Instant::now(), self.reduced_motion),
        )
    }
    pub(super) fn header_y(&self) -> f32 {
        self.header_slide.value(Instant::now(), self.reduced_motion)
    }
    pub(super) fn trigger_scale(&self) -> f32 {
        self.trigger.value(Instant::now(), self.reduced_motion)
    }
}
