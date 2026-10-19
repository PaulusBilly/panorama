//! Everything the Addons screen does when asked: each action starts one core call
//! on the services runtime and applies its reply on the UI thread.
use super::{
    Addons,
    backend::{Done, Snapshot},
    model::{InstallFlow, InstalledRow, PREVIEW_TTL, RowPending, move_row},
    state,
};
use crate::{theme::motion::*, transition::Tween};
use gpui::{Context, Window};
use panorama_core::addons::{
    AddonKey,
    manage::{AddonUrl, InstallError},
};
use std::time::{Duration, Instant};

/// Distance a swapped row travels: its height, the rule included.
const ROW_STRIDE: f32 = 76.0;

impl Addons {
    /// Run `work` against a oneshot reply and hand its value to `then` on the UI thread.
    fn request<T: 'static>(
        &mut self,
        reply: futures::channel::oneshot::Receiver<T>,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    ) {
        cx.spawn(async move |this, cx| {
            if let Ok(value) = reply.await {
                let _ = this.update(cx, |this, cx| then(this, value, cx));
            }
        })
        .detach();
    }

    pub(super) fn reload(&mut self, cx: &mut Context<Self>) {
        let Some(backend) = &self.backend else {
            return;
        };
        self.loading = true;
        self.load_failed = false;
        let reply = backend.load();
        self.request(reply, cx, |this, result, cx| {
            this.loading = false;
            match result {
                Ok(snapshot) => this.loaded(snapshot, cx),
                Err(_) => this.load_failed = true,
            }
            cx.notify();
        });
        cx.notify();
    }

    fn loaded(&mut self, snapshot: Snapshot, _: &mut Context<Self>) {
        self.take_snapshot(snapshot);
        if self.seeded {
            return;
        }
        self.seeded = true;
        if self.args.addons_confirm_remove
            && let Some(row) = self.rows.get_mut(1)
        {
            row.pending = RowPending::ConfirmRemove;
        }
        if self.args.addons_preview {
            self.pending_input = Some("https://example.com/manifest.json".into());
            self.flow = InstallFlow::Preview {
                card: super::fixture::preview_card(),
                token: super::backend::Token::Fixture,
                expires: Instant::now() + PREVIEW_TTL,
            };
            self.reveal = Tween::fixed(1.0);
        }
        if self.args.addons_error {
            self.pending_input = Some("example.com".into());
            self.flow = InstallFlow::Failed(InstallError::InvalidUrl.into());
        }
    }

    /// Replace rows and the details choice, keeping per-row state for rows that remain.
    fn take_snapshot(&mut self, snapshot: Snapshot) {
        let now = Instant::now();
        let (rows, added) = state::merge_rows(&self.rows, &self.leaving, snapshot.rows);
        if self.seeded {
            for key in added {
                let mut fade = Tween::fixed(0.0);
                fade.retarget(1.0, DURATION_STANDARD, Some(EASE_EDITORIAL), now);
                self.present.insert(key, fade);
            }
        }
        self.rows = rows;
        self.meta.take(snapshot.choices, snapshot.meta);
    }

    /// The user edited the link. `InputState::set_value` emits no change event, so the
    /// seeds and the post-install clear never land here.
    pub(super) fn input_changed(&mut self, cx: &mut Context<Self>) {
        self.flow.edited();
        cx.notify();
    }

    fn set_input_locked(&self, locked: bool, cx: &mut Context<Self>) {
        self.input
            .update(cx, |input, cx| input.set_readonly(locked, cx));
    }

    pub(super) fn submit(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value();
        let text = text.trim();
        if !self.flow.submit(text.chars().count()) {
            return;
        }
        self.notice = None;
        let parsed = AddonUrl::parse(text);
        let url = match parsed {
            Ok(url) => url,
            Err(error) => {
                self.flow.failed(error.into());
                cx.notify();
                return;
            }
        };
        let Some(backend) = &self.backend else {
            self.flow.failed(InstallError::Core.into());
            cx.notify();
            return;
        };
        self.set_input_locked(true, cx);
        let id = self.previews.next();
        let reply = backend.preview(url);
        self.request(reply, cx, move |this, result, cx| {
            if !this.previews.is_current(id) {
                return;
            }
            this.set_input_locked(false, cx);
            let ok = result.is_ok();
            this.flow.previewed(result, Instant::now());
            if ok {
                this.expire_later(cx);
            }
            cx.notify();
        });
        cx.notify();
    }

    fn expire_later(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(PREVIEW_TTL + Duration::from_millis(50))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.flow.expire(Instant::now());
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn cancel_preview(&mut self, cx: &mut Context<Self>) {
        if self.flow.cancel() {
            self.previews.next();
            self.set_input_locked(false, cx);
            cx.notify();
        }
    }

    pub(super) fn confirm_install(&mut self, cx: &mut Context<Self>) {
        let Some(token) = self.flow.confirm(Instant::now()) else {
            cx.notify();
            return;
        };
        let Some(backend) = &self.backend else {
            self.flow.failed(InstallError::Core.into());
            cx.notify();
            return;
        };
        self.set_input_locked(true, cx);
        let reply = backend.install(token);
        self.request(reply, cx, |this, done, cx| this.installed(done, cx));
        cx.notify();
    }

    fn installed(&mut self, done: Done, cx: &mut Context<Self>) {
        self.set_input_locked(false, cx);
        if let Some(snapshot) = done.snapshot {
            self.take_snapshot(snapshot);
        }
        match done.result {
            Ok(()) => {
                if let Some(name) = self.flow.installed() {
                    self.set_notice(format!("Installed {name}."), cx);
                }
                self.pending_input = Some(String::new());
            }
            Err(error) => self.flow.failed(error),
        }
        cx.notify();
    }

    fn set_notice(&mut self, text: String, cx: &mut Context<Self>) {
        self.notice = Some(text);
        self.notice_generation += 1;
        let generation = self.notice_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(4)).await;
            let _ = this.update(cx, |this, cx| {
                if this.notice_generation == generation {
                    this.notice = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn row_mut(&mut self, key: &AddonKey) -> Option<&mut InstalledRow> {
        self.rows.iter_mut().find(|row| &row.key == key)
    }

    pub(super) fn busy_rows(&self) -> bool {
        self.rows
            .iter()
            .any(|row| matches!(row.pending, RowPending::Moving | RowPending::Removing))
    }

    /// Reorder `self.rows` and start the vertical slide of every row that moved.
    fn apply_order(&mut self, order: &[AddonKey]) {
        let now = Instant::now();
        for (key, places) in state::reorder(&mut self.rows, order) {
            let mut slide = Tween::fixed(places as f32 * ROW_STRIDE);
            slide.retarget(0.0, DURATION_STANDARD, Some(EASE_EDITORIAL), now);
            self.slide.insert(key, slide);
        }
    }

    pub(super) fn move_by(&mut self, key: AddonKey, delta: isize, cx: &mut Context<Self>) {
        if self.busy_rows() {
            return;
        }
        let order = state::visible_order(&self.rows, &self.leaving);
        let Some(next) = move_row(&order, &key, delta) else {
            return;
        };
        let Some(backend) = &self.backend else {
            return;
        };
        let reply = backend.reorder(next.clone());
        self.apply_order(&next);
        if let Some(row) = self.row_mut(&key) {
            row.pending = RowPending::Moving;
        }
        self.request(reply, cx, move |this, done, cx| {
            let failure = done.result.err();
            // The core's order wins; without a readable one, undo the local swap.
            let settled = done.snapshot.as_ref().map_or(order, |snapshot| {
                state::visible_order(&snapshot.rows, &this.leaving)
            });
            this.apply_order(&settled);
            if let Some(row) = this.row_mut(&key) {
                row.pending = failure.map_or(RowPending::None, RowPending::Error);
            }
            cx.notify();
        });
        cx.notify();
    }

    pub(super) fn set_pending(
        &mut self,
        key: &AddonKey,
        pending: RowPending,
        cx: &mut Context<Self>,
    ) {
        if let Some(row) = self.row_mut(key) {
            row.pending = pending;
        }
        cx.notify();
    }

    pub(super) fn remove(&mut self, key: AddonKey, cx: &mut Context<Self>) {
        let Some(backend) = &self.backend else {
            return;
        };
        let reply = backend.remove(key.clone());
        self.set_pending(&key, RowPending::Removing, cx);
        self.request(reply, cx, move |this, done, cx| {
            match done.result {
                Ok(()) => {
                    this.focus_after = state::focus_after_remove(&this.rows, &this.leaving, &key);
                    let mut fade = Tween::fixed(1.0);
                    fade.retarget(0.0, DURATION_STANDARD, Some(EASE_EDITORIAL), Instant::now());
                    this.present.insert(key.clone(), fade);
                    this.leaving.insert(key);
                    if let Some(snapshot) = done.snapshot {
                        this.meta.take(snapshot.choices, snapshot.meta);
                    }
                }
                Err(error) => this.set_pending(&key, RowPending::Error(error), cx),
            }
            cx.notify();
        });
    }

    pub(super) fn configure(&mut self, key: AddonKey, cx: &mut Context<Self>) {
        let Some(backend) = &self.backend else {
            return;
        };
        let reply = backend.configure(key);
        self.request(reply, cx, |_, url, cx| {
            if let Some(url) = url {
                cx.open_url(url.as_str());
            }
        });
    }

    pub(super) fn choose_meta(&mut self, source: Option<AddonKey>, cx: &mut Context<Self>) {
        let Some(backend) = &self.backend else {
            return;
        };
        let Some(id) = self.meta.choose(source.clone()) else {
            return;
        };
        let reply = backend.set_meta(source);
        self.request(reply, cx, move |this, done, cx| {
            let snapshot = done.snapshot.map(|s| (s.choices, s.meta));
            this.meta.reply(id, done.result, snapshot);
            cx.notify();
        });
        cx.notify();
    }
}
