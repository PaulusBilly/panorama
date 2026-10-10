use super::*;
use serde_json::json;

#[test]
fn addon_url_accepts_https_stremio_case_trim_and_opaque_configuration() {
    for (input, expected) in [
        (
            " https://addon.test/config%40token/manifest.json?key=SECRET ",
            "https://addon.test/config%40token/manifest.json?key=SECRET",
        ),
        (
            "stremio://addon.test/manifest.json",
            "https://addon.test/manifest.json",
        ),
        (
            "STREMIO://addon.test/settings%2Fsecret/manifest.json",
            "https://addon.test/settings%2Fsecret/manifest.json",
        ),
        (
            "HTTPS://addon.test/manifest.json",
            "HTTPS://addon.test/manifest.json",
        ),
        (
            "https://8.8.8.8/manifest.json",
            "https://8.8.8.8/manifest.json",
        ),
        (
            "https://addon.test/%252e%252e/manifest.json",
            "https://addon.test/%252e%252e/manifest.json",
        ),
    ] {
        let url = AddonUrl::parse(input).unwrap();
        assert_eq!(url.as_str(), expected);
        assert_eq!(format!("{url:?}"), "AddonUrl { .. }");
    }
}

#[test]
fn addon_url_rejects_schemes_credentials_fragments_whitespace_and_local_literals() {
    for input in [
        "http://addon.test/manifest.json",
        "file:///manifest.json",
        "data:manifest.json",
        "javascript:manifest.json",
        "/manifest.json",
        "addon.test/manifest.json",
        "https://user@addon.test/manifest.json",
        "https://:secret@addon.test/manifest.json",
        "https://@addon.test/manifest.json",
        "https://addon%40evil.test/manifest.json",
        "https://addon.test/manifest.json#",
        "https://addon.test/manifest.json#secret",
        "https://addon.test/manifest.json/",
        "https://addon.test/other.json",
        "https://addon.test/a b/manifest.json",
        "https://addon.test/a\nb/manifest.json",
        "https://addon.test/a\tb/manifest.json",
        "https://addon.test/a\0b/manifest.json",
        "https://127.0.0.1/manifest.json",
        "https://127.2.3.4/manifest.json",
        "https://2130706433/manifest.json",
        "https://0x7f000001/manifest.json",
        "https://169.254.1.2/manifest.json",
        "https://[::1]/manifest.json",
        "https://[fe80::1]/manifest.json",
        "https://[::ffff:127.0.0.1]/manifest.json",
        "https://0.0.0.0/manifest.json",
        "https://[::]/manifest.json",
        "https://addon.test/%2e%2e/manifest.json",
        "https://addon.test/.%2E/manifest.json",
        "https://addon.test/../manifest.json",
        "https://addon.test\\evil/manifest.json",
    ] {
        assert_eq!(
            AddonUrl::parse(input).unwrap_err(),
            InstallError::InvalidUrl,
            "{input}"
        );
    }
}

#[test]
fn addon_url_length_boundary_is_2048_characters() {
    let prefix = "https://addon.test/";
    let suffix = "/manifest.json";
    let url = format!(
        "{prefix}{}{suffix}",
        "x".repeat(2048 - prefix.len() - suffix.len())
    );
    assert!(AddonUrl::parse(&url).is_ok());
    assert_eq!(
        AddonUrl::parse(&url.replace("/manifest", "x/manifest")).unwrap_err(),
        InstallError::InvalidUrl
    );
}

fn manifest() -> serde_json::Value {
    json!({"id":"example", "version":"1.2.3", "name":" Example ",
        "types":["movie"], "resources":["meta"], "catalogs":[]})
}

#[test]
fn manifest_shape_limits_and_sanitized_summary() {
    let mut value = manifest();
    value["description"] = json!("雪".repeat(2001));
    value["logo"] = json!("http://addon.test/SECRET");
    value["background"] = json!("https://addon.test/SECRET");
    value["behaviorHints"] = json!({"configurable":true, "configurationRequired":true});
    let parsed = parse_manifest(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(parsed.name, "Example");
    assert_eq!(parsed.description.as_ref().unwrap().chars().count(), 2000);
    assert!(parsed.logo.is_none());
    assert!(parsed.background.is_some());
    let preview = AddonPreview::new(&parsed, false, None, PreviewToken([42; 32]));
    assert!(preview.configurable && preview.configuration_required);
    assert!(!format!("{preview:?} {:?}", preview.token).contains("SECRET"));
    for (field, bad) in [
        ("id", json!("")),
        ("id", json!("x".repeat(201))),
        ("id", json!("white space")),
        ("id", json!("control\n")),
        ("version", json!("not-semver")),
        ("name", json!("x".repeat(201))),
        ("types", json!([])),
        ("resources", json!([])),
        (
            "catalogs",
            json!(
                (0..201)
                    .map(|i| json!({"id":i.to_string(),"type":"movie"}))
                    .collect::<Vec<_>>()
            ),
        ),
    ] {
        let mut value = manifest();
        value[field] = bad;
        assert_eq!(
            parse_manifest(&serde_json::to_vec(&value).unwrap()).unwrap_err(),
            InstallError::InvalidManifest
        );
    }
    for field in ["id", "version", "name", "resources", "types"] {
        let mut value = manifest();
        value.as_object_mut().unwrap().remove(field);
        assert_eq!(
            parse_manifest(&serde_json::to_vec(&value).unwrap()).unwrap_err(),
            InstallError::InvalidManifest
        );
    }
    assert_eq!(
        parse_manifest(b"not JSON SECRET").unwrap_err(),
        InstallError::InvalidManifest
    );
}

#[test]
fn all_management_errors_have_no_payloads_in_debug_or_display() {
    for error in [
        InstallError::InvalidUrl,
        InstallError::InvalidManifest,
        InstallError::TooLarge,
        InstallError::Network,
        InstallError::Timeout,
        InstallError::SignedOut,
        InstallError::Protected,
        InstallError::ImpersonatesProtected,
        InstallError::InvalidOrder,
        InstallError::InvalidSelection,
        InstallError::InvalidToken,
        InstallError::ConfigurationRequired,
        InstallError::Core,
        InstallError::ApiPush,
        InstallError::Storage,
    ] {
        let text = format!("{error:?} {error}");
        for secret in ["https://", "SECRET", "manifest.json", "authKey", "?key="] {
            assert!(!text.contains(secret));
        }
    }
}
