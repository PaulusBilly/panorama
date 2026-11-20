use super::*;

#[test]
fn symmetric_delay_gives_exact_offset() {
    let mut clock = ClockSync::new();
    clock.record(1_000, 6_050, 1_100);
    assert_eq!(clock.offset_ms(), Some(5_000));
    assert_eq!(clock.host_now(2_000), Some(7_000));
}

#[test]
fn no_offset_before_a_sample() {
    assert_eq!(ClockSync::new().offset_ms(), None);
}

#[test]
fn lowest_rtt_sample_wins() {
    let mut clock = ClockSync::new();
    clock.record(0, 5_300 + 200, 400);
    clock.record(1_000, 6_010 + 20, 1_040);
    clock.record(2_000, 7_200 + 100, 2_200);
    assert_eq!(clock.offset_ms(), Some(5_010));
}

#[test]
fn keeps_only_last_eight() {
    let mut clock = ClockSync::new();
    clock.record(0, 1_000 + 5, 10);
    for i in 1..=8 {
        clock.record(i * 1_000, i * 1_000 + 2_000 + 50, i * 1_000 + 100);
    }
    assert_eq!(clock.offset_ms(), Some(2_000));
}

#[test]
fn stale_samples_are_dropped_after_gap() {
    let mut clock = ClockSync::new();
    assert!(!clock.note_local_time(0));
    clock.record(0, 5_050, 100);
    assert!(!clock.note_local_time(5_000));
    assert_eq!(clock.offset_ms(), Some(5_000));
    assert!(clock.note_local_time(10_001));
    assert_eq!(clock.offset_ms(), None);
}
