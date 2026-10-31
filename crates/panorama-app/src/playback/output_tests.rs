use super::*;

fn device(name: &str, codecs: &[&str]) -> Device {
    Device {
        name: name.into(),
        label: format!("{name} label"),
        passthrough: codecs.iter().map(|codec| (*codec).to_owned()).collect(),
    }
}

fn map(settings: &PlaybackSettings, devices: &[Device]) -> Vec<(&'static str, String)> {
    properties(settings, devices, true)
}

#[test]
fn defaults_map_to_the_controllers_auto_properties() {
    assert_eq!(
        map(&PlaybackSettings::default(), &[]),
        [
            ("audio-spdif", String::new()),
            ("audio-device", "auto".into()),
            ("audio-channels", "auto-safe".into()),
            ("audio-spdif", String::new()),
            ("target-colorspace-hint", "auto".into()),
            ("target-prim", "auto".into()),
            ("target-trc", "auto".into()),
        ]
    );
}

#[test]
fn channels_and_sdr_map_to_their_mpv_values() {
    for (channels, expected) in [
        (Channels::Auto, "auto-safe"),
        (Channels::Stereo, "stereo"),
        (Channels::Surround51, "5.1"),
        (Channels::Surround71, "7.1"),
    ] {
        let settings = PlaybackSettings {
            channels,
            ..Default::default()
        };
        assert_eq!(map(&settings, &[])[2].1, expected);
    }
    let sdr = PlaybackSettings {
        video: Video::Sdr,
        ..Default::default()
    };
    let props = map(&sdr, &[]);
    assert_eq!(props[4], ("target-colorspace-hint", "no".into()));
    assert_eq!(props[5], ("target-prim", "bt.709".into()));
    assert_eq!(props[6], ("target-trc", "bt.1886".into()));
}

#[test]
fn passthrough_needs_a_confirmed_codec_and_the_users_opt_in() {
    let devices = [device("wasapi/hdmi", &["ac3", "eac3", "flac", "dts"])];
    let mut settings = PlaybackSettings {
        device: "wasapi/hdmi".into(),
        ..Default::default()
    };
    assert_eq!(map(&settings, &devices)[3].1, "", "not opted in");
    settings = with_passthrough(&settings, true);
    assert!(settings.passthrough());
    let props = map(&settings, &devices);
    assert_eq!(props[1].1, "wasapi/hdmi");
    assert_eq!(
        props[3].1, "ac3,eac3,dts",
        "only passthrough codecs, in device order"
    );
    assert_eq!(
        map(&settings, &[device("wasapi/hdmi", &[])])[3].1,
        "",
        "nothing confirmed"
    );
    assert!(!device("x", &["flac"]).passthrough_available());
    assert!(device("x", &["truehd"]).passthrough_available());
    assert!(!with_passthrough(&settings, false).passthrough());
    // The system default never holds a receiver entry.
    assert!(
        with_passthrough(&PlaybackSettings::default(), true)
            .receivers
            .is_empty()
    );
}

#[test]
fn choosing_a_device_keeps_its_remembered_passthrough() {
    let settings = PlaybackSettings {
        receivers: vec!["wasapi/hdmi".into()],
        ..Default::default()
    };
    assert!(!settings.passthrough());
    assert!(with_device(&settings, "wasapi/hdmi").passthrough());
    assert!(!with_device(&settings, "wasapi/usb").passthrough());
}

#[test]
fn a_vanished_device_falls_back_with_a_notice() {
    let mut settings = PlaybackSettings {
        device: "wasapi/usb".into(),
        ..Default::default()
    };
    assert_eq!(reconcile(&mut settings, &[device("wasapi/usb", &[])]), None);
    let notice = reconcile(&mut settings, &[device("wasapi/hdmi", &[])]);
    assert_eq!(
        notice,
        Some("Audio output disconnected. Using system default with decoded audio.")
    );
    assert_eq!(settings.device, "auto");
    assert_eq!(reconcile(&mut settings, &[]), None);
}

#[test]
fn the_select_lists_the_default_first_and_labels_unnamed_outputs() {
    let devices = select_devices(&[
        device("auto", &[]),
        Device {
            name: "wasapi/x".into(),
            label: String::new(),
            passthrough: vec![],
        },
    ]);
    assert_eq!(devices.len(), 2);
    assert_eq!(devices[0].label, "System default");
    assert_eq!(devices[1].label, "Audio output");
}

#[test]
fn stored_settings_round_trip_and_reject_malformed_input() {
    let settings = PlaybackSettings {
        device: "wasapi/hdmi".into(),
        channels: Channels::Surround51,
        video: Video::Sdr,
        receivers: vec!["wasapi/hdmi".into()],
    };
    assert_eq!(PlaybackSettings::from_bytes(&settings.to_bytes()), settings);
    for bytes in [
        &b""[..],
        b"{",
        b"[]",
        br#"{"device":"a","channels":"9.1","video":"auto","receivers":[]}"#,
        br#"{"device":"a","channels":"auto","video":"hdr","receivers":[]}"#,
        br#"{"device":"a","channels":"auto","video":"auto","receivers":["auto"]}"#,
        br#"{"device":"a","channels":"auto","video":"auto","receivers":[1]}"#,
        br#"{"device":7,"channels":"auto","video":"auto","receivers":[]}"#,
    ] {
        assert_eq!(
            PlaybackSettings::from_bytes(bytes),
            PlaybackSettings::default()
        );
    }
}

#[test]
fn the_colorspace_hint_is_only_set_for_a_renderer_that_takes_it() {
    let sdr = PlaybackSettings {
        video: Video::Sdr,
        ..Default::default()
    };
    let with = properties(&sdr, &[], true);
    let without = properties(&sdr, &[], false);
    assert!(with.contains(&("target-colorspace-hint", "no".into())));
    assert!(
        !without
            .iter()
            .any(|(name, _)| *name == "target-colorspace-hint")
    );
    assert_eq!(with.len(), without.len() + 1);
    assert!(without.contains(&("target-prim", "bt.709".into())));
}
