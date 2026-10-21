#[cfg(not(target_os = "macos"))]
pub(crate) const OPTIONS: &[(&str, &str, bool)] = &[
    ("terminal", "no", false),
    ("msg-level", "all=warn", false),
    ("keep-open", "yes", false),
    ("hr-seek", "default", false),
    ("vo", "gpu-next", false),
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
    // Per-frame HDR peak measurement is a compute pass on every 4K frame; HDR10/HDR10+
    // metadata already carries the peak, and skipping it keeps fanless Macs cooler.
    ("hdr-compute-peak", "no", false),
];
