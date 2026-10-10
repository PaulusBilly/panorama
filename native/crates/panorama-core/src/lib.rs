pub mod media;
pub mod store;

pub const APP_NAME: &str = "Panorama";

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::{APP_NAME, version};

    #[test]
    fn app_name_is_panorama() {
        assert_eq!(APP_NAME, "Panorama");
    }

    #[test]
    fn version_is_package_version() {
        assert_eq!(version(), "0.0.0");
    }
}
