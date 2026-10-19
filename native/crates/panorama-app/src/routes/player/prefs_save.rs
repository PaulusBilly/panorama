//! Discord sharing and the debounced, ordered preference writes of the player.
use super::Player;
use crate::playback::{
    output,
    prefs::{self, DiscordPort, DiscordView},
    subtitle_prefs,
};
use gpui::Context;
use std::sync::Arc;

impl Player {
    /// Read the Discord opt-in when the settings panel opens.
    pub(super) fn refresh_discord(&mut self, cx: &mut Context<Self>) {
        let Some(services) = self.services(cx) else {
            return;
        };
        let runtime = services.runtime.clone();
        let port: Arc<dyn DiscordPort> = services;
        cx.spawn(async move |entity, cx| {
            let settings = prefs::read_discord(&runtime, port).await.ok().flatten();
            let _ = entity.update(cx, |player, cx| {
                // A refresh that raced with a toggle must not overwrite its pending state.
                let busy = player.ui.discord.as_ref().is_some_and(|view| view.busy);
                if let Some(settings) = settings
                    && !busy
                {
                    player.ui.discord = Some(DiscordView::from_settings(settings));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Turn Discord sharing on or off on a blocking worker.
    pub(super) fn set_discord(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let Some(view) = &mut self.ui.discord else {
            return;
        };
        if !view.begin_toggle() {
            return;
        }
        cx.notify();
        let Some(services) = self.services(cx) else {
            return;
        };
        let runtime = services.runtime.clone();
        let port: Arc<dyn DiscordPort> = services;
        cx.spawn(async move |entity, cx| {
            let result = prefs::toggle_discord(&runtime, port, enabled)
                .await
                .unwrap_or_else(|_| Err(std::io::Error::other("worker stopped")));
            let _ = entity.update(cx, |player, cx| {
                player.ui.discord = player.ui.discord.as_ref().map(|view| view.finished(result));
                cx.notify();
            });
        })
        .detach();
    }

    /// Debounce a write of preference group `which` (0 subtitles, 1 settings).
    pub(super) fn schedule_save(&mut self, which: usize, cx: &mut Context<Self>) {
        // Capture overrides are never persisted, and fixture mode has no store.
        let has_store = self
            .services(cx)
            .is_some_and(|services| services.prefs_writer.is_some());
        if self.ui.ephemeral || !has_store {
            return;
        }
        self.ui.dirty[which] = true;
        self.ui.saves[which] = Some(cx.spawn(async move |entity, cx| {
            cx.background_executor().timer(prefs::SAVE_DELAY).await;
            let _ = entity.update(cx, |player, cx| player.write_prefs(which, cx));
        }));
    }

    /// Hand one group to the ordered writer now.
    fn write_prefs(&mut self, which: usize, cx: &gpui::App) {
        if !std::mem::take(&mut self.ui.dirty[which]) {
            return;
        }
        let Some(writer) = self
            .services(cx)
            .and_then(|services| services.prefs_writer.clone())
        else {
            return;
        };
        let (name, bytes) = if which == 0 {
            (subtitle_prefs::KEY, self.ui.prefs.to_bytes())
        } else {
            (output::KEY, self.ui.output.to_bytes())
        };
        writer.save(name, bytes);
    }

    /// Write whatever the debounce has not reached yet (the player is closing).
    pub(super) fn flush_prefs(&mut self, cx: &gpui::App) {
        for which in 0..2 {
            self.ui.saves[which] = None;
            self.write_prefs(which, cx);
        }
    }
}
