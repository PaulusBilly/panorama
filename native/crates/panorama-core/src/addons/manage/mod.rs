//! Validated install URLs, redacted previews and single-use confirmation tokens.

mod preview;
mod types;
mod url;

pub(crate) use preview::{parse_manifest, replacement};
pub use types::{AddonPreview, InstallError, PreviewToken};
pub use url::AddonUrl;
pub(crate) use url::forbidden_host;

#[cfg(test)]
mod tests;
