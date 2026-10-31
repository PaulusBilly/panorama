//! Panorama's application shell, theme and navigation.
pub mod a11y;
pub mod account_menu;
pub mod addons_screen;
pub mod alert_row;
pub mod app;
pub mod app_state;
pub mod args;
pub mod assets;
#[allow(unsafe_code)]
pub mod awake;
pub mod award_marks;
pub mod button;
pub mod carousel;
pub mod cascade_loader;
pub mod catalog_switcher;
pub mod continue_row;
pub mod continue_watching;
pub mod debug;
pub mod film_card;
pub mod film_display;
pub mod film_grid;
pub mod fixtures;
pub mod hero;
pub mod home_layout;
pub mod image_cache;
pub mod login;
pub mod motion;
pub mod playback;
pub mod router;
pub mod routes;
pub mod services;
pub mod sidebar;
pub mod theme;
pub mod titlebar;
pub mod transition;
#[allow(unsafe_code)]
pub mod window_controls;

pub mod startup_error;
