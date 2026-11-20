/// Under this, playback is in sync.
const DEADBAND: f64 = 0.25;
/// A speed correction runs until the error is back under this.
const SETTLED: f64 = 0.1;
/// From this, catching up by speed would take too long.
const SEEK_FROM: f64 = 2.0;
/// Covers the time a seek takes to land.
const SEEK_LEAD: f64 = 0.15;
const SEEK_COOLDOWN_MS: i64 = 1_500;
const GAIN: f64 = 0.1;
const MAX_NUDGE: f64 = 0.05;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Correction {
    None,
    Speed(f64),
    Seek(f64),
}

/// What a member's player should do to meet the party's `expected` position. A position just
/// after a seek is not trusted yet.
pub fn correct(local: f64, expected: f64, current_speed: f64, ms_since_seek: i64) -> Correction {
    if ms_since_seek < SEEK_COOLDOWN_MS {
        return Correction::None;
    }
    let error = local - expected;
    if error.abs() >= SEEK_FROM {
        return Correction::Seek(expected + SEEK_LEAD);
    }
    let correcting = current_speed != 1.0;
    let speed = if error.abs() >= DEADBAND || (correcting && error.abs() >= SETTLED) {
        1.0 - (error * GAIN).clamp(-MAX_NUDGE, MAX_NUDGE)
    } else {
        1.0
    };
    if speed == current_speed {
        Correction::None
    } else {
        Correction::Speed(speed)
    }
}
