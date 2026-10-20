//! Audio and video output settings, as mpv properties.
use serde_json::{Value, json};

/// Store key (under `pref:`) of the persisted settings.
pub const KEY: &str = "playbackSettings";
/// The system default output, mpv's `auto` device.
pub const DEFAULT_DEVICE: &str = "auto";
/// Codecs a receiver can take undecoded.
const PASSTHROUGH_CODECS: [&str; 5] = ["ac3", "dts", "eac3", "truehd", "dts-hd"];

/// Decoded channel layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Channels {
    /// Whatever the output supports.
    Auto,
    /// Two channels.
    Stereo,
    /// 5.1.
    Surround51,
    /// 7.1.
    Surround71,
}

impl Channels {
    /// The select items in display order.
    pub const ALL: [Self; 4] = [Self::Auto, Self::Stereo, Self::Surround51, Self::Surround71];

    /// The label and stored name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Stereo => "stereo",
            Self::Surround51 => "5.1",
            Self::Surround71 => "7.1",
        }
    }

    /// The select label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Stereo => "Stereo",
            Self::Surround51 => "5.1",
            Self::Surround71 => "7.1",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|item| item.name() == value)
    }
}

/// Video output colour handling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Video {
    /// Pass the source colours to the display.
    Auto,
    /// Map to SDR (BT.709 / BT.1886).
    Sdr,
}

impl Video {
    /// The select items in display order.
    pub const ALL: [Self; 2] = [Self::Auto, Self::Sdr];

    /// The stored name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Sdr => "sdr",
        }
    }

    /// The select label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Sdr => "SDR",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|item| item.name() == value)
    }
}

/// The persisted playback settings.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlaybackSettings {
    /// The mpv `audio-device` name, or [`DEFAULT_DEVICE`].
    pub device: String,
    /// Decoded channel layout.
    pub channels: Channels,
    /// Video colour handling.
    pub video: Video,
    /// Devices whose receiver passthrough the user turned on.
    pub receivers: Vec<String>,
}

impl Default for PlaybackSettings {
    fn default() -> Self {
        Self {
            device: DEFAULT_DEVICE.into(),
            channels: Channels::Auto,
            video: Video::Auto,
            receivers: vec![],
        }
    }
}

impl PlaybackSettings {
    /// Parse stored bytes; anything malformed is the defaults, as the controller's constructor.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
            return Self::default();
        };
        let text = |name: &str| value.get(name).and_then(Value::as_str);
        let (Some(device), Some(channels), Some(video), Some(receivers)) = (
            text("device").filter(|device| device.len() <= 4096),
            text("channels").and_then(Channels::parse),
            text("video").and_then(Video::parse),
            value.get("receivers").and_then(Value::as_array),
        ) else {
            return Self::default();
        };
        let receivers: Option<Vec<String>> = receivers
            .iter()
            .map(|entry| {
                entry
                    .as_str()
                    .filter(|name| *name != DEFAULT_DEVICE && name.len() <= 4096)
                    .map(str::to_owned)
            })
            .collect();
        match receivers {
            Some(receivers) if receivers.len() <= 100 => Self {
                device: device.to_owned(),
                channels,
                video,
                receivers,
            },
            _ => Self::default(),
        }
    }

    /// The persisted JSON.
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "device": self.device,
            "channels": self.channels.name(),
            "video": self.video.name(),
            "receivers": self.receivers,
        }))
        .unwrap_or_default()
    }

    /// Whether the user turned passthrough on for the current device.
    pub fn passthrough(&self) -> bool {
        self.receivers.contains(&self.device)
    }
}

/// An audio output device mpv reported.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Device {
    /// The `audio-device` value.
    pub name: String,
    /// The label shown in the select.
    pub label: String,
    /// Codecs the device is confirmed to take undecoded. libmpv does not report this, so the
    /// native host passes none and passthrough stays unavailable until a probe exists.
    pub passthrough: Vec<String>,
}

impl Device {
    /// Whether any passthrough codec is confirmed.
    pub fn passthrough_available(&self) -> bool {
        self.allowed().next().is_some()
    }

    fn allowed(&self) -> impl Iterator<Item = &str> {
        self.passthrough
            .iter()
            .map(String::as_str)
            .filter(|codec| PASSTHROUGH_CODECS.contains(codec))
    }
}

/// The devices the select offers: the system default first, then every named output.
pub fn select_devices(reported: &[Device]) -> Vec<Device> {
    let mut devices = vec![Device {
        name: DEFAULT_DEVICE.into(),
        label: "System default".into(),
        passthrough: vec![],
    }];
    devices.extend(
        reported
            .iter()
            .filter(|device| device.name != DEFAULT_DEVICE)
            .map(|device| Device {
                label: if device.label.is_empty() {
                    "Audio output".into()
                } else {
                    device.label.clone()
                },
                ..device.clone()
            }),
    );
    devices
}

/// The mpv properties for these settings, in the order the controller applies them.
pub fn properties(
    settings: &PlaybackSettings,
    devices: &[Device],
    hint_supported: bool,
) -> Vec<(&'static str, String)> {
    let text = |value: &str| value.to_owned();
    let allowed: Vec<&str> = devices
        .iter()
        .find(|device| device.name == settings.device)
        .map(|device| device.allowed().collect())
        .unwrap_or_default();
    let encoded = settings.passthrough() && !allowed.is_empty();
    let sdr = settings.video == Video::Sdr;
    let mut properties = vec![
        ("audio-spdif", text("")),
        ("audio-device", text(&settings.device)),
        (
            "audio-channels",
            text(match settings.channels {
                Channels::Auto => "auto-safe",
                channels => channels.name(),
            }),
        ),
        (
            "audio-spdif",
            text(&if encoded {
                allowed.join(",")
            } else {
                String::new()
            }),
        ),
        ("target-prim", text(if sdr { "bt.709" } else { "auto" })),
        ("target-trc", text(if sdr { "bt.1886" } else { "auto" })),
    ];
    // The controller sets the hint only for the d3d11 and macvk renderers.
    if hint_supported {
        properties.insert(
            4,
            (
                "target-colorspace-hint",
                text(if sdr { "no" } else { "auto" }),
            ),
        );
    }
    properties
}

/// Whether this build's renderer takes `target-colorspace-hint`: the d3d11 backend of the
/// Windows build (`gpu-api=d3d11` in the panorama-mpv options).
pub const HINT_SUPPORTED: bool = cfg!(windows);

/// The state line under the video select.
pub fn status_line(notice: Option<&str>, effective_passthrough: bool) -> String {
    notice
        .unwrap_or(if effective_passthrough {
            "Receiver passthrough active"
        } else {
            "Decoded audio"
        })
        .to_owned()
}

/// Settings against the devices now present: a saved device that vanished falls back to the
/// system default with a notice (`poll`/`initialize` in the controller).
pub fn reconcile(settings: &mut PlaybackSettings, devices: &[Device]) -> Option<&'static str> {
    if settings.device != DEFAULT_DEVICE
        && !devices.iter().any(|device| device.name == settings.device)
    {
        settings.device = DEFAULT_DEVICE.into();
        return Some("Audio output disconnected. Using system default with decoded audio.");
    }
    None
}

/// Apply the user's choice of device: passthrough follows what was remembered for it.
pub fn with_device(settings: &PlaybackSettings, device: &str) -> PlaybackSettings {
    PlaybackSettings {
        device: device.to_owned(),
        ..settings.clone()
    }
}

/// Turn passthrough on or off for the current device.
pub fn with_passthrough(settings: &PlaybackSettings, on: bool) -> PlaybackSettings {
    let mut next = settings.clone();
    next.receivers.retain(|name| *name != settings.device);
    if on && settings.device != DEFAULT_DEVICE {
        next.receivers.push(settings.device.clone());
    }
    next
}

#[cfg(test)]
#[path = "output_tests.rs"]
mod tests;
