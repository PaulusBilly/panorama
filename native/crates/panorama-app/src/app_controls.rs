use super::*;

impl AppShell {
    /// Open a fresh sign-in form above all route chrome.
    pub fn open_login(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu.set_open(false);
        gpui_component::Theme::update(cx, |theme| theme.colors.caret = self.theme.ink.into());
        if self.login.is_none() {
            let shell = cx.weak_entity();
            let state = self.state.clone();
            self.login = Some(cx.new(|cx| LoginDialog::new(shell, state, window, cx)));
        }
        cx.notify();
    }
    /// Release the closed dialog and restore focus to the account avatar.
    pub fn finish_login(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.login = None;
        self.sidebar.account.focus(window, cx);
        cx.notify();
    }
}
