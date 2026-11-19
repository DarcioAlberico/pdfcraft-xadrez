//! Windows only: embed the app icon and version info (VERSIONINFO) into `pdfcraft.exe`, so it
//! shows in Explorer, the taskbar, the Start menu and Alt-Tab. A separate PDF document icon
//! uses resource ID 2; the installer references it as `pdfcraft.exe,-2`. The product version is
//! the chess edition's number (`build_info::EDITION`, plan A9-V), read as
//! `apps/pdfcraft-cli/build.rs` reads it; the file version stays the workspace's.
//!
//! On every other target this does nothing. A missing resource compiler is a warning, so a
//! cross-compile from macOS or Linux still links, unless `PDFCRAFT_REQUIRE_WINRES=1` turns it
//! into an error (for release builds).

#[path = "src/windows_manifest.rs"]
mod windows_manifest;

/// `build_info.rs`, whose `pub const EDITION: &str = "…";` line holds the chess edition's number.
const BUILD_INFO: &str = include_str!("../../crates/engine/src/build_info.rs");

/// The chess edition's number from that line, and its three parts; `None` unless it is three
/// dot-separated numbers that fit a version resource (16 bits each).
fn edition() -> Option<(&'static str, [u16; 3])> {
    let (_, rest) = BUILD_INFO.split_once("pub const EDITION: &str = \"")?;
    let (number, _) = rest.split_once('"')?;
    let mut parts = number.split('.').map(|p| p.parse::<u16>().ok().filter(|_| p.bytes().all(|b| b.is_ascii_digit())));
    let parsed = [parts.next()??, parts.next()??, parts.next()??];
    parts.next().is_none().then_some((number, parsed))
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/app-icon/pdfcraft.ico");
    println!("cargo:rerun-if-changed=../../assets/app-icon/pdfcraft-document.ico");
    println!("cargo:rerun-if-changed=src/windows_manifest.rs");
    println!("cargo:rerun-if-changed=../../crates/engine/src/build_info.rs");
    println!("cargo:rerun-if-env-changed=PDFCRAFT_REQUIRE_WINRES");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let Some((edition, [major, minor, patch])) = edition() else {
        println!("cargo::error=crates/engine/src/build_info.rs has no EDITION of three numbers");
        return;
    };
    let product = (u64::from(major) << 48) | (u64::from(minor) << 32) | (u64::from(patch) << 16);
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../assets/app-icon/pdfcraft.ico")
        .set_icon_with_id("../../assets/app-icon/pdfcraft-document.ico", "2")
        .set_manifest(windows_manifest::WINDOWS_MANIFEST)
        .set("ProductName", "PdfCraft")
        .set("FileDescription", "PdfCraft PDF workbench")
        .set("LegalCopyright", "Copyright (c) the PdfCraft contributors. MIT OR Apache-2.0.")
        .set("OriginalFilename", "pdfcraft.exe")
        .set("InternalName", "pdfcraft")
        .set("ProductVersion", edition)
        .set_version_info(winresource::VersionInfo::PRODUCTVERSION, product);
    if let Err(e) = res.compile() {
        if std::env::var_os("PDFCRAFT_REQUIRE_WINRES").is_some() {
            println!("cargo::error=embedding Windows resources failed: {e}");
            return;
        }
        println!("cargo:warning=pdfcraft.exe built without icon/version resources: {e}");
    }
}
