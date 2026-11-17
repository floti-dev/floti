//! Floti has one version, the desktop's: `packages/desktop/package.json`
//! names it, and every build of `floti` carries it as `FLOTI_VERSION`. The
//! crate's own version in its manifest is not Floti's and is never shown.
//!
//! A Windows `floti.exe` also carries the desktop app's icon
//! (`packages/desktop/build/icon.ico`), compiled in from `floti.rc`.

use std::path::Path;

fn main() {
    carry_version();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_windows_resources();
    }
}

/// Compiles `floti.rc` and links it into the `floti` binary only: the test
/// tools built beside it carry no icon. The target decides, not the OS doing
/// the build, so nothing of this runs for macOS or Linux.
///
/// embed-resource, a maintained crate, finds and runs the toolchain's own
/// resource compiler (rc.exe from the Windows SDK on MSVC) and hands its
/// output to the linker. It takes a plain `.rc`, so the resources stay
/// readable in one file, and links them into one named binary, where
/// winresource writes the script itself and links into every binary. A
/// resource compiler it cannot find fails the build, so no release ships a
/// floti.exe without its icon.
fn embed_windows_resources() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let script = crate_dir.join("floti.rc");
    let icon = crate_dir
        .join("..")
        .join("..")
        .join("packages")
        .join("desktop")
        .join("build")
        .join("icon.ico");
    println!("cargo:rerun-if-changed={}", script.display());
    println!("cargo:rerun-if-changed={}", icon.display());
    embed_resource::compile_for(&script, ["floti"], embed_resource::NONE)
        .manifest_required()
        .unwrap_or_else(|err| {
            panic!(
                "floti.exe's icon could not be compiled from {}: {err}",
                script.display()
            )
        });
}

/// Sets `FLOTI_VERSION` from the desktop's `package.json`.
fn carry_version() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("packages")
        .join("desktop")
        .join("package.json");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let text = std::fs::read_to_string(&manifest).unwrap_or_else(|err| {
        panic!(
            "floti takes its version from {}, which could not be read: {err}",
            manifest.display()
        )
    });
    let package: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|err| panic!("{} is not JSON: {err}", manifest.display()));
    let version = package["version"]
        .as_str()
        .filter(|version| {
            !version.is_empty()
                && version
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
        })
        .unwrap_or_else(|| panic!("{} names no version floti can carry", manifest.display()));
    println!("cargo:rustc-env=FLOTI_VERSION={version}");
}
