//! Pure Addons state: install flow, installed rows, ordering, copy and catalog groups.
use panorama_core::addons::{AddonKey, manage::InstallError};
use std::time::{Duration, Instant};
use url::Url;

pub use super::choices::{
    CatalogGroup, MetaChoice, Screen, catalog_groups, meta_selection, radio_step, screen,
};

/// How long a fetched manifest may be confirmed, matching the core's token expiry.
pub const PREVIEW_TTL: Duration = Duration::from_secs(600);

/// Longest addon name shown anywhere.
pub const NAME_CAP: usize = 80;
/// Longest description shown, as the core already caps it.
pub const DESCRIPTION_CAP: usize = 2000;

/// A management failure, including the one the core cannot report on its own.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AddonError {
    /// The core refused or failed the operation.
    Core(InstallError),
    /// The preview outlived its ten minutes.
    Expired,
}

impl From<InstallError> for AddonError {
    fn from(error: InstallError) -> Self {
        Self::Core(error)
    }
}

/// One line of plain copy: what happened, then what to do.
pub fn error_copy(error: &AddonError) -> &'static str {
    use InstallError as E;
    match error {
        AddonError::Expired => "This preview expired. Preview the addon again.",
        AddonError::Core(error) => match error {
            E::InvalidUrl => {
                "That isn't an addon URL. Use the https:// or stremio:// link that ends in manifest.json."
            }
            E::InvalidManifest => "This addon's manifest is incomplete, so it can't be installed.",
            E::TooLarge => "This addon's manifest is too large to install.",
            E::Network => "Couldn't reach the addon. Check the link and your connection.",
            E::Timeout => "The addon took too long to answer. Try again.",
            E::SignedOut => "Sign in to manage addons.",
            E::Protected => "Built-in addons can't be removed.",
            E::ImpersonatesProtected => {
                "This addon claims to be a built-in addon, so it wasn't installed."
            }
            E::InvalidOrder => "That order didn't work. Try moving the addon again.",
            E::InvalidSelection => "That choice isn't available anymore. Pick another one.",
            E::InvalidToken => "This preview is no longer valid. Preview the addon again.",
            E::ConfigurationRequired => {
                "This addon must be configured first. Finish its setup page, then paste the link it gives you."
            }
            E::Core => "Your addons are busy. Try again in a moment.",
            E::ApiPush => "Saved on this computer, but not to your Stremio account. Try again.",
            E::Storage => "Panorama couldn't save that on this computer. Try again.",
        },
    }
}

/// Unicode format characters (category Cf): bidi overrides and isolates, zero-width marks
/// and the like. They reorder or hide text, so untrusted names must not carry them.
const FORMAT_RANGES: &[(u32, u32)] = &[
    (0x00AD, 0x00AD),
    (0x0600, 0x0605),
    (0x061C, 0x061C),
    (0x06DD, 0x06DD),
    (0x070F, 0x070F),
    (0x0890, 0x0891),
    (0x08E2, 0x08E2),
    (0x180E, 0x180E),
    (0x200B, 0x200F),
    (0x202A, 0x202E),
    (0x2060, 0x2064),
    (0x2066, 0x206F),
    (0xFEFF, 0xFEFF),
    (0xFFF9, 0xFFFB),
    (0x110BD, 0x110BD),
    (0x110CD, 0x110CD),
    (0x13430, 0x1343F),
    (0x1BCA0, 0x1BCA3),
    (0x1D173, 0x1D17A),
    (0xE0001, 0xE0001),
    (0xE0020, 0xE007F),
];

fn is_format(c: char) -> bool {
    let code = c as u32;
    FORMAT_RANGES
        .iter()
        .any(|&(low, high)| (low..=high).contains(&code))
}

/// Remove control and format characters, flatten whitespace, trim, and cap length so
/// untrusted text is safe to lay out.
pub fn display_text(text: &str, cap: usize) -> String {
    text.chars()
        .filter(|c| (!c.is_control() || c.is_whitespace()) && !is_format(*c))
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .chars()
        .take(cap)
        .collect()
}

/// What an addon offers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Capability {
    /// Film catalogs.
    Catalogs,
    /// Synopses, credits and artwork.
    Details,
    /// Playable sources.
    Streams,
    /// Subtitle files.
    Subtitles,
}

impl Capability {
    /// Sentence-case label; callers uppercase through type styles.
    pub fn label(self) -> &'static str {
        match self {
            Self::Catalogs => "Catalogs",
            Self::Details => "Film details",
            Self::Streams => "Streams",
            Self::Subtitles => "Subtitles",
        }
    }
}

/// Distinct capabilities in a fixed display order, ignoring unknown resources.
pub fn capabilities(resources: &[String]) -> Vec<Capability> {
    [
        ("catalog", Capability::Catalogs),
        ("meta", Capability::Details),
        ("stream", Capability::Streams),
        ("subtitles", Capability::Subtitles),
    ]
    .into_iter()
    .filter(|(name, _)| resources.iter().any(|resource| resource == name))
    .map(|(_, capability)| capability)
    .collect()
}

/// Media types the preview names, in fixed order.
pub fn media_types(types: &[String]) -> Vec<&'static str> {
    [("movie", "Movies"), ("series", "Series")]
        .into_iter()
        .filter(|(name, _)| types.iter().any(|kind| kind == name))
        .map(|(_, label)| label)
        .collect()
}

/// Display fields of a previewed manifest; the token travels separately.
/// Debug is redacted because every field comes from an untrusted manifest.
#[derive(Clone, PartialEq)]
pub struct PreviewCard {
    /// Capped plain-text name.
    pub name: String,
    /// Version text.
    pub version: String,
    /// Capped description.
    pub description: Option<String>,
    /// HTTPS logo.
    pub logo: Option<Url>,
    /// What the addon offers.
    pub capabilities: Vec<Capability>,
    /// Movies and Series labels.
    pub types: Vec<&'static str>,
}

impl std::fmt::Debug for PreviewCard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PreviewCard { .. }")
    }
}

/// The install lifecycle. `T` is the confirmation token, opaque to this machine.
/// Debug names the state only.
pub enum InstallFlow<T> {
    /// Waiting for a link.
    Idle,
    /// The manifest is being fetched; only the pasted length is kept.
    Previewing {
        /// Characters in the pasted link.
        url_len: usize,
    },
    /// A manifest is waiting for confirmation.
    Preview {
        /// Display fields.
        card: PreviewCard,
        /// Single-use confirmation.
        token: T,
        /// When the confirmation lapses.
        expires: Instant,
    },
    /// The confirmed manifest is being installed.
    Installing {
        /// Display fields kept on screen during the install.
        card: PreviewCard,
    },
    /// The last step failed.
    Failed(AddonError),
}

impl<T> std::fmt::Debug for InstallFlow<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Idle => "InstallFlow::Idle",
            Self::Previewing { .. } => "InstallFlow::Previewing",
            Self::Preview { .. } => "InstallFlow::Preview",
            Self::Installing { .. } => "InstallFlow::Installing",
            Self::Failed(_) => "InstallFlow::Failed",
        })
    }
}

impl<T> InstallFlow<T> {
    /// A user edit of the link field. It discards a preview, whose manifest no longer
    /// matches the text, and clears an error. Returns whether the flow changed.
    pub fn edited(&mut self) -> bool {
        if matches!(self, Self::Preview { .. } | Self::Failed(_)) {
            *self = Self::Idle;
            return true;
        }
        false
    }

    /// Begin a preview. Empty links and in-flight work are ignored.
    pub fn submit(&mut self, url_len: usize) -> bool {
        if url_len == 0 || matches!(self, Self::Previewing { .. } | Self::Installing { .. }) {
            return false;
        }
        *self = Self::Previewing { url_len };
        true
    }

    /// Accept a fetch result. A result arriving after cancel is dropped.
    pub fn previewed(&mut self, result: Result<(PreviewCard, T), AddonError>, now: Instant) {
        if !matches!(self, Self::Previewing { .. }) {
            return;
        }
        *self = match result {
            Ok((card, token)) => Self::Preview {
                card,
                token,
                expires: now + PREVIEW_TTL,
            },
            Err(error) => Self::Failed(error),
        };
    }

    /// Start installing and hand the token to the caller, unless the preview lapsed.
    pub fn confirm(&mut self, now: Instant) -> Option<T> {
        self.expire(now);
        match std::mem::replace(self, Self::Idle) {
            Self::Preview { card, token, .. } => {
                *self = Self::Installing { card };
                Some(token)
            }
            other => {
                *self = other;
                None
            }
        }
    }

    /// Finish a successful install and return the installed name.
    pub fn installed(&mut self) -> Option<String> {
        match std::mem::replace(self, Self::Idle) {
            Self::Installing { card } => Some(card.name),
            other => {
                *self = other;
                None
            }
        }
    }

    /// Dismiss a preview, an error or an in-flight fetch. An install cannot be cancelled.
    pub fn cancel(&mut self) -> bool {
        if matches!(self, Self::Installing { .. } | Self::Idle) {
            return false;
        }
        *self = Self::Idle;
        true
    }

    /// Record a failure of the step in flight.
    pub fn failed(&mut self, error: AddonError) {
        if matches!(self, Self::Previewing { .. } | Self::Installing { .. }) {
            *self = Self::Failed(error);
        }
    }

    /// Turn a lapsed preview into `Failed(Expired)`.
    pub fn expire(&mut self, now: Instant) {
        if matches!(self, Self::Preview { expires, .. } if now >= *expires) {
            *self = Self::Failed(AddonError::Expired);
        }
    }

    /// Whether a request is in flight.
    pub fn busy(&self) -> bool {
        matches!(self, Self::Previewing { .. } | Self::Installing { .. })
    }
}

/// What a row's controls are doing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RowPending {
    /// Idle.
    None,
    /// A reorder is being saved.
    Moving,
    /// Waiting for Remove or Keep.
    ConfirmRemove,
    /// The core is removing it.
    Removing,
    /// The last action failed.
    Error(AddonError),
}

/// One installed addon as the list shows it. Debug is redacted (untrusted fields).
#[derive(Clone, PartialEq)]
pub struct InstalledRow {
    /// Salted installation identity.
    pub key: AddonKey,
    /// Capped plain-text name.
    pub name: String,
    /// Version text.
    pub version: String,
    /// HTTPS logo.
    pub logo: Option<Url>,
    /// What the addon offers.
    pub capabilities: Vec<Capability>,
    /// Built-in; cannot be removed.
    pub protected: bool,
    /// The core can open a configuration page.
    pub configurable: bool,
    /// Current control state.
    pub pending: RowPending,
}

impl std::fmt::Debug for InstalledRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("InstalledRow { .. }")
    }
}

/// Swap a row with its neighbour. `None` at either edge or for an unknown key.
/// The core accepts any permutation, so protected addons move like the rest.
pub fn move_row(order: &[AddonKey], key: &AddonKey, delta: isize) -> Option<Vec<AddonKey>> {
    let from = order.iter().position(|candidate| candidate == key)?;
    let to = from.checked_add_signed(delta)?;
    if to >= order.len() || delta == 0 {
        return None;
    }
    let mut next = order.to_vec();
    next.swap(from, to);
    Some(next)
}
