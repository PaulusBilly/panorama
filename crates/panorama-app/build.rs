use std::env;

fn main() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let rc = "../../packaging/windows/panorama.rc";
    println!("cargo:rerun-if-changed={rc}");
    println!("cargo:rerun-if-changed=../../packaging/windows/panorama.ico");

    let version = env::var("CARGO_PKG_VERSION").expect("cargo sets CARGO_PKG_VERSION");
    let mut parts = version
        .split(['.', '-', '+'])
        .take(3)
        .map(|part| part.parse::<u16>().expect("numeric major.minor.patch"));
    let mut next = || parts.next().expect("major.minor.patch");
    let macros = [
        format!("VERSION_MAJOR={}", next()),
        format!("VERSION_MINOR={}", next()),
        format!("VERSION_PATCH={}", next()),
        format!("VERSION_STRING=\"{version}\""),
    ];
    embed_resource::compile(rc, macros)
        .manifest_optional()
        .unwrap();
}
