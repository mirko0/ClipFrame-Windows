//! Embeds the application manifest (Per-Monitor DPI v2, long paths, Win10+), icon and version
//! info.

// Build scripts talk to Cargo via stdout and should fail loudly.
#![allow(clippy::print_stdout, clippy::expect_used)]

const MANIFEST: &str = "assets/clipframe.manifest";
const ICON: &str = "assets/clipframe.ico";

fn main() {
    println!("cargo:rerun-if-changed={MANIFEST}");
    println!("cargo:rerun-if-changed={ICON}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let manifest = std::fs::read_to_string(MANIFEST).expect("application manifest must exist");
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_manifest(&manifest)
        .set_icon(ICON)
        .set("ProductName", "ClipFrame")
        .set("FileDescription", "ClipFrame screenshot tool");
    resource
        .compile()
        .expect("failed to compile Windows resources");
}
