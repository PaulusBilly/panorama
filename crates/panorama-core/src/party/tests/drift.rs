use super::*;

const LONG_AGO: i64 = 60_000;

#[track_caller]
fn assert_speed(correction: Correction, expected: f64) {
    match correction {
        Correction::Speed(speed) => assert!(
            (speed - expected).abs() < 1e-9,
            "speed {speed}, expected {expected}"
        ),
        other => panic!("expected Speed({expected}), got {other:?}"),
    }
}

#[test]
fn small_error_does_nothing() {
    assert_eq!(correct(100.2, 100.0, 1.0, LONG_AGO), Correction::None);
    assert_eq!(correct(99.8, 100.0, 1.0, LONG_AGO), Correction::None);
}

#[test]
fn medium_error_slows_down() {
    assert_speed(correct(101.0, 100.0, 1.0, LONG_AGO), 0.95);
    assert_speed(correct(100.3, 100.0, 1.0, LONG_AGO), 0.97);
}

#[test]
fn medium_error_speeds_up() {
    assert_speed(correct(99.5, 100.0, 1.0, LONG_AGO), 1.05);
}

#[test]
fn keeps_correcting_until_inside_100ms() {
    assert_speed(correct(100.2, 100.0, 0.95, LONG_AGO), 0.98);
}

#[test]
fn back_to_normal_inside_100ms() {
    assert_eq!(
        correct(100.05, 100.0, 0.95, LONG_AGO),
        Correction::Speed(1.0)
    );
}

#[test]
fn unchanged_speed_is_not_resent() {
    let Correction::Speed(speed) = correct(101.5, 100.0, 1.0, LONG_AGO) else {
        panic!("expected a speed correction");
    };
    assert_eq!(correct(101.9, 100.0, speed, LONG_AGO), Correction::None);
}

#[test]
fn large_error_seeks() {
    assert_eq!(
        correct(103.0, 100.0, 1.0, LONG_AGO),
        Correction::Seek(100.15)
    );
    assert_eq!(
        correct(98.0, 100.0, 1.0, LONG_AGO),
        Correction::Seek(100.15)
    );
}

#[test]
fn no_correction_within_1500ms_of_seek() {
    assert_eq!(correct(110.0, 100.0, 1.0, 1_499), Correction::None);
    assert_eq!(correct(110.0, 100.0, 1.0, 1_500), Correction::Seek(100.15));
}
