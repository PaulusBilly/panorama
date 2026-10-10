//! Independent retained route entities.
pub mod addons;
pub mod film;
pub mod home;
pub mod player;
pub mod search;

use crate::{app::AppShell, router::Route};
use gpui::{AnyView, AppContext, Context, WeakEntity};

/// Construct an independent entity for one history entry.
pub fn create(
    route: &Route,
    id: u64,
    shell: WeakEntity<AppShell>,
    cx: &mut Context<AppShell>,
) -> AnyView {
    match route {
        Route::Home => cx.new(|cx| home::Home::new(shell, id, cx)).into(),
        Route::Search { query } => cx.new(|_| search::Search::new(query.clone())).into(),
        Route::Film { id } => cx.new(|_| film::Film::new(id.clone())).into(),
        Route::Player { id } => cx.new(|_| player::Player::new(id.clone())).into(),
        Route::Addons => cx.new(|_| addons::Addons::new()).into(),
    }
}
