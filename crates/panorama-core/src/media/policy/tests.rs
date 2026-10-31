//! Test equivalents from `tests/unit/buffering-policy.test.ts`.
use super::*;
fn sample(now_ms: u64) -> BufferingSample {
    BufferingSample {
        now_ms,
        buffered_seconds: Some(10.0),
        source_mbps: Some(8.0),
        download_mbps: Some(12.0),
        transfer_demanded: true,
        buffering: false,
        paused: false,
        playback_speed: 1.0,
        throttled: false,
    }
}

#[test]
fn does_not_probe_capacity_during_paused_or_intentionally_idle_downloads() {
    let mut policy = BufferingPolicy::new();
    for now in (0..=60000).step_by(1000) {
        let mut value = sample(now);
        value.transfer_demanded = false;
        assert_eq!(policy.update(&value).parallel, 3);
    }
    let mut value = sample(61000);
    value.paused = true;
    assert_eq!(policy.update(&value).parallel, 3);
}
#[test]
fn reverts_an_extra_connection_when_useful_throughput_does_not_improve() {
    let mut policy = BufferingPolicy::new();
    for now in (0..10000).step_by(1000) {
        policy.update(&sample(now));
    }
    assert_eq!(policy.update(&sample(10000)).parallel, 4);
    for now in (11000..15000).step_by(1000) {
        policy.update(&sample(now));
    }
    assert_eq!(policy.update(&sample(15000)).parallel, 3);
}
#[test]
fn retains_a_useful_increase_then_respects_throttling_and_buffer_limits() {
    let mut policy = BufferingPolicy::new();
    for now in (0..=10000).step_by(1000) {
        policy.update(&sample(now));
    }
    for now in (11000..15000).step_by(1000) {
        let mut value = sample(now);
        value.download_mbps = Some(16.0);
        policy.update(&value);
    }
    let mut value = sample(15000);
    value.download_mbps = Some(16.0);
    assert_eq!(policy.update(&value).parallel, 4);
    value.now_ms = 15001;
    value.throttled = true;
    assert_eq!(policy.update(&value).parallel, 3);
    for now in (16000..100000).step_by(1000) {
        let mut value = sample(now);
        value.buffering = now % 2000 == 0;
        value.download_mbps = Some(20.0);
        let decision = policy.update(&value);
        assert!((1..=6).contains(&decision.parallel));
        assert!(decision.target_ahead_seconds <= 120);
        assert_eq!(decision.resume_buffer_seconds, 5);
    }
}
#[test]
fn comfortable_buffer_uses_one_connection_and_idle_cancels_probe() {
    let mut policy = BufferingPolicy::new();
    for now in (0..=10000).step_by(1000) {
        let mut value = sample(now);
        value.buffered_seconds = Some(80.0);
        policy.update(&value);
    }
    assert_eq!(policy.parallel, 1);
    let mut policy = BufferingPolicy::new();
    for now in (0..=10000).step_by(1000) {
        policy.update(&sample(now));
    }
    let mut value = sample(10001);
    value.paused = true;
    assert_eq!(policy.update(&value).parallel, 3);
    value.throttled = true;
    for _ in 0..10 {
        assert!(policy.update(&value).parallel >= 1);
    }
}
