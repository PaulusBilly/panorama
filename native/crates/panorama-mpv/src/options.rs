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
    ("demuxer-max-bytes", "512MiB", false),
    ("demuxer-max-back-bytes", "64MiB", false),
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

/// The Electron macOS host's options: VideoToolbox decoding, rendered by `gpu-next`
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
    ("demuxer-max-bytes", "512MiB", false),
    ("demuxer-max-back-bytes", "64MiB", false),
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
];

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn cpp_options_match_order_values_and_best_effort() {
        let cpp = include_str!("../../../../desktop/native/mpv-host/src/addon_win.cc");
        let found: Vec<_> = cpp
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                let best_effort = line.starts_with("TrySetOption(\"");
                if !best_effort && !line.starts_with("SetOption(\"") {
                    return None;
                }
                let fields: Vec<_> = line.split('"').collect();
                if fields.len() != 5 {
                    return None;
                }
                Some((fields[1], fields[3], best_effort))
            })
            .collect();
        assert_eq!(found, OPTIONS);
        assert!(cpp.contains("SetOption(\"wid\", std::to_string"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn objc_options_match_order_and_values() {
        let objc = include_str!("../../../../desktop/native/mpv-host/src/addon.mm");
        let found: Vec<_> = objc
            .lines()
            .filter_map(|line| line.trim().strip_prefix("mpv_set_option_string(_mpv, \""))
            .filter_map(|rest| {
                let fields: Vec<_> = rest.split('"').collect();
                (fields.len() == 4).then(|| (fields[0], fields[2]))
            })
            .collect();
        let ours: Vec<_> = OPTIONS
            .iter()
            .map(|&(key, value, _)| (key, value))
            .collect();
        assert_eq!(found, ours);
        assert!(objc.contains("mpv_set_option_string(_mpv, \"wid\", descriptor.c_str())"));
    }
}
