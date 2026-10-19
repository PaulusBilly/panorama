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

#[cfg(test)]
mod tests {
    use super::*;

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
}
