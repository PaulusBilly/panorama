use super::*;

impl Render for AppShell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let player = matches!(self.history.current(), Route::Player { .. });
        if player != self.player_transparent {
            self.player_transparent = player;
            // The player fades the traffic lights with its chrome; other routes always show them.
            if !player {
                crate::window_controls::set_opacity(window, 1.0);
            }
            window.set_background_appearance(if player {
                gpui::WindowBackgroundAppearance::Transparent
            } else {
                gpui::WindowBackgroundAppearance::Opaque
            });
            gpui_component::Theme::update(cx, |theme| {
                theme.colors.background = self
                    .theme
                    .canvas
                    .opacity(if player { 0.0 } else { 1.0 })
                    .into()
            });
        }
        let now = Instant::now();
        // A trailer handed on from a card lives while its zoom opens or its film page shows.
        let keep = match (&self.zoom, self.history.current()) {
            (Some(zoom), _) if zoom.direction == crate::zoom::Direction::Open => {
                Some(zoom.card.film.id.clone())
            }
            (None, Route::Film { id, .. }) => Some(id.clone()),
            _ => None,
        };
        crate::trailer_preview::Handoff::settle(keep.as_deref(), window, cx);
        if self.viewport != window.viewport_size() {
            self.viewport = window.viewport_size();
            self.menu.dismiss();
            self.join.dismiss();
        }
        let moving = !self.reduced_motion && (self.menu.moving(false) || self.join.moving(false));
        let real_playback =
            self.args.play_file.is_some() || self.args.play_url.is_some() || self.args.play_torrent;
        let playback_ready = !real_playback
            || self
                .state
                .read(cx)
                .player
                .as_ref()
                .is_some_and(|player| player.capture_ready);
        let capture_waiting = self.screenshot.is_some() && !self.capture_scheduled;
        let settling = self.zoom.is_some()
            || (self.state.read(cx).quiet.as_ref()).is_some_and(crate::zoom::Quiet::active);
        if moving || capture_waiting || settling {
            cx.on_next_frame(window, |_, _, cx| cx.notify());
            window.request_animation_frame();
        }
        if capture_waiting
            && !moving
            && playback_ready
            && now.duration_since(self.launched)
                >= Duration::from_millis(if self.args.live { 8000 } else { 1600 })
            && let Some(path) = self.screenshot.take()
        {
            self.capture_scheduled = true;
            debug::screenshot::schedule(window, cx, path, self.outcome.clone(), self.state.clone());
        }
        let body = div()
            .absolute()
            .inset_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .children(self.mounted.as_ref().map(|mounted| {
                div()
                    .id(("route", mounted.id))
                    .flex_1()
                    .min_h_0()
                    .child(mounted.view.clone())
            }));
        let view = crate::home_layout::ViewSettings {
            theme: self.theme,
            keyboard: self.keyboard_navigation,
            reduced: self.reduced_motion,
            active: self.login.is_none(),
            width: f32::from(window.viewport_size().width),
            height: f32::from(window.viewport_size().height),
        };
        let route = self.history.current();
        let account = self.state.read(cx).account.clone();
        let in_party =
            (self.state.read(cx).party.as_ref()).is_some_and(|party| party.view.ended().is_none());
        let sidebar = route.has_sidebar().then(|| {
            self.sidebar.render(
                route.destination(),
                &account,
                crate::sidebar::SidebarStatus {
                    account_menu: self.menu.open,
                    join: self.join.open,
                    in_party,
                },
                view,
                window,
                cx,
            )
        });
        let menu = self
            .menu
            .visible(self.reduced_motion)
            .then(|| self.menu.render(view, &account, window, cx));
        let join = (route.has_sidebar() && self.join.visible(self.reduced_motion))
            .then(|| self.join_popover(view, in_party, window, cx));
        div()
            .id("panorama-shell")
            .relative()
            .size_full()
            .when(!player, |shell| shell.bg(self.theme.canvas))
            .text_color(self.theme.ink)
            .font(assets::font())
            .font_features(gpui::FontFeatures(std::sync::Arc::new(vec![(
                "kern".into(),
                1,
            )])))
            .when(capture_waiting, |shell| {
                let outcome = self.outcome.clone();
                shell.child(
                    gpui::canvas(
                        move |_, window, cx| {
                            if let Err(error) = debug::check_default_font(window) {
                                let _ = outcome.send(Err(error));
                                cx.quit();
                            }
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size(px(0.0)),
                )
            })
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(|shell, event: &KeyDownEvent, window, cx| {
                if crate::routes::player::pointer::is_sentinel(event) {
                    // The idle-pointer hide key: consumed here, seen by nothing else.
                    window.prevent_default();
                    cx.stop_propagation();
                    return;
                }
                if let Route::Player { id } = shell.history.current()
                    && shell.lobby.is_none()
                    && let Some(player) = shell.mounted.as_ref().and_then(|mounted| {
                        mounted
                            .view
                            .clone()
                            .downcast::<routes::player::Player>()
                            .ok()
                    })
                {
                    let id = id.clone();
                    let action = player.update(cx, |player, cx| player.key_down(event, window, cx));
                    let handled = action.is_some();
                    if let Some(crate::routes::player::keyboard::KeyAction::Close) = action {
                        if let Some(host) = &shell.state.read(cx).playback_host {
                            host.send(crate::playback::host::Command::Stop);
                        }
                        if window.is_fullscreen() {
                            window.toggle_fullscreen();
                        }
                        shell.back_to_film(id, window, cx);
                    }
                    if handled {
                        window.prevent_default();
                        cx.stop_propagation();
                    }
                }
                if navigation::moves_focus(&event.keystroke) {
                    shell.keyboard_navigation = true;
                }
                cx.notify();
            }))
            .on_key_down(cx.listener(Self::key_down))
            .on_scroll_wheel(cx.listener(|shell, _, _, cx| {
                if shell.menu.open || shell.join.open {
                    shell.menu.dismiss();
                    shell.join.dismiss();
                    cx.notify();
                }
            }))
            .on_any_mouse_down(
                cx.listener(|shell, event: &gpui::MouseDownEvent, window, cx| {
                    shell.keyboard_navigation = false;
                    cx.notify();
                    match event.button {
                        gpui::MouseButton::Navigate(NavigationDirection::Back) => {
                            shell.back(window, cx)
                        }
                        gpui::MouseButton::Navigate(NavigationDirection::Forward) => {
                            shell.forward(window, cx)
                        }
                        _ => return,
                    }
                    cx.stop_propagation();
                }),
            )
            .child(body)
            .children(sidebar)
            .when(self.menu.open, |shell| shell.child(self.menu.catcher(cx)))
            .children(menu)
            .when(self.join.open, |shell| shell.child(self.join_catcher(cx)))
            .children(join)
            .children(
                self.zoom
                    .as_ref()
                    .map(|zoom| zoom.render(window.viewport_size(), self.theme, window, cx)),
            )
            // Drawn last on both platforms, taking no layout space.
            .when(!window.is_fullscreen(), |shell| {
                shell.child(
                    self.titlebar
                        .render(self.theme, self.keyboard_navigation, window),
                )
            })
            .when_some(self.lobby.clone(), |shell, lobby| shell.child(lobby))
            .when_some(self.login.clone(), |shell, login| shell.child(login))
    }
}
