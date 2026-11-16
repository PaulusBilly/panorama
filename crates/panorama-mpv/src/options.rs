#[cfg(not(target_os = "macos"))]
pub(crate) const OPTIONS: &[(&str, &str, bool)] = &[
    ("terminal", "no", false),
    ("msg-level", "all=warn", false),
    ("keep-open", "yes", false),
    ("hr-seek", "default", false),
    ("vo", "gpu-next", false),
    // mpv's window lives as long as the player and is black while there is no picture. Without
    // it mpv has no window before the first frame or between files, and the surface shows
    // stale pixels there: the film page, or whatever window last covered it.
    ("force-window", "immediate", false),
    ("gpu-api", "d3d11", false),
    ("gpu-context", "d3d11", false),
    ("target-colorspace-hint", "auto", false),
    ("hwdec", "d3d11va,auto-safe", false),
    ("cache", "yes", false),
    // 60 s ahead (~100 MB at 13 Mbps). Sources already sit in the on-disk media/torrent cache,
    // so 256 MiB (~30 s of an 80 Mbps remux) is plenty in RAM.
    ("cache-secs", "60", false),
    ("demuxer-max-bytes", "256MiB", false),
    ("demuxer-max-back-bytes", "32MiB", false),
    ("cache-pause", "yes", false),
    ("cache-pause-wait", "2", false),
    ("cache-on-disk", "no", false),
    ("demuxer-cache-wait", "no", false),
    ("cache-pause-initial", "yes", true),
    ("network-timeout", "60", true),
    ("osc", "no", false),
    ("input-default-bindings", "no", false),
    ("input-vo-keyboard", "no", false),
    ("audio-client-name", "Panorama", false),
];

/// macOS options: VideoToolbox decoding, rendered by `gpu-next`
/// through MoltenVK into the `panorama-macvk` Metal surface.
#[cfg(target_os = "macos")]
pub(crate) const OPTIONS: &[(&str, &str, bool)] = &[
    ("terminal", "no", false),
    ("msg-level", "all=warn", false),
    ("keep-open", "yes", false),
    ("hr-seek", "default", false),
    ("hwdec", "auto-safe", false),
    ("gpu-hwdec-interop", "auto", false),
    ("cache", "yes", false),
    // 60 s ahead (~100 MB at 13 Mbps). Sources already sit in the on-disk media/torrent cache,
    // so 256 MiB (~30 s of an 80 Mbps remux) is plenty in RAM.
    ("cache-secs", "60", false),
    ("demuxer-max-bytes", "256MiB", false),
    ("demuxer-max-back-bytes", "32MiB", false),
    ("cache-pause", "yes", false),
    ("cache-pause-wait", "2", false),
    ("cache-on-disk", "no", false),
    ("demuxer-cache-wait", "no", false),
    ("cache-pause-initial", "yes", true),
    ("network-timeout", "60", true),
    ("audio-client-name", "Panorama", false),
    ("vo", "gpu-next", false),
    ("gpu-api", "vulkan", false),
    ("gpu-context", "panorama-macvk", false),
    ("target-colorspace-hint", "auto", false),
    // HDR goes to the layer as PQ so macOS tone-maps it to the screen's EDR headroom, as
    // AVFoundation does for Apple TV and QuickTime. The default (`target`) tone-mapped in
    // libplacebo against a headroom read before any EDR content was on screen, and came
    // out visibly darker than QuickTime from the mid-tones up. SDR is unaffected.
    // `-dynamic` hands macOS each scene's peak from Dolby Vision or HDR10+ metadata, so a
    // dark scene is mapped for its own range rather than the film's brightest frame, as
    // Apple TV does with Dolby Vision; plain HDR10 falls back to MaxCLL.
    ("target-colorspace-hint-mode", "source-dynamic", false),
    // Per-frame HDR peak measurement is a compute pass on every 4K frame; HDR10/HDR10+
    // metadata already carries the peak, and skipping it keeps fanless Macs cooler.
    ("hdr-compute-peak", "no", false),
];
