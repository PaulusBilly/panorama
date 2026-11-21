pub mod addons;
pub mod credits;
pub mod discord;
pub mod images;
pub mod media;
pub mod mubi;
pub mod party;
pub mod store;
pub mod stremio;
pub mod tmdb;
pub mod torrent;
pub mod trailer;

#[cfg(test)]
mod test_tracing;

pub const APP_NAME: &str = "Panorama";

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::APP_NAME;

    #[test]
    fn app_name_is_panorama() {
        assert_eq!(APP_NAME, "Panorama");
    }
}
