use super::*;

impl Render for AppShell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        if self.viewport != window.viewport_size() {
            self.viewport = window.viewport_size();
            self.menu.dismiss();
        }
        self.presence
            .retain(|presence| !presence.exiting || !presence.settled(now));
        let moving = !self.reduced_motion
            && (self.presence.iter().any(|presence| !presence.settled(now))
                || self.header_mix.moving(now, false)
                || self.logo_mix.moving(now, false)
                || self.header_slide.moving(now, false)
                || self.trigger.moving(now, false)
                || self.search.motion.moving(now, false)
                || self.menu.moving(false));
        let capture_waiting = self.screenshot.is_some() && !self.capture_scheduled;
        if moving || capture_waiting {
            cx.on_next_frame(window, |_, _, cx| cx.notify());
            window.request_animation_frame();
        }
        if capture_waiting
            && !moving
            && now.duration_since(self.launched) >= Duration::from_millis(1600)
            && let Some(path) = self.screenshot.take()
        {
            self.capture_scheduled = true;
            debug::screenshot::schedule(window, cx, path, self.outcome.clone());
        }
        let mut body = div()
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_hidden();
        for presence in &self.presence {
            let pose = if self.reduced_motion {
                Pose {
                    opacity: 1.0,
                    y: 0.0,
                }
            } else {
                presence.pose(now)
            };
            let mounted = &presence.view;
            let mut layer = div()
                .id(("route-presence", mounted.id))
                .absolute()
                .inset_0()
                .top(px(pose.y))
                .bottom(px(-pose.y))
                .opacity(pose.opacity)
                .flex()
                .flex_col();
            layer = layer.child(
                div()
                    .flex_1()
                    .min_h_0()
                    .when(matches!(mounted.route, Route::Addons), |view| {
                        view.pt(px(60.0))
                    })
                    .child(mounted.view.clone()),
            );
            if !matches!(mounted.route, Route::Player { .. }) {
                let home = mounted.route == Route::Home;
                let film = matches!(mounted.route, Route::Film { .. });
                let band_route = home || matches!(mounted.route, Route::Search { .. });
                let band_open = band_route && self.search.open;
                layer = layer.child(mounted.header.render(
                    &mounted.route,
                    HeaderAppearance {
                        theme: self.theme,
                        active: !presence.exiting && self.login.is_none() && !self.sticky.hidden,
                        keyboard: self.keyboard_navigation,
                        foreground: if band_route || film {
                            self.header_foreground()
                        } else {
                            self.theme.ink
                        },
                        chrome_opacity: if band_route {
                            1.0 - self.search.motion.value(now, self.reduced_motion)
                        } else {
                            1.0
                        },
                        search_open: band_open,
                        logo: if home || film {
                            gpui::Rgba {
                                r: self.logo_mix.value(now, self.reduced_motion),
                                g: self.logo_mix.value(now, self.reduced_motion),
                                b: self.logo_mix.value(now, self.reduced_motion),
                                a: 1.0,
                            }
                        } else {
                            gpui::black().into()
                        },
                        background: if band_route || film {
                            self.header_background()
                        } else {
                            self.theme.canvas
                        },
                        y: self.header_y(),
                        pressed: self.trigger_scale(),
                    },
                    window,
                    cx,
                ));
                if band_route {
                    layer = layer.child(
                        div()
                            .absolute()
                            .left_0()
                            .top(px(60.0 + self.header_y()))
                            .w_full()
                            .bg(self.header_background())
                            .child(self.render_search_band(
                                !presence.exiting && self.login.is_none() && !self.sticky.hidden,
                                window,
                                cx,
                            ))
                            .when(matches!(mounted.route, Route::Search { .. }), |band| {
                                band.pb(px(20.0))
                            }),
                    );
                }
            }
            if presence.exiting {
                layer = layer.child(div().absolute().inset_0().occlude());
            }
            body = body.child(layer);
        }
        if self.menu.visible(self.reduced_motion) {
            body = body.child(self.menu.render(
                crate::home_layout::ViewSettings {
                    theme: self.theme,
                    keyboard: self.keyboard_navigation,
                    reduced: self.reduced_motion,
                    active: true,
                    width: f32::from(window.viewport_size().width),
                    height: f32::from(window.viewport_size().height),
                },
                matches!(self.state.read(cx).account, Account::SignedIn(_)),
                window,
                cx,
            ));
        }
        div()
            .id("panorama-shell")
            .size_full()
            .flex()
            .flex_col()
            .bg(self.theme.canvas)
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
                if shell.login.is_none()
                    && event.keystroke.key == "escape"
                    && (shell.search.open
                        || matches!(shell.history.current(), Route::Search { .. }))
                {
                    shell.clear_search(window, cx);
                    window.prevent_default();
                    cx.stop_propagation();
                }
                shell.keyboard_navigation = true;
                cx.notify();
            }))
            .on_key_down(cx.listener(Self::key_down))
            .on_scroll_wheel(cx.listener(|shell, _, _, cx| {
                if shell.menu.open {
                    shell.menu.dismiss();
                    cx.notify();
                }
            }))
            .on_any_mouse_down(
                cx.listener(|shell, event: &gpui::MouseDownEvent, window, cx| {
                    shell.keyboard_navigation = false;
                    if shell.menu.open {
                        let width = f32::from(window.viewport_size().width);
                        let inset = (width - crate::theme::content_width(width)) * 0.5;
                        let x = f32::from(event.position.x);
                        let y = f32::from(event.position.y)
                            - if window.is_fullscreen() { 0.0 } else { 32.0 };
                        let in_popup = x >= width - inset - 192.0
                            && x <= width - inset
                            && (68.0..=132.0).contains(&y);
                        let in_trigger = x >= width - inset - 40.0
                            && x <= width - inset
                            && (20.0..=60.0).contains(&y);
                        if !in_popup && !in_trigger {
                            shell.menu.set_open(false);
                        }
                    }
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
            .when(!window.is_fullscreen(), |shell| {
                shell.child(
                    self.titlebar
                        .render(self.theme, self.keyboard_navigation, window),
                )
            })
            .child(body)
            .when_some(self.login.clone(), |shell, login| shell.child(login))
    }
}
