use super::*;
use serde_json::json;

#[test]
fn defaults_match_the_runtime_snapshot() {
    let defaults = SubtitlePreferences::default();
    assert_eq!(hex(defaults.text_color), "#ffffff");
    assert_eq!(
        (
            defaults.text_opacity,
            defaults.font_size,
            defaults.padding_x,
            defaults.padding_y,
            defaults.line_height,
            defaults.box_opacity,
            defaults.box_rounding,
            defaults.delay,
            defaults.vertical_position
        ),
        (100.0, 38.0, 20.0, 0.0, 1.24, 68.0, 0.0, 0.0, 15.0)
    );
    assert_eq!(defaults.font_weight, FontWeight::Regular);
    assert_eq!(SubtitlePreferences::normalise(&json!(null)), defaults);
    assert_eq!(SubtitlePreferences::normalise(&json!([1, 2])), defaults);
    assert_eq!(SubtitlePreferences::normalise(&json!({})), defaults);
}

#[test]
fn every_field_clamps_to_its_range() {
    for field in Field::ALL {
        let (min, max) = field.range();
        let mut prefs = SubtitlePreferences::default();
        prefs.set(field, max + 1000.0);
        assert_eq!(prefs.get(field), max, "{field:?} max");
        prefs.set(field, min - 1000.0);
        assert_eq!(prefs.get(field), min, "{field:?} min");
    }
}

#[test]
fn steps_limits_and_labels_follow_the_tsx() {
    let table: Vec<_> = Field::ALL
        .iter()
        .map(|field| (field.label(), field.step(), field.range(), field.suffix()))
        .collect();
    assert_eq!(
        table,
        [
            ("Text opacity", 1.0, (0.0, 100.0), "%"),
            ("Delay", 0.5, (-10.0, 10.0), ""),
            ("Font size", 1.0, (12.0, 96.0), "px"),
            ("Vertical Position", 5.0, (0.0, 100.0), "%"),
            ("Horizontal Padding", 1.0, (0.0, 64.0), "px"),
            ("Vertical Padding", 1.0, (0.0, 64.0), "px"),
            ("Line height", 0.05, (1.0, 2.0), ""),
            ("Box opacity", 1.0, (0.0, 100.0), "%"),
            ("Box rounding", 1.0, (0.0, 32.0), "px"),
        ]
    );
    assert!(!Field::Delay.editable());
    assert_eq!(Field::FontSize.action_label(true), "Increase font size");
    assert_eq!(
        Field::VerticalPosition.action_label(false),
        "Decrease vertical position"
    );
}

#[test]
fn stepping_snaps_to_the_grid_without_float_noise() {
    let mut prefs = SubtitlePreferences::default();
    prefs.step(Field::LineHeight, -1);
    assert_eq!(prefs.line_height, 1.19);
    prefs.step(Field::LineHeight, 1);
    prefs.step(Field::LineHeight, 1);
    assert_eq!(prefs.line_height, 1.29);
    prefs.step(Field::Delay, 1);
    prefs.step(Field::Delay, 1);
    prefs.step(Field::Delay, -1);
    assert_eq!(prefs.delay, 0.5);
    prefs.step(Field::VerticalPosition, -1);
    assert_eq!(prefs.vertical_position, 10.0);
    prefs.set(Field::Delay, -0.25);
    assert_eq!(prefs.delay, 0.0, "JS Math.round sends -0.5 up");
    prefs.set(Field::Delay, f64::NAN);
    assert_eq!(prefs.delay, 0.0);
    assert_eq!(Field::Delay.text(-1.5), "-1.5s");
    assert_eq!(Field::LineHeight.text(1.2), "1.20");
    assert_eq!(Field::FontSize.text(38.0), "38px");
}

#[test]
fn the_json_round_trips_without_the_session_delay() {
    let mut prefs = SubtitlePreferences {
        text_color: [0xff, 0xe0, 0x66],
        font_weight: FontWeight::Bold,
        language: Some(crate::routes::player::subtitles::Lang::Id),
        source: Some(crate::routes::player::subtitles::Source::Addon(
            "OpenSubtitles".into(),
        )),
        ..Default::default()
    };
    prefs.set(Field::FontSize, 36.0);
    prefs.set(Field::BoxOpacity, 0.0);
    prefs.set(Field::VerticalPosition, 20.0);
    prefs.set(Field::LineHeight, 1.35);
    prefs.set(Field::Delay, 2.5);
    let restored = SubtitlePreferences::from_bytes(&prefs.to_bytes());
    assert_eq!(
        restored,
        SubtitlePreferences {
            delay: 0.0,
            ..prefs.clone()
        }
    );
    assert!(!String::from_utf8_lossy(&prefs.to_bytes()).contains("elay"));
}

#[test]
fn invalid_fields_fall_back_one_by_one() {
    let prefs = SubtitlePreferences::normalise(&json!({
        "textColor": "yellow",
        "textOpacity": "lots",
        "fontSizePx": 500,
        "fontWeight": "heavy",
        "paddingX": null,
        "paddingY": -4,
        "lineHeight": 1.2345,
        "backgroundOpacity": 12.4,
        "borderRadius": 7.5,
        "verticalPosition": 250,
    }));
    assert_eq!(hex(prefs.text_color), "#ffffff");
    assert_eq!(prefs.text_opacity, 100.0);
    assert_eq!(prefs.font_size, 96.0);
    assert_eq!(prefs.font_weight, FontWeight::Regular);
    assert_eq!(prefs.padding_x, 20.0);
    assert_eq!(prefs.padding_y, 0.0);
    assert_eq!(prefs.line_height, 1.23);
    assert_eq!(prefs.box_opacity, 12.0);
    assert_eq!(prefs.box_rounding, 8.0);
    assert_eq!(prefs.vertical_position, 100.0);
    let legacy = SubtitlePreferences::normalise(&json!({"fontWeight": "semibold"}));
    assert_eq!(legacy.font_weight, FontWeight::Medium);
}

#[test]
fn corrupt_stored_bytes_are_the_defaults() {
    for bytes in [&b""[..], b"{", b"\xff\xfe", b"42", b"\"text\"", b"[]"] {
        assert_eq!(
            SubtitlePreferences::from_bytes(bytes),
            SubtitlePreferences::default()
        );
    }
}

#[test]
fn hex_colours_parse_strictly_where_asked() {
    assert_eq!(parse_hex("#FFE066"), Some([255, 224, 102]));
    assert_eq!(parse_hex("#fa0"), Some([255, 170, 0]));
    assert_eq!(parse_hex("#fa08"), Some([255, 170, 0]));
    assert_eq!(parse_hex("#ffe06680"), Some([255, 224, 102]));
    assert_eq!(parse_hex("ffe066"), None);
    assert_eq!(parse_hex("#ffe06"), None);
    assert_eq!(parse_hex("#gggggg"), None);
    assert_eq!(parse_hex("#é1234"), None);
    assert_eq!(parse_strict_hex(" #ffe066 "), Some([255, 224, 102]));
    assert_eq!(parse_strict_hex("#fa0"), None);
    assert_eq!(parse_strict_hex("#ffe06680"), None);
}
