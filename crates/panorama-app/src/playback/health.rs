//! Port of `components/usePlaybackHealth.ts`: wait estimate and heavy-source detection.

/// Media that must be buffered ahead before mpv resumes (`cache-pause-wait`).
const RESUME_BUFFER_SECONDS: f64 = 5.0;
const RATE_SAMPLES: usize = 10;
const HEAVY_MIN_SAMPLES: usize = 8;
/// Only a clear, repeated shortfall warrants the notice; brief dips recover on their own.
const HEAVY_RATIO: f64 = 0.6;
const HEAVY_MIN_REBUFFERS: u32 = 3;
/// With more media ready than this the downloader idles by design, so its rate says nothing.
const DEMAND_BUFFER_SECONDS: f64 = 30.0;

/// One transport measurement taken by the host; carries no URL or token.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LinkHealth {
    /// Source bitrate from size and duration.
    pub source_mbps: Option<f64>,
    /// Proxy download rate, or mpv's `cache-speed` when the proxy has none.
    pub download_mbps: Option<f64>,
    /// Whether the proxy has readers waiting for bytes.
    pub transfer_demanded: Option<bool>,
}

/// Average of the most recent download samples.
pub fn average_rate(samples: &[f64]) -> Option<f64> {
    let recent = &samples[samples.len().saturating_sub(RATE_SAMPLES)..];
    (!recent.is_empty()).then(|| recent.iter().sum::<f64>() / recent.len() as f64)
}

/// Seconds until enough media is buffered to resume.
pub fn estimate_buffer_wait(
    cache_seconds: Option<f64>,
    source_mbps: Option<f64>,
    download_mbps: Option<f64>,
) -> Option<f64> {
    let (source, download) = (source_mbps?, download_mbps?);
    if download <= 0.0 {
        return None;
    }
    let missing = (RESUME_BUFFER_SECONDS - cache_seconds.unwrap_or(0.0)).max(0.0);
    Some(missing * source / download)
}

/// A source is too heavy once playback has stalled repeatedly and the sustained rate stays well below its need.
pub fn is_source_too_heavy(samples: &[f64], source_mbps: Option<f64>, rebuffers: u32) -> bool {
    let Some(source) = source_mbps else {
        return false;
    };
    if rebuffers < HEAVY_MIN_REBUFFERS || samples.len() < HEAVY_MIN_SAMPLES {
        return false;
    }
    average_rate(samples).is_some_and(|rate| rate < source * HEAVY_RATIO)
}

/// Rate with one decimal below 10 Mbps.
pub fn format_mbps(value: f64) -> String {
    if value >= 10.0 {
        format!("{}", value.round() as i64)
    } else {
        format!("{value:.1}")
    }
}

/// Wait in seconds, or minutes from one minute.
pub fn format_wait(seconds: f64) -> String {
    if seconds < 60.0 {
        format!("{} s", seconds.round().max(1.0) as i64)
    } else {
        format!("{} min", (seconds / 60.0).round() as i64)
    }
}

/// Accumulated playback health for the current source.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Health {
    samples: Vec<f64>,
    /// Most recent source bitrate.
    pub source_mbps: Option<f64>,
    /// Average recent demanded download rate.
    pub download_mbps: Option<f64>,
    /// Estimated seconds until playback resumes, only while buffering.
    pub wait_seconds: Option<f64>,
    /// Whether the source outpaces the connection.
    pub too_heavy: bool,
}

impl Health {
    /// Fold one host measurement with the player state it was taken in.
    pub fn observe(
        &mut self,
        link: LinkHealth,
        buffering: bool,
        cache_seconds: Option<f64>,
        rebuffers: u32,
    ) {
        let demanding = link
            .transfer_demanded
            .unwrap_or(buffering || cache_seconds.unwrap_or(0.0) < DEMAND_BUFFER_SECONDS);
        if let Some(rate) = link.download_mbps.filter(|rate| rate.is_finite())
            && demanding
        {
            self.samples.push(rate);
        }
        let keep = RATE_SAMPLES * 3;
        if self.samples.len() > keep {
            self.samples.drain(..self.samples.len() - keep);
        }
        self.source_mbps = link.source_mbps.filter(|rate| rate.is_finite());
        self.download_mbps = average_rate(&self.samples);
        self.too_heavy = is_source_too_heavy(&self.samples, self.source_mbps, rebuffers);
        self.wait_seconds = buffering
            .then(|| estimate_buffer_wait(cache_seconds, self.source_mbps, self.download_mbps))
            .flatten();
    }

    /// The Buffering label, with the wait once it is long enough to matter.
    pub fn buffering_label(&self) -> String {
        match self.wait_seconds {
            Some(wait) if wait >= 5.0 => format!("Buffering · about {}", format_wait(wait)),
            _ => "Buffering".into(),
        }
    }

    /// Copy for the heavy-source notice, when its measurements exist.
    pub fn heavy_copy(&self) -> Option<String> {
        let (source, download) = (self.source_mbps?, self.download_mbps?);
        Some(format!(
            "This source needs about {} Mbps. The connection is delivering {} Mbps. Pause to let it download ahead, or choose a lighter source.",
            format_mbps(source),
            format_mbps(download)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_estimate_cases() {
        let close = |value: Option<f64>, expected: f64| {
            assert!((value.unwrap() - expected).abs() < 0.05, "{value:?}");
        };
        close(
            estimate_buffer_wait(Some(0.0), Some(37.6), Some(1.4)),
            134.3,
        );
        close(estimate_buffer_wait(Some(4.0), Some(20.0), Some(10.0)), 2.0);
        close(estimate_buffer_wait(Some(8.0), Some(20.0), Some(10.0)), 0.0);
        assert_eq!(estimate_buffer_wait(Some(0.0), None, Some(10.0)), None);
        assert_eq!(estimate_buffer_wait(Some(0.0), Some(20.0), Some(0.0)), None);
    }

    #[test]
    fn a_source_is_heavy_only_after_repeated_stalls_with_a_sustained_shortfall() {
        let slow = [1.4; 10];
        assert!(is_source_too_heavy(&slow, Some(37.6), 3));
        assert!(!is_source_too_heavy(&slow, Some(37.6), 2));
        assert!(!is_source_too_heavy(&slow[..4], Some(37.6), 3));
        assert!(!is_source_too_heavy(&[30.0; 10], Some(37.6), 5));
        assert!(!is_source_too_heavy(&slow, None, 3));
    }

    #[test]
    fn only_recent_samples_are_averaged() {
        assert_eq!(average_rate(&[]), None);
        let mut samples = vec![100.0; 10];
        samples.extend([2.0; 10]);
        assert_eq!(average_rate(&samples), Some(2.0));
    }

    #[test]
    fn rates_and_waits_are_concise() {
        assert_eq!(format_mbps(37.6), "38");
        assert_eq!(format_mbps(1.43), "1.4");
        assert_eq!(format_wait(12.4), "12 s");
        assert_eq!(format_wait(0.2), "1 s");
        assert_eq!(format_wait(134.0), "2 min");
    }

    #[test]
    fn observing_links_drives_wait_label_and_notice() {
        let mut health = Health::default();
        let link = LinkHealth {
            source_mbps: Some(8.0),
            download_mbps: Some(2.0),
            transfer_demanded: Some(true),
        };
        health.observe(link, true, Some(0.0), 0);
        assert_eq!(health.buffering_label(), "Buffering · about 20 s");
        assert!(!health.too_heavy);
        health.observe(link, true, Some(4.5), 0);
        assert_eq!(health.buffering_label(), "Buffering");
        for _ in 0..8 {
            health.observe(link, false, Some(1.0), 3);
        }
        assert!(health.too_heavy);
        assert_eq!(health.wait_seconds, None);
        assert_eq!(
            health.heavy_copy().as_deref(),
            Some(
                "This source needs about 8.0 Mbps. The connection is delivering 2.0 Mbps. Pause to let it download ahead, or choose a lighter source."
            )
        );
    }

    #[test]
    fn idle_downloaders_do_not_count_as_samples() {
        let mut health = Health::default();
        let idle = LinkHealth {
            source_mbps: Some(8.0),
            download_mbps: Some(0.1),
            transfer_demanded: None,
        };
        health.observe(idle, false, Some(60.0), 1);
        assert_eq!(health.download_mbps, None);
        health.observe(idle, false, Some(10.0), 1);
        assert_eq!(health.download_mbps, Some(0.1));
    }
}
