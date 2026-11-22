//! The watch party in the player: its panel page, and a toast for what others do.
use super::{
    Player,
    panel_widgets::{note, setting_row, slot},
    time::format_time,
};
use crate::{
    party::{EndReason, Link, PartySync, PartyView, Role, RosterRow},
    theme::{Theme, Typography, focus_ring},
};
use gpui::{AnyElement, ClipboardItem, Div, SharedString, Stateful, Window, div, prelude::*, px};
use panorama_core::party::{
    ByeReason, Gate, HostSettings, MemberState, Notice, NoticeKind, transport::PathKind,
};
use std::time::{Duration, Instant};

/// The party page's scroll handle, after the Subtitles tab page.
const PARTY_SCROLL: usize = 5;
const TOAST_FOR: Duration = Duration::from_secs(3);
const TOAST_FADE: Duration = Duration::from_millis(200);

/// What the player remembers about the party between frames.
#[derive(Default)]
pub(super) struct PartyUi {
    /// The newest notice already shown, so a notice toasts once.
    seen: Option<Instant>,
    /// Whether notices before this player opened have been skipped.
    primed: bool,
    /// The party was started here, and its invite goes to the clipboard once it is ready.
    /// A party started from the lobby was copied there, so a film opening later leaves the
    /// clipboard alone.
    copy_pending: bool,
    /// The host's source switches already seen, so each toasts once.
    switches: Option<u32>,
    ended: bool,
    toast: Option<(SharedString, Instant)>,
}

#[derive(Clone, Copy, PartialEq)]
enum Tone {
    Plain,
    Accent,
    Danger,
}

impl Player {
    /// Turns new party notices into the toast, and copies a host's invite once it exists.
    pub(super) fn sync_party(&mut self, cx: &mut gpui::Context<Self>) {
        let state = self.state.read(cx);
        let Some(party) = &state.party else {
            self.party_ui = PartyUi::default();
            return;
        };
        let view = &party.view;
        let mut toast = None;
        if let Some(entry) = view.notices.last()
            && self.party_ui.seen.is_none_or(|seen| entry.at > seen)
        {
            self.party_ui.seen = Some(entry.at);
            if self.party_ui.primed {
                toast = Some(notice_text(&entry.notice));
            }
        }
        self.party_ui.primed = true;
        if self.party_ui.copy_pending
            && let Role::Host {
                invite: Some(link), ..
            } = &view.role
        {
            let link = link.as_str().to_owned();
            self.party_ui.copy_pending = false;
            if self.args.party_preview.is_none() {
                cx.write_to_clipboard(ClipboardItem::new_string(link));
            }
            toast = Some("Invite link copied. Send it to your friends.".into());
        }
        if self
            .party_ui
            .switches
            .is_some_and(|seen| party.switches > seen)
        {
            let host = view
                .roster
                .iter()
                .find(|row| row.member.is_host)
                .map_or("The host", |row| row.member.name.as_str());
            toast = Some(format!("{host} switched the source"));
        }
        self.party_ui.switches = Some(party.switches);
        if !self.party_ui.ended && view.ended().is_some() {
            self.party_ui.ended = true;
            toast = Some("Party ended".into());
        }
        if let Some(text) = toast {
            self.party_ui.toast = Some((text.into(), Instant::now()));
        }
    }

    /// A message of the player's own about the party, such as a control the host keeps.
    pub(super) fn party_toast_now(&mut self, text: &'static str) {
        self.party_ui.toast = Some((text.into(), Instant::now()));
    }

    pub(super) fn party_toast_moving(&self, now: Instant) -> bool {
        self.party_ui
            .toast
            .as_ref()
            .is_some_and(|(_, at)| now.duration_since(*at) < TOAST_FOR)
    }

    /// The toast at the top centre: it fades in, holds, then fades out. It never takes clicks.
    pub(super) fn party_toast(&self, theme: Theme, reduced: bool, now: Instant) -> Option<Div> {
        let (text, at) = self.party_ui.toast.as_ref()?;
        let elapsed = now.duration_since(*at);
        if elapsed >= TOAST_FOR {
            return None;
        }
        let opacity = if reduced {
            1.0
        } else {
            let enter = (elapsed.as_secs_f32() / TOAST_FADE.as_secs_f32()).min(1.0);
            let exit = ((TOAST_FOR - elapsed).as_secs_f32() / TOAST_FADE.as_secs_f32()).min(1.0);
            enter.min(exit)
        };
        let rise = if reduced { 0.0 } else { 6.0 * (1.0 - opacity) };
        Some(
            div()
                .absolute()
                .top(px(84.0 - rise))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .opacity(opacity)
                .child(
                    div()
                        .id("party-toast")
                        .role(gpui::Role::Status)
                        .aria_label(text.clone())
                        .max_w(px(420.0))
                        .px(px(16.0))
                        .py(px(9.0))
                        .rounded_full()
                        .bg(theme.player_canvas.opacity(0.88))
                        .border_1()
                        .border_color(theme.ink.opacity(0.12))
                        .label()
                        .text_color(theme.ink)
                        .truncate()
                        .child(text.clone()),
                ),
        )
    }

    /// The body of the Watch party page: a way in when there is no party, else who is
    /// watching and how, the host's switches, the invite and the way out.
    pub(super) fn party_body(
        &self,
        theme: Theme,
        keyboard: bool,
        window: &Window,
        cx: &mut gpui::Context<Self>,
    ) -> AnyElement {
        let section = |label: &'static str| {
            note(label, theme, 0.55)
                .px(px(20.0))
                .pt(px(12.0))
                .pb(px(4.0))
        };
        let helper = |text: SharedString| note(text, theme, 0.55).px(px(20.0)).pb(px(8.0));
        let body = div()
            .id("player-party-body")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.scrolls[PARTY_SCROLL])
            .flex()
            .flex_col()
            .pb(px(8.0));
        let view = self
            .state
            .read(cx)
            .party
            .as_ref()
            .map(|party| party.view.clone());
        let Some(view) = view else {
            return body
                .child(
                    helper(
                        "Watch this in sync with friends. Everyone streams their own copy from \
                         their own addons."
                            .into(),
                    )
                    .pt(px(4.0)),
                )
                .child(self.party_row(
                    "party-start",
                    slot::PARTY_PRIMARY,
                    "Start a party",
                    Tone::Accent,
                    false,
                    theme,
                    keyboard,
                    window,
                    |player, cx| {
                        player.party_ui.copy_pending = true;
                        player
                            .state
                            .update(cx, |state, cx| state.start_party(None, cx))
                    },
                    cx,
                ))
                .into_any_element();
        };
        let host = view.is_host();
        if let Some(reason) = view.ended() {
            return body
                .child(helper(crate::party::ended_text(reason, host).into()).pt(px(4.0)))
                .child(self.party_row(
                    "party-close",
                    slot::PARTY_EXIT,
                    "Close",
                    Tone::Plain,
                    false,
                    theme,
                    keyboard,
                    window,
                    |player, cx| player.state.update(cx, |state, cx| state.leave_party(cx)),
                    cx,
                ))
                .into_any_element();
        }
        let mut body = body.child(helper(status_line(&view).into()).pt(px(4.0)));
        if let Role::Host { invite, .. } = &view.role {
            body = body.child(self.party_row(
                "party-copy",
                slot::PARTY_PRIMARY,
                if invite.is_some() {
                    "Copy invite link"
                } else {
                    "Getting the invite link…"
                },
                Tone::Accent,
                invite.is_none(),
                theme,
                keyboard,
                window,
                |player, cx| player.copy_invite(cx),
                cx,
            ));
        }
        if !view.roster.is_empty() {
            body = body
                .child(section("People"))
                .children(view.roster.iter().map(|row| roster_row(row, theme)));
        }
        if let Role::Host { settings, .. } = &view.role {
            let settings = *settings;
            body = body
                .child(section("Host controls"))
                .child(self.checkbox(
                    "party-wait",
                    slot::PARTY_WAIT,
                    "Wait for everyone",
                    settings.wait_for_everyone,
                    false,
                    theme,
                    keyboard,
                    window,
                    move |player, cx| {
                        player.set_party_settings(
                            HostSettings {
                                wait_for_everyone: !settings.wait_for_everyone,
                                ..settings
                            },
                            cx,
                        )
                    },
                    cx,
                ))
                .child(helper(
                    "Pauses for anyone buffering, and after a jump until everyone is there. Up to 30 seconds at a time.".into(),
                ))
                .child(self.checkbox(
                    "party-control",
                    slot::PARTY_CONTROL,
                    "Let everyone control",
                    settings.anyone_controls,
                    false,
                    theme,
                    keyboard,
                    window,
                    move |player, cx| {
                        player.set_party_settings(
                            HostSettings {
                                anyone_controls: !settings.anyone_controls,
                                ..settings
                            },
                            cx,
                        )
                    },
                    cx,
                ))
                .child(helper(
                    "Anyone can play, pause or seek. Otherwise only you can.".into(),
                ));
        } else {
            let sync = view.sync;
            body = body
                .child(section("Follow the host"))
                .child(self.checkbox(
                    "party-follow-subtitles",
                    slot::PARTY_FOLLOW_SUBTITLES,
                    "Subtitles",
                    sync.subtitles,
                    false,
                    theme,
                    keyboard,
                    window,
                    move |player, cx| {
                        player.set_party_sync(
                            PartySync {
                                subtitles: !sync.subtitles,
                                ..sync
                            },
                            cx,
                        )
                    },
                    cx,
                ))
                .child(self.checkbox(
                    "party-follow-audio",
                    slot::PARTY_FOLLOW_AUDIO,
                    "Audio",
                    sync.audio,
                    false,
                    theme,
                    keyboard,
                    window,
                    move |player, cx| {
                        player.set_party_sync(
                            PartySync {
                                audio: !sync.audio,
                                ..sync
                            },
                            cx,
                        )
                    },
                    cx,
                ))
                .child(helper(
                    "Uses the host's subtitles and audio language. Turn off to choose your own."
                        .into(),
                ));
        }
        body.child(self.party_row(
            "party-leave",
            slot::PARTY_EXIT,
            if host { "End party" } else { "Leave party" },
            Tone::Danger,
            false,
            theme,
            keyboard,
            window,
            |player, cx| player.state.update(cx, |state, cx| state.leave_party(cx)),
            cx,
        ))
        .into_any_element()
    }

    fn set_party_settings(&mut self, settings: HostSettings, cx: &mut gpui::Context<Self>) {
        self.state
            .update(cx, |state, cx| state.set_party_settings(settings, cx));
    }

    fn set_party_sync(&mut self, sync: PartySync, cx: &mut gpui::Context<Self>) {
        self.state
            .update(cx, |state, cx| state.set_party_sync(sync, cx));
    }

    /// Shows the Watch party page, as the sidebar's Watch party asks.
    pub fn open_party_panel(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) {
        self.show_panel(super::panel::PanelId::Party, window, cx);
    }

    /// "Waiting for <name>" while the party holds everyone for someone else.
    pub(super) fn party_waiting(&self, cx: &gpui::App) -> Option<String> {
        waiting_text(&self.state.read(cx).party.as_ref()?.view)
    }

    fn copy_invite(&mut self, cx: &mut gpui::Context<Self>) {
        let link = self
            .state
            .read(cx)
            .party
            .as_ref()
            .and_then(|party| match &party.view.role {
                Role::Host {
                    invite: Some(link), ..
                } => Some(link.as_str().to_owned()),
                _ => None,
            });
        if let Some(link) = link {
            cx.write_to_clipboard(ClipboardItem::new_string(link));
            self.party_toast_now("Invite link copied");
            cx.notify();
        }
    }

    /// A row that acts when pressed: start, copy, leave.
    #[allow(clippy::too_many_arguments)]
    fn party_row(
        &self,
        id: &'static str,
        slot: usize,
        label: &'static str,
        tone: Tone,
        disabled: bool,
        theme: Theme,
        keyboard: bool,
        window: &Window,
        on_press: impl Fn(&mut Player, &mut gpui::Context<Player>) + 'static,
        cx: &mut gpui::Context<Self>,
    ) -> Stateful<Div> {
        let ink = theme.ink;
        let color = match tone {
            Tone::Plain => theme.ink,
            Tone::Accent => theme.accent_text,
            Tone::Danger => theme.danger,
        };
        let row = setting_row(id, theme)
            .role(gpui::Role::Button)
            .aria_label(label)
            .text_color(color)
            .when(disabled, |row| row.opacity(0.5))
            .when(!disabled, |row| {
                row.cursor_pointer()
                    .hover(move |style| style.bg(ink.opacity(0.05)))
                    .on_click(cx.listener(move |player, _, _, cx| {
                        on_press(player, cx);
                        cx.notify();
                    }))
            })
            .child(label);
        focus_ring(
            row,
            &self.focus.pool[slot],
            super::widgets::focus_theme(theme),
            !disabled,
            keyboard,
            0.0,
            window,
        )
    }
}

fn roster_row(row: &RosterRow, theme: Theme) -> Div {
    let member = &row.member;
    let (state, attention) = member_label(row);
    div()
        .flex()
        .items_center()
        .gap(px(12.0))
        .min_h(px(44.0))
        .px(px(20.0))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .flex()
                .flex_col()
                .child(div().label().truncate().child(member.name.clone()))
                .when(member.is_host, |name| name.child(note("Host", theme, 0.55))),
        )
        .child(
            div()
                .flex_shrink_0()
                .caption()
                .text_color(if attention {
                    theme.danger
                } else {
                    theme.ink.opacity(0.62)
                })
                .child(state),
        )
}

/// How a person's row reads, and whether it needs attention.
fn member_label(row: &RosterRow) -> (&'static str, bool) {
    let member = &row.member;
    if !member.release_matched {
        return ("Different release", true);
    }
    match member.state {
        MemberState::Failed => ("Can't play", true),
        MemberState::Buffering => ("Buffering", false),
        MemberState::Loading if member.late => ("Catching up", false),
        MemberState::Loading => ("Loading", false),
        MemberState::Ready => ("Ready", false),
        MemberState::Paused => ("Paused", false),
        MemberState::Playing => ("Watching", false),
    }
}

/// The line under the title: what the connection is doing, or how many are watching.
fn status_line(view: &PartyView) -> String {
    match view.link {
        Link::Starting if view.is_host() => "Getting an invite link ready…".into(),
        Link::Starting | Link::Connecting => "Connecting to the host…".into(),
        Link::Ended(_) => "Party ended".into(),
        Link::Live => {
            let count = view.roster.len().max(1);
            let people = if count == 1 {
                "Just you so far".to_owned()
            } else {
                format!("{count} watching")
            };
            let relayed = view
                .roster
                .iter()
                .any(|row| row.path == Some(PathKind::Relayed));
            let direct = view
                .roster
                .iter()
                .any(|row| row.path == Some(PathKind::Direct));
            match (relayed, direct) {
                (true, _) => format!("{people} · Through a relay"),
                (false, true) => format!("{people} · Direct connection"),
                _ => people,
            }
        }
    }
}

/// Who the party is paused for, by the name the roster gives them.
fn waiting_text(view: &PartyView) -> Option<String> {
    let Some(Gate::Waiting(id)) = view.gate else {
        return None;
    };
    if view.ended().is_some() {
        return None;
    }
    let name = view
        .roster
        .iter()
        .find(|row| row.member.member_id == id)
        .map_or("someone", |row| row.member.name.as_str());
    Some(format!("Waiting for {name}"))
}

fn notice_text(notice: &Notice) -> String {
    let by = &notice.by;
    let at = notice.at.map(format_time);
    match (notice.kind, at) {
        (NoticeKind::Paused, Some(at)) => format!("{by} paused at {at}"),
        (NoticeKind::Paused, None) => format!("{by} paused"),
        (NoticeKind::Played, _) => format!("{by} pressed play"),
        (NoticeKind::Seeked, Some(at)) => format!("{by} jumped to {at}"),
        (NoticeKind::Seeked, None) => format!("{by} jumped"),
        (NoticeKind::Joined, _) => format!("{by} joined"),
        (NoticeKind::Left, _) => format!("{by} left"),
        (NoticeKind::WaitingFor, _) => format!("Waiting for {by}"),
        (NoticeKind::DifferentRelease, _) => format!("{by} is on a different release"),
        (NoticeKind::DifferentLength, _) => format!("{by}'s copy is a different length"),
        (NoticeKind::Denied, _) => "Only the host can play, pause or seek".into(),
    }
}

/// A made-up party for `--party-preview` captures: four people, one buffering and one on
/// another release.
pub(super) fn preview(kind: &str) -> PartyView {
    use crate::party::{InviteLink, NoticeEntry};
    use panorama_core::party::MemberView;
    let person = |member_id, name: &str, state, release_matched, is_host, path| RosterRow {
        member: MemberView {
            member_id,
            name: name.into(),
            state,
            late: false,
            release_matched,
            is_host,
        },
        path,
    };
    let host = kind == "host";
    PartyView {
        role: if host {
            Role::Host {
                invite: Some(InviteLink::preview()),
                settings: HostSettings::default(),
            }
        } else {
            Role::Member
        },
        link: if kind == "ended" {
            Link::Ended(EndReason::Bye(ByeReason::HostLeft))
        } else {
            Link::Live
        },
        roster: vec![
            person(
                0,
                "Billy",
                MemberState::Playing,
                true,
                true,
                Some(PathKind::Direct),
            ),
            person(
                1,
                "Alex",
                MemberState::Playing,
                true,
                false,
                Some(PathKind::Direct),
            ),
            person(
                2,
                "Sam",
                MemberState::Buffering,
                true,
                false,
                Some(PathKind::Relayed),
            ),
            person(
                3,
                "Rina",
                MemberState::Playing,
                false,
                false,
                Some(PathKind::Direct),
            ),
        ],
        anyone_controls: false,
        notices: vec![NoticeEntry {
            notice: Notice {
                kind: NoticeKind::Paused,
                by: "Alex".into(),
                at: Some(724.0),
            },
            at: Instant::now(),
        }],
        sync: PartySync::default(),
        me: None,
        gate: (kind != "ended").then_some(Gate::Waiting(2)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice(kind: NoticeKind, at: Option<f64>) -> Notice {
        Notice {
            kind,
            by: "Sam".into(),
            at,
        }
    }

    #[test]
    fn notices_read_as_sentences() {
        assert_eq!(
            notice_text(&notice(NoticeKind::Paused, Some(724.0))),
            "Sam paused at 12:04"
        );
        assert_eq!(
            notice_text(&notice(NoticeKind::Seeked, Some(2470.0))),
            "Sam jumped to 41:10"
        );
        assert_eq!(
            notice_text(&notice(NoticeKind::WaitingFor, None)),
            "Waiting for Sam"
        );
    }

    #[test]
    fn waiting_pill_names_the_member_the_party_waits_for() {
        let mut view = preview("member");
        view.gate = Some(Gate::Waiting(2));
        assert_eq!(waiting_text(&view).as_deref(), Some("Waiting for Sam"));
        view.gate = Some(Gate::Waiting(9));
        assert_eq!(waiting_text(&view).as_deref(), Some("Waiting for someone"));
        view.gate = Some(Gate::Ready);
        assert_eq!(waiting_text(&view), None);
        view.gate = None;
        assert_eq!(waiting_text(&view), None);
        let mut ended = preview("ended");
        ended.gate = Some(Gate::Waiting(2));
        assert_eq!(waiting_text(&ended), None);
    }

    #[test]
    fn a_different_release_outranks_the_player_state() {
        let row = |state, matched| RosterRow {
            member: panorama_core::party::MemberView {
                member_id: 1,
                name: "Sam".into(),
                state,
                late: false,
                release_matched: matched,
                is_host: false,
            },
            path: None,
        };
        assert_eq!(
            member_label(&row(MemberState::Playing, false)),
            ("Different release", true)
        );
        assert_eq!(
            member_label(&row(MemberState::Playing, true)),
            ("Watching", false)
        );
        assert_eq!(
            member_label(&row(MemberState::Failed, true)),
            ("Can't play", true)
        );
    }
}
