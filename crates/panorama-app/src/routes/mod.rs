//! Independent retained route entities.
pub mod festival;
pub mod film;
pub mod home;
pub mod player;
pub mod search;

use crate::{app::AppShell, app_state::AppState, args::Args, router::Route};
use gpui::{AnyView, AppContext, Context, Entity, WeakEntity, Window};

/// Construct an independent entity for one history entry.
pub fn create(
    route: &Route,
    id: u64,
    shell: WeakEntity<AppShell>,
    state: Entity<AppState>,
    args: Args,
    window: &mut Window,
    cx: &mut Context<AppShell>,
) -> AnyView {
    match route {
        Route::Home => cx
            .new(|cx| home::Home::new(shell, state, args, id, cx))
            .into(),
        Route::Search { query } => cx
            .new(|cx| search::Search::new(shell, state, args, id, query.clone(), window, cx))
            .into(),
        Route::Film { id: film_id, kind } => cx
            .new(|cx| film::Film::new(film_id.clone(), *kind, id, shell, state, args, cx))
            .into(),
        Route::Festival { slug, year } => cx
            .new(|cx| festival::Festival::new(shell, state, args, id, slug.clone(), *year, cx))
            .into(),
        Route::Player { id: film_id } => cx
            .new(|cx| player::Player::new(film_id.clone(), id, shell, state, args, cx))
            .into(),
        Route::Addons => cx
            .new(|cx| crate::addons_screen::Addons::new(shell, state, args, id, window, cx))
            .into(),
    }
}
