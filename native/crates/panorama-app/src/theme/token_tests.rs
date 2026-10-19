use super::{
    Theme,
    color::{hex, oklch},
    typography::*,
};
use crate::motion::{DURATION_DELIBERATE, DURATION_FAST, DURATION_STANDARD, EASE_EDITORIAL};
use std::{collections::BTreeMap, fs, path::Path};

fn block<'a>(css: &'a str, selector: &str) -> &'a str {
    let start = css
        .find(selector)
        .unwrap_or_else(|| panic!("Missing {selector}"));
    let opening = start + css[start..].find('{').unwrap();
    let mut depth = 1;
    for (offset, byte) in css.as_bytes()[opening + 1..].iter().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        if depth == 0 {
            return &css[opening + 1..opening + 1 + offset];
        }
    }
    panic!("Unclosed CSS block {selector}");
}

fn declarations(block: &str) -> BTreeMap<&str, &str> {
    block
        .split(';')
        .filter_map(|declaration| declaration.trim().split_once(':'))
        .map(|(key, value)| (key.trim(), value.trim()))
        .collect()
}

fn numbers(value: &str) -> Vec<f64> {
    value
        .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
        .filter(|s| !s.is_empty() && *s != "-")
        .map(|s| s.parse().unwrap())
        .collect()
}

fn color(value: &str) -> gpui::Rgba {
    if let Some(hexadecimal) = value.strip_prefix('#') {
        return hex(u32::from_str_radix(hexadecimal, 16).unwrap());
    }
    assert!(value.starts_with("oklch("), "{value}");
    let values = numbers(value);
    oklch(
        values[0] as f32,
        values[1] as f32,
        values[2] as f32,
        values.get(3).copied().unwrap_or(1.0) as f32,
    )
}

fn css() -> String {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    match fs::read_to_string(manifest.join("../../../app/globals.css")) {
        Ok(css) => css,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::read_to_string(manifest.join("tests/fixtures/globals.css")).unwrap()
        }
        Err(error) => panic!("Cannot read repository tokens: {error}"),
    }
}

#[test]
fn color_tokens_match_css_and_fixture() {
    let css = css();
    let light = declarations(block(&css, ":root {"));
    let dark = declarations(block(&css, "[data-theme=\"dark\"] {"));
    assert_eq!(
        light
            .keys()
            .filter(|key| key.starts_with("--theme-"))
            .count(),
        25
    );
    for (theme, overrides) in [(Theme::light(), BTreeMap::new()), (Theme::dark(), dark)] {
        let mut expected = light.clone();
        expected.extend(overrides);
        assert_eq!(
            expected
                .keys()
                .filter(|key| key.starts_with("--theme-"))
                .count(),
            theme.colors().len()
        );
        for (role, actual) in theme.colors() {
            let css_value = color(expected[format!("--theme-{role}").as_str()]);
            for (actual, expected) in [actual.r, actual.g, actual.b, actual.a].into_iter().zip([
                css_value.r,
                css_value.g,
                css_value.b,
                css_value.a,
            ]) {
                assert!((actual - expected).abs() < 0.000001, "{role}");
            }
        }
    }
    let fixture = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/globals.css"),
    )
    .unwrap();
    for selector in [":root {", "[data-theme=\"dark\"] {", "@theme {"] {
        assert_eq!(
            declarations(block(&css, selector)),
            declarations(block(&fixture, selector))
        );
    }
}

#[test]
fn type_motion_and_spacing_tokens_match_css() {
    let css = css();
    let tokens = declarations(block(&css, "@theme {"));
    for (name, style) in [
        ("xs", CAPTION),
        ("sm", LABEL),
        ("md", BODY),
        ("lg", HEADING),
        ("xl", TITLE),
        ("2xl", TEXT_2XL),
        ("3xl", TEXT_3XL),
    ] {
        let size = tokens[format!("--text-{name}").as_str()]
            .strip_suffix("rem")
            .unwrap()
            .parse::<f32>()
            .unwrap()
            * 16.0;
        assert_eq!(style.size, size, "{name}");
        assert_eq!(
            style.line_height,
            tokens[format!("--text-{name}--line-height").as_str()]
                .parse::<f32>()
                .unwrap(),
            "{name}"
        );
    }
    for (name, scale, style) in [
        ("caption", Some("xs"), CAPTION),
        ("label", Some("sm"), LABEL),
        ("body", Some("md"), BODY),
        ("heading", Some("lg"), HEADING),
        ("title", Some("xl"), TITLE),
        ("display", None, TEXT_3XL),
    ] {
        let utility = declarations(block(&css, &format!("@utility type-{name} ")));
        // The scale tokens' pixel sizes are checked above; display's clamp is checked below.
        if let Some(scale) = scale {
            assert_eq!(
                utility["font-size"],
                format!("var(--text-{scale})"),
                "{name}"
            );
        }
        assert_eq!(utility["font-weight"].parse::<f32>().unwrap(), style.weight);
        assert_eq!(
            utility["line-height"].parse::<f32>().unwrap(),
            style.line_height
        );
    }
    assert_eq!(
        declarations(block(&css, "@utility type-display "))["font-size"],
        "clamp(var(--text-2xl), 5vw, var(--text-3xl))"
    );
    for (name, duration) in [
        ("fast", DURATION_FAST),
        ("standard", DURATION_STANDARD),
        ("deliberate", DURATION_DELIBERATE),
    ] {
        assert_eq!(
            tokens[format!("--duration-{name}").as_str()],
            format!("{}ms", duration.as_millis())
        );
    }
    assert_eq!(numbers(tokens["--ease-editorial"]), EASE_EDITORIAL);
    assert_eq!(
        tokens["--animate-shimmer"],
        "shimmer 1.6s ease-in-out infinite"
    );
    assert_eq!(super::SHIMMER.duration.as_secs_f32(), 1.6);
    assert_eq!(super::SHIMMER.easing, [0.42, 0.0, 0.58, 1.0]);
    assert_eq!(
        tokens["--animate-shimmer"].split_whitespace().last(),
        Some(if super::SHIMMER.infinite {
            "infinite"
        } else {
            "once"
        })
    );
    let root = declarations(block(&css, ":root {"));
    assert_eq!(
        root["--page-gutter"],
        format!("{}px", super::page_gutter(1280.0))
    );
    assert_eq!(
        root["--dialog-inset"],
        format!("{}px", super::dialog_inset(1280.0))
    );
    for (query, width) in [
        ("@media (max-width: 1100px)", 1100.0),
        ("@media (max-width: 700px)", 700.0),
    ] {
        let media = block(&css, query);
        let root = declarations(block(media, ":root"));
        assert_eq!(
            root["--page-gutter"],
            format!("{}px", super::page_gutter(width))
        );
        if let Some(inset) = root.get("--dialog-inset") {
            assert_eq!(*inset, format!("{}px", super::dialog_inset(width)));
        }
    }
    assert_eq!(
        declarations(block(&css, ".content-container {"))["width"],
        "min(calc(100% - (clamp(20px, 4.27vw, 70px) * 2)), 93.75rem)"
    );
    for (width, max) in [(1640, 70.25), (1220, 46.75), (850, 42.125), (720, 20.9375)] {
        let media = block(&css, &format!("@media (width < {width}px)"));
        let limit = declarations(block(media, ".content-container"))["max-width"]
            .strip_suffix("rem")
            .unwrap()
            .parse::<f32>()
            .unwrap();
        assert_eq!(limit, max);
        assert!(super::content_width(width as f32 - 1.0) <= limit * 16.0);
    }
}
