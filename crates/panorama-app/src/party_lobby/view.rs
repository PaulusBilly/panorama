use super::{
    Command, PartyLobby, Readiness, Side, invite, is_ready,
    source_rows::{self, Choice},
};
use crate::{
    button::{Button, Icon, title_case},
    party::{Blocked, Link, PartySync, PartyView, RosterRow},
    theme::{
        Theme, caps, focus_ring,
        typography::{BUTTON, CAPTION, LABEL, TypeStyle},
    },
};
use gpui::{
    Animation, AnimationExt, BoxShadow, ClipboardItem, Context, Div, FocusHandle, Render, Rgba,
    SharedString, Stateful, Transformation, Window, div, linear_color_stop, linear_gradient, point,
    prelude::*, px, relative, svg,
};
use panorama_core::{
    addons::FilmDetails,
    party::{Gate, MemberState},
};
use std::time::{Duration, Instant};

const WIDTH: f32 = 560.0;
const HEADER: f32 = 252.0;
const GUTTER: f32 = 28.0;
const ROW: f32 = 44.0;
const TITLE: TypeStyle = TypeStyle {
    size: 30.0,
    weight: 500.0,
    line_height: 1.05,
};
const SMALL: TypeStyle = TypeStyle {
    size: 11.0,
    ..LABEL
};

/// What the lobby needs from the shell to draw a frame.
#[derive(Clone, Copy)]
struct Look {
    theme: Theme,
    reduced: bool,
    keyboard: bool,
}

impl Render for PartyLobby {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let look = self
            .shell
            .upgrade()
            .map(|shell| {
                let shell = shell.read(cx);
                Look {
                    theme: shell.theme,
                    reduced: shell.reduced_motion,
                    keyboard: shell.keyboard_navigation,
                }
            })
            .unwrap_or(Look {
                theme: Theme::default(),
                reduced: false,
                keyboard: true,
            });
        let now = Instant::now();
        let progress = self.presence.value(now, look.reduced);
        let scrim = self.scrim.value(now, look.reduced);
        let closing = self.exit.is_some();
        if self.presence.moving(now, look.reduced)
            || self.scrim.moving(now, look.reduced)
            || closing
        {
            cx.on_next_frame(window, |lobby, window, cx| {
                let reduced = lobby
                    .shell
                    .upgrade()
                    .is_some_and(|shell| shell.read(cx).reduced_motion);
                match lobby.exit {
                    Some(_) if !lobby.presence.moving(Instant::now(), reduced) => {
                        let shell = lobby.shell.clone();
                        let _ = shell.update(cx, |shell, cx| shell.finish_party_lobby(window, cx));
                    }
                    _ => cx.notify(),
                }
            });
            window.request_animation_frame();
        }
        let covered = self
            .shell
            .upgrade()
            .is_some_and(|shell| shell.read(cx).covers_lobby());
        if !closing && !covered && !self.root.contains_focused(window, cx) {
            // The player opening under a friend's lobby takes focus; the lobby keeps it.
            cx.defer_in(window, |lobby, window, cx| lobby.root.focus(window, cx));
        }
        let viewport = window.viewport_size();
        let width = (f32::from(viewport.width) - 40.0).min(WIDTH);
        let height = f32::from(viewport.height) - crate::titlebar::SAFE_TOP - 40.0;
        let party = self.party(cx).cloned();
        let film = self.header_film(cx);
        let title = film.as_ref().map(|film| film.name.clone()).or_else(|| {
            let state = self.state.read(cx);
            Some(state.party.as_ref()?.media()?.title.clone())
        });
        let header = self.header(film.as_ref(), title, look, window, cx);
        let mut body = div().px(px(GUTTER)).pt(px(6.0)).flex().flex_col();
        if let Some(view) = &party {
            body = body.child(self.people(view, look, cx));
        }
        body = match &self.side {
            Side::Host(_) => body.child(self.source(look, window, cx)),
            Side::Member => body.child(self.follow(party.as_ref(), look, window, cx)),
        };
        let footer = self.footer(party.as_ref(), look, window, cx);
        let panel = div()
            .relative()
            .w(px(width))
            .max_h(px(height))
            .flex()
            .flex_col()
            .bg(look.theme.surface)
            .text_color(look.theme.ink)
            .opacity(progress)
            .top(px(if look.reduced {
                0.0
            } else {
                12.0 * (1.0 - progress)
            }))
            .shadow(vec![BoxShadow {
                inset: false,
                color: look.theme.shadow.into(),
                offset: point(px(0.0), px(24.0)),
                blur_radius: px(80.0),
                spread_radius: px(0.0),
            }])
            .child(
                div()
                    .id("party-lobby-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(header)
                    .child(body),
            )
            .child(footer);
        // A friend's player loads under the lobby; it stays hidden until the host starts.
        let (label, backdrop) = match &self.side {
            Side::Host(_) => ("Watch party lobby", look.theme.scrim.opacity(0.72)),
            Side::Member => ("Watch party", look.theme.player_canvas),
        };
        div()
            .id("party-lobby")
            .absolute()
            .inset_0()
            .occlude()
            .key_context("PanoramaLobby")
            .track_focus(&self.root)
            .role(gpui::Role::Dialog)
            .aria_label(label)
            .on_action(cx.listener(|lobby, _: &super::NextControl, window, cx| {
                lobby.cycle(false, window, cx)
            }))
            .on_action(
                cx.listener(|lobby, _: &super::PreviousControl, window, cx| {
                    lobby.cycle(true, window, cx)
                }),
            )
            .on_action(cx.listener(|lobby, _: &super::CloseLobby, window, cx| {
                lobby.act(Command::Escape, window, cx)
            }))
            .child(div().absolute().inset_0().bg(backdrop).opacity(scrim))
            .child(
                div()
                    .relative()
                    .size_full()
                    .pt(px(crate::titlebar::SAFE_TOP))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(panel),
            )
    }
}

impl PartyLobby {
    /// The film's backdrop under the card's scrims, with its title and credits.
    fn header(
        &mut self,
        film: Option<&FilmDetails>,
        title: Option<String>,
        look: Look,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = look.theme;
        let art = film.and_then(|film| film.background.clone().or_else(|| film.poster.clone()));
        let poster = film.is_some_and(|film| film.background.is_none());
        let bounds = (WIDTH, HEADER);
        let image = art.as_ref().and_then(|url| {
            let services = self.state.read(cx).services.clone()?;
            let scale = window.scale_factor();
            let images = self.state.read(cx).images.clone();
            let first = self.art.as_ref() != Some(url);
            images.update(cx, |images, cx| {
                if first {
                    images.prefetch(url.as_str(), bounds, &services, window, cx);
                }
                images.cached(url.as_str(), bounds, scale).or_else(|| {
                    Some(crate::image_cache::CachedImage {
                        image: images.newest(url.as_str())?,
                        loaded: Instant::now() - Duration::from_secs(1),
                        tint: None,
                    })
                })
            })
        });
        self.art = art;
        let scrim = |from: Rgba, to: Rgba, start: f32, end: f32| {
            div().absolute().inset_0().bg(linear_gradient(
                180.0,
                linear_color_stop(from, start),
                linear_color_stop(to, end),
            ))
        };
        let shade = theme.canvas.opacity(0.58);
        div()
            .relative()
            .h(px(HEADER))
            .flex_shrink_0()
            .overflow_hidden()
            .bg(theme.artwork_empty)
            .when_some(image, |header, image| {
                let opacity = crate::film_card::fade_in(&image, look.reduced);
                header.child(crate::hero::cover(image.image, bounds, poster, 1.0).opacity(opacity))
            })
            .child(scrim(shade.opacity(0.0), shade, 0.3, 0.62))
            .child(scrim(theme.surface.opacity(0.0), theme.surface, 0.62, 1.0))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .border_1()
                    .border_b_0()
                    .border_color(theme.image_outline),
            )
            .child(
                div().absolute().top(px(14.0)).right(px(14.0)).child(
                    Button::round(
                        "party-lobby-close",
                        Icon::Close,
                        match (self.is_host(), self.readiness(cx).friends) {
                            (true, 0) => "Close and end the party",
                            (true, _) => "Close and keep the party",
                            (false, _) => "Close and leave the party",
                        },
                    )
                    .compact()
                    .focus(&self.close, look.keyboard)
                    .tab_stop(self.exit.is_none())
                    .on_click(
                        cx.listener(|lobby, _, window, cx| lobby.act(Command::Escape, window, cx)),
                    ),
                ),
            )
            .child(
                div()
                    .absolute()
                    .left(px(GUTTER))
                    .right(px(GUTTER))
                    .bottom(px(18.0))
                    .flex()
                    .flex_col()
                    .text_color(theme.ink)
                    .child(
                        TITLE
                            .apply(div())
                            .line_clamp(2)
                            .child(caps(title.as_deref().unwrap_or("Watch party"))),
                    )
                    .when_some(film, |block, film| {
                        block.child(crate::hero::credits(film, 10.0))
                    }),
            )
    }

    fn people(&self, view: &PartyView, look: Look, cx: &mut Context<Self>) -> Div {
        let theme = look.theme;
        let count = view.roster.len();
        let mut section = section(
            "People",
            Some(match count {
                1 => "Just you so far".to_owned(),
                count => format!("{count} in the party"),
            }),
            theme,
            true,
        );
        for row in &view.roster {
            section = section.child(person(row, view, look));
        }
        if let Side::Host(_) = self.side {
            let link = invite(Some(view));
            let ready = link.is_some();
            section = section.child(
                div()
                    .mt(px(8.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h(px(40.0))
                            .px(px(12.0))
                            .flex()
                            .items_center()
                            .bg(theme.canvas)
                            .text_color(theme.ink_muted)
                            .text_size(px(12.0))
                            .child(
                                div().min_w_0().truncate().child(
                                    link.clone()
                                        .unwrap_or_else(|| "Getting an invite link ready".into()),
                                ),
                            ),
                    )
                    .child(
                        Button::glass(
                            "party-lobby-copy",
                            if self.copied {
                                "Invite link copied"
                            } else {
                                "Copy invite link"
                            },
                        )
                        .leading(if self.copied { Icon::Check } else { Icon::Copy })
                        .disabled(!ready)
                        .focus(&self.copy, look.keyboard)
                        .tab_stop(ready && self.exit.is_none())
                        .on_click(cx.listener(move |lobby, _, _, cx| {
                            if let Some(link) = link.clone() {
                                cx.write_to_clipboard(ClipboardItem::new_string(link));
                                lobby.copied = true;
                                cx.notify();
                            }
                        })),
                    ),
            );
        }
        section
    }

    /// The host's source, and the list to change it.
    fn source(&mut self, look: Look, window: &Window, cx: &mut Context<Self>) -> Div {
        let theme = look.theme;
        let Side::Host(selection) = &self.side else {
            return div();
        };
        let current = source_rows::current(selection);
        let choices = if self.changing {
            source_rows::choices(selection)
        } else {
            Vec::new()
        };
        let count = choices.iter().map(|group| group.rows.len()).sum::<usize>();
        while self.sources.len() < count {
            self.sources.push(cx.focus_handle());
        }
        let change = quiet(
            "party-lobby-change",
            "Change",
            Some("chevron-down.svg"),
            &self.change,
            look,
            self.exit.is_none(),
            window,
        )
        .aria_expanded(self.changing)
        .on_click(cx.listener(|lobby, _, _, cx| {
            lobby.changing = !lobby.changing;
            cx.notify();
        }));
        let row = div()
            .min_h(px(56.0))
            .px(px(14.0))
            .py(px(10.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .bg(theme.canvas)
            .when_some(
                current.as_ref().and_then(|c| c.quality.clone()),
                |row, q| {
                    row.child(
                        LABEL
                            .apply(div())
                            .text_size(px(12.0))
                            .line_height(relative(1.0))
                            .px(px(7.0))
                            .py(px(3.0))
                            .border_1()
                            .border_color(theme.rule)
                            .flex_shrink_0()
                            .child(q),
                    )
                },
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        LABEL.apply(div()).truncate().child(
                            current
                                .as_ref()
                                .map_or_else(|| "No source".into(), |c| c.label.clone()),
                        ),
                    )
                    .when_some(current.as_ref().map(|c| c.detail.clone()), |s, detail| {
                        s.child(
                            SMALL
                                .apply(div())
                                .text_color(theme.ink_muted)
                                .truncate()
                                .child(detail),
                        )
                    }),
            )
            .child(change);
        let mut list = div().flex().flex_col();
        let mut slot = 0;
        for group in choices {
            list = list.child(
                SMALL
                    .apply(div())
                    .pt(px(12.0))
                    .pb(px(4.0))
                    .text_color(theme.ink_muted)
                    .child(group.addon),
            );
            for choice in group.rows {
                let handle = self.sources[slot].clone();
                list = list.child(self.choice(slot, choice, &handle, look, window, cx));
                slot += 1;
            }
        }
        let pick_film = quiet(
            "party-lobby-pick-film",
            "Pick another film",
            None,
            &self.pick_film,
            look,
            self.exit.is_none(),
            window,
        )
        .on_click(
            cx.listener(|lobby, _, window, cx| lobby.act(Command::PickAnotherFilm, window, cx)),
        );
        let end = quiet(
            "party-lobby-end",
            "End party",
            None,
            &self.leave,
            look,
            self.exit.is_none(),
            window,
        )
        .on_click(cx.listener(|lobby, _, window, cx| lobby.act(Command::LeaveParty, window, cx)));
        // The two ways out of the lobby, apart from starting: pulled out by their own padding
        // so the labels line up with the rows above.
        section("Source", None, theme, false)
            .child(row)
            .when(self.changing, |section| section.child(list))
            .child(
                div()
                    .mt(px(8.0))
                    .mx(px(-10.0))
                    .flex()
                    .justify_between()
                    .child(pick_film)
                    .child(end),
            )
    }

    fn choice(
        &self,
        slot: usize,
        choice: Choice,
        handle: &FocusHandle,
        look: Look,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = look.theme;
        let source = choice.source;
        let enabled = !choice.disabled && !choice.selected;
        let row = div()
            .id(("party-lobby-source", slot))
            .role(gpui::Role::RadioButton)
            .aria_label(choice.label.clone())
            .aria_toggled(if choice.selected {
                gpui::accesskit::Toggled::True
            } else {
                gpui::accesskit::Toggled::False
            })
            .min_h(px(ROW))
            .px(px(14.0))
            .py(px(6.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .when(choice.selected, |row| row.bg(theme.selected))
            .when(choice.disabled, |row| row.opacity(0.5))
            .when(enabled, |row| {
                row.cursor_pointer()
                    .hover(move |style| style.bg(theme.hover))
                    .on_click(cx.listener(move |lobby, _, _, cx| lobby.choose_source(source, cx)))
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(LABEL.apply(div()).truncate().child(choice.label))
                    .when_some(choice.detail, |text, detail| {
                        text.child(
                            SMALL
                                .apply(div())
                                .text_color(theme.ink_muted)
                                .truncate()
                                .child(detail),
                        )
                    }),
            )
            .when(choice.selected, |row| {
                row.child(svg().path("check.svg").size(px(16.0)).text_color(theme.ink))
            });
        focus_ring(
            row,
            handle,
            theme,
            !choice.disabled && self.exit.is_none(),
            look.keyboard,
            0.0,
            window,
        )
    }

    /// A friend's switches for following the host's subtitles and audio.
    fn follow(
        &self,
        view: Option<&PartyView>,
        look: Look,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let sync = view.map_or_else(|| self.state.read(cx).party_sync(), |view| view.sync);
        let host = view
            .and_then(|view| view.roster.iter().find(|row| row.member.is_host))
            .map_or_else(|| "the host".to_owned(), |row| row.member.name.clone());
        let picked = self
            .state
            .read(cx)
            .party
            .as_ref()
            .and_then(|party| party.host_tracks())
            .and_then(|tracks| tracks.subtitle.as_ref())
            .and_then(|track| track.title.clone().or_else(|| track.language.clone()));
        let subtitles = match (sync.subtitles, picked) {
            (true, Some(track)) => format!("{track}, as {host} picks them"),
            (true, None) => format!("As {host} picks them"),
            (false, _) => "Choose your own subtitles".to_owned(),
        };
        let audio = if sync.audio {
            format!("As {host} picks it")
        } else {
            "Choose your own audio track".to_owned()
        };
        section("Follow the host", None, look.theme, false)
            .child(self.switch(
                "party-lobby-subtitles",
                "Subtitles",
                subtitles,
                sync.subtitles,
                &self.subtitles,
                PartySync {
                    subtitles: !sync.subtitles,
                    ..sync
                },
                look,
                window,
                cx,
            ))
            .child(self.switch(
                "party-lobby-audio",
                "Audio",
                audio,
                sync.audio,
                &self.audio,
                PartySync {
                    audio: !sync.audio,
                    ..sync
                },
                look,
                window,
                cx,
            ))
    }

    #[allow(clippy::too_many_arguments)]
    fn switch(
        &self,
        id: &'static str,
        label: &'static str,
        caption: String,
        on: bool,
        handle: &FocusHandle,
        flipped: PartySync,
        look: Look,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = look.theme;
        let row = div()
            .id(id)
            .role(gpui::Role::Switch)
            .aria_label(label)
            .aria_toggled(if on {
                gpui::accesskit::Toggled::True
            } else {
                gpui::accesskit::Toggled::False
            })
            .h(px(ROW))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.0))
            .cursor_pointer()
            .on_click(cx.listener(move |lobby, _, _, cx| {
                lobby
                    .state
                    .update(cx, |state, cx| state.set_party_sync(flipped, cx));
            }))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(LABEL.apply(div()).child(label))
                    .child(
                        SMALL
                            .apply(div())
                            .text_color(theme.ink_muted)
                            .truncate()
                            .child(caption),
                    ),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .w(px(36.0))
                    .h(px(20.0))
                    .p(px(2.0))
                    .rounded_full()
                    .flex()
                    .when(on, |track| track.justify_end().bg(theme.accent))
                    .when(!on, |track| track.bg(theme.surface_strong))
                    .child(div().size(px(16.0)).rounded_full().bg(theme.ink)),
            );
        focus_ring(
            row,
            handle,
            theme,
            self.exit.is_none(),
            look.keyboard,
            0.0,
            window,
        )
    }

    fn footer(
        &self,
        view: Option<&PartyView>,
        look: Look,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = look.theme;
        let open = self.exit.is_none();
        let readiness = view.map_or_else(Readiness::default, |view| Readiness::of(&view.roster));
        let status = div()
            .min_w_0()
            .flex()
            .flex_wrap()
            .gap_x(px(4.0))
            .text_color(theme.ink_muted)
            .id("party-lobby-status")
            .role(gpui::Role::Status);
        let (status, actions) = match &self.side {
            Side::Host(_) => {
                let start_anyway = readiness.offers_start_anyway().then(|| {
                    quiet(
                        "party-lobby-start-anyway",
                        "Start anyway",
                        None,
                        &self.start_anyway,
                        look,
                        open,
                        window,
                    )
                    .on_click(cx.listener(|lobby, _, window, cx| {
                        lobby.act(Command::StartAnyway, window, cx)
                    }))
                });
                let start = Button::primary("party-lobby-start", "Start watching")
                    .leading(Icon::Play)
                    .disabled(!readiness.can_start())
                    .reduced_motion(look.reduced)
                    .focus(&self.start, look.keyboard)
                    .tab_stop(open)
                    .on_click(cx.listener(|lobby, _, window, cx| {
                        lobby.act(Command::StartWatching, window, cx)
                    }));
                (
                    host_status(status, view, &readiness, theme),
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .children(start_anyway)
                        .child(start),
                )
            }
            Side::Member => {
                let blocked = self.state.read(cx).party_blocked();
                let fix = blocked.and_then(|blocked| {
                    let label = match blocked {
                        Blocked::SignedOut => "Sign in",
                        Blocked::FilmNotFound | Blocked::SourceFailed => "Try again",
                        Blocked::NoAddons => return None,
                    };
                    Some(
                        Button::primary("party-lobby-fix", label)
                            .reduced_motion(look.reduced)
                            .focus(&self.start, look.keyboard)
                            .tab_stop(open)
                            .on_click(cx.listener(move |lobby, _, window, cx| {
                                lobby.fix(blocked, window, cx)
                            })),
                    )
                });
                let rejoin = (self.state.read(cx).party.as_ref())
                    .and_then(|party| party.rejoin_invite().map(str::to_owned))
                    .map(|invite| {
                        Button::primary("party-lobby-rejoin", "Rejoin")
                            .reduced_motion(look.reduced)
                            .focus(&self.start, look.keyboard)
                            .tab_stop(open)
                            .on_click(cx.listener(move |lobby, _, window, cx| {
                                let shell = lobby.shell.clone();
                                let invite = invite.clone();
                                let _ = shell.update(cx, |shell, cx| {
                                    shell.join_party_now(&invite, None, window, cx)
                                });
                            }))
                    });
                let line: SharedString = match blocked {
                    Some(blocked) => blocked.text().into(),
                    None => member_status(
                        view,
                        self.state
                            .read(cx)
                            .party
                            .as_ref()
                            .is_some_and(|party| party.media().is_some()),
                    ),
                };
                (
                    status
                        .when(blocked.is_some(), |status| status.text_color(theme.ink))
                        .child(line),
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            Button::glass("party-lobby-leave", "Leave party")
                                .focus(&self.leave, look.keyboard)
                                .tab_stop(open)
                                .on_click(cx.listener(|lobby, _, window, cx| {
                                    lobby.act(Command::LeaveParty, window, cx)
                                })),
                        )
                        .children(fix)
                        .children(rejoin),
                )
            }
        };
        LABEL
            .apply(div())
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.0))
            .px(px(GUTTER))
            .pt(px(18.0))
            .pb(px(22.0))
            .border_t_1()
            .border_color(theme.rule)
            .child(status)
            .child(actions)
    }
}

fn section(title: &'static str, aside: Option<String>, theme: Theme, first: bool) -> Div {
    div()
        .py(px(18.0))
        .when(!first, |section| {
            section.border_t_1().border_color(theme.rule)
        })
        .flex()
        .flex_col()
        .child(
            div()
                .mb(px(10.0))
                .flex()
                .items_baseline()
                .justify_between()
                .child(LABEL.apply(div()).line_height(relative(1.0)).child(title))
                .when_some(aside, |head, aside| {
                    head.child(
                        CAPTION
                            .apply(div())
                            .text_color(theme.ink_muted)
                            .child(aside),
                    )
                }),
        )
}

fn person(row: &RosterRow, view: &PartyView, look: Look) -> Div {
    let theme = look.theme;
    let member = &row.member;
    let role = if member.is_host {
        Some("Host")
    } else if view.me == Some(member.member_id) {
        Some("You")
    } else {
        None
    };
    let initial: String = member
        .name
        .chars()
        .take(1)
        .flat_map(char::to_uppercase)
        .collect();
    let (label, ready) = match member.state {
        state if is_ready(state) => ("Ready", true),
        MemberState::Failed => ("Can't play this source", false),
        _ => ("Loading the source", false),
    };
    let marker = if ready {
        div()
            .size(px(7.0))
            .rounded_full()
            .bg(theme.accent)
            .into_any_element()
    } else if member.state == MemberState::Failed {
        div().into_any_element()
    } else {
        spinner(member.member_id, look)
    };
    div()
        .h(px(ROW))
        .flex()
        .items_center()
        .gap(px(12.0))
        .child(
            div()
                .size(px(30.0))
                .flex_shrink_0()
                .rounded_full()
                .bg(theme.surface_strong)
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(12.0))
                .child(initial),
        )
        .child(
            LABEL
                .apply(div())
                .flex_1()
                .min_w_0()
                .flex()
                .items_baseline()
                .gap(px(6.0))
                .child(div().truncate().child(member.name.clone()))
                .when_some(role, |name, role| {
                    name.child(SMALL.apply(div()).text_color(theme.ink_muted).child(role))
                }),
        )
        .child(
            LABEL
                .apply(div())
                .flex()
                .items_center()
                .gap(px(7.0))
                .text_color(if ready {
                    theme.ink
                } else if member.state == MemberState::Failed {
                    theme.danger
                } else {
                    theme.ink_muted
                })
                .child(marker)
                .child(label),
        )
}

fn spinner(id: u32, look: Look) -> gpui::AnyElement {
    let icon = svg()
        .path("loader-2.svg")
        .size(px(14.0))
        .text_color(look.theme.ink_muted);
    if look.reduced {
        return icon.into_any_element();
    }
    icon.with_animation(
        ("party-lobby-spin", id as usize),
        Animation::new(Duration::from_secs(1)).repeat(),
        |icon, delta| {
            icon.with_transformation(Transformation::rotate(gpui::radians(
                delta * std::f32::consts::TAU,
            )))
        },
    )
    .into_any_element()
}

/// A text-only pill for the lesser action beside a main one.
fn quiet(
    id: &'static str,
    label: &'static str,
    trailing: Option<&'static str>,
    handle: &FocusHandle,
    look: Look,
    active: bool,
    window: &Window,
) -> Stateful<Div> {
    let theme = look.theme;
    let button = BUTTON
        .apply(div())
        .id(id)
        .role(gpui::Role::Button)
        .aria_label(label)
        .h(px(40.0))
        .px(px(10.0))
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(6.0))
        .rounded_full()
        .text_color(theme.ink_muted)
        .cursor_pointer()
        .hover(move |style| style.text_color(theme.ink).bg(theme.hover))
        .child(title_case(label))
        .when_some(trailing, |button, path| {
            button.child(svg().path(path).size(px(16.0)).text_color(theme.ink_muted))
        });
    focus_ring(button, handle, theme, active, look.keyboard, 20.0, window)
}

fn host_status(
    status: Stateful<Div>,
    view: Option<&PartyView>,
    readiness: &Readiness,
    theme: Theme,
) -> Stateful<Div> {
    let name = |text: String| div().text_color(theme.ink).child(text);
    match view.map(|view| view.link) {
        Some(Link::Starting) => return status.child("Getting the party ready"),
        Some(Link::Ended(reason)) => {
            return status.child(crate::party::lobby_ended_text(reason, true));
        }
        _ => {}
    }
    if let [one] = readiness.failed.as_slice()
        && readiness.waiting.len() == 1
    {
        return status
            .child(name(one.clone()))
            .child("can't play this source");
    }
    match readiness.waiting.as_slice() {
        [] if readiness.friends == 0 => status.child("Nobody has joined yet"),
        [] => status.child("Everyone is ready"),
        [one] => status.child("Waiting for").child(name(one.clone())),
        [one, two] => status
            .child("Waiting for")
            .child(name(one.clone()))
            .child("and")
            .child(name(two.clone())),
        many => status
            .child("Waiting for")
            .child(name(format!("{} friends", many.len()))),
    }
}

pub(super) fn member_status(view: Option<&PartyView>, has_film: bool) -> SharedString {
    let Some(view) = view else {
        return "You left the party".into();
    };
    let host = view
        .roster
        .iter()
        .find(|row| row.member.is_host)
        .map(|row| row.member.name.clone());
    match (view.link, host) {
        (Link::Ended(reason), _) => crate::party::lobby_ended_text(reason, false).into(),
        (Link::Live, Some(host)) if !has_film => format!("{host} is choosing a film").into(),
        (Link::Live, Some(host)) if view.gate.is_some_and(|gate| gate != Gate::Lobby) => {
            format!("{host} is loading the film").into()
        }
        (Link::Live, Some(host)) => format!("{host} starts the film when everyone is ready").into(),
        _ => "Joining the party".into(),
    }
}
