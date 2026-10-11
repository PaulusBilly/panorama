//! Independent retained route entities.
pub mod addons;
pub mod film;
pub mod home;
pub mod player;
pub mod search;

use crate::{app::AppShell, app_state::AppState, args::Args, router::Route};
use gpui::{AnyView, AppContext, Context, Entity, WeakEntity};

/// Construct an independent entity for one history entry.
pub fn create(
    route: &Route,
    id: u64,
    shell: WeakEntity<AppShell>,
    state: Entity<AppState>,
    args: Args,
    cx: &mut Context<AppShell>,
) -> AnyView {
    match route {
        Route::Home => cx
            .new(|cx| home::Home::new(shell, state, args, id, cx))
            .into(),
        Route::Search { query } => cx
            .new(|cx| home::Home::new_search(shell, state, args, id, query.clone(), cx))
            .into(),
        Route::Film { id: film_id } => cx
            .new(|cx| film::Film::new(film_id.clone(), id, shell, state, args, cx))
            .into(),
        Route::Player { id } => {
            let source = state
                .read(cx)
                .playback
                .as_ref()
                .filter(|selection| selection.film_id == *id)
                .map(|selection| selection.source.clone());
            cx.new(|_| player::Player::new(id.clone(), source)).into()
        }
        Route::Addons => cx.new(|_| addons::Addons::new()).into(),
    }
}
