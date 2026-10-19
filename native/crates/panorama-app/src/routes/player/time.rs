//! Time labels, ported from `formatTime` in `PlayerDialog.tsx`.

/// `m:ss` or `h:mm:ss`; non-finite and negative values read as `0:00`.
pub fn format_time(value: f64) -> String {
    if !value.is_finite() || value < 0.0 {
        return "0:00".into();
    }
    let seconds = (value % 60.0).floor() as u64;
    let minutes = ((value / 60.0).floor() as u64) % 60;
    let hours = (value / 3600.0).floor() as u64;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// The right-hand label: time left, or a placeholder while the duration is unknown.
pub fn remaining(duration: Option<f64>, shown: f64) -> String {
    match duration {
        Some(duration) => format!("-{}", format_time((duration - shown).max(0.0))),
        None => "--:--".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_like_the_electron_player() {
        for (value, expected) in [
            (0.0, "0:00"),
            (5.9, "0:05"),
            (59.99, "0:59"),
            (60.0, "1:00"),
            (754.0, "12:34"),
            (3600.0, "1:00:00"),
            (3725.4, "1:02:05"),
            (36_000.0, "10:00:00"),
            (-1.0, "0:00"),
            (f64::NAN, "0:00"),
            (f64::INFINITY, "0:00"),
        ] {
            assert_eq!(format_time(value), expected, "{value}");
        }
    }

    #[test]
    fn remaining_clamps_and_marks_unknown_durations() {
        assert_eq!(remaining(Some(6100.0), 1200.0), "-1:21:40");
        assert_eq!(remaining(Some(10.0), 20.0), "-0:00");
        assert_eq!(remaining(None, 20.0), "--:--");
    }
}
