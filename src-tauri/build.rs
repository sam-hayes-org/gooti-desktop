use std::path::PathBuf;

fn main() {
    // tauri-build would put the Windows manifest only into the app. The test programs need it as
    // well: without Common Controls v6 they stop at start on Windows (STATUS_ENTRYPOINT_NOT_FOUND).
    // So the linker embeds it into every binary instead.
    let attributes = tauri_build::Attributes::new()
        .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
    tauri_build::try_build(attributes).expect("tauri-build failed");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    // MSVC linker flags (link.exe, lld-link). The GNU build (check:windows under Wine) goes without.
    if target_os == "windows" && target_env == "msvc" {
        let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
}
