//! What this build calls itself. A local or patched build sets `PDFCRAFT_BUILD_LABEL` at compile
//! time (`PDFCRAFT_BUILD_LABEL=titlebar cargo build`, or through `packaging/linux/package.sh`), so
//! it can be told from the release of the same version. The version itself stays the release's:
//! the update check compares it with published releases.
//!
//! Every build also has an id ([`BUILD`], plan A9 E2): the short commit and the date, which the
//! release script sets through `PDFCRAFT_BUILD_ID` (the crate's `build.rs` keeps it plain), else
//! `dev`. The fork's user keeps builds side by side (`dist/app`, `dist/app/anterior-…`), all of
//! one version: the id is what tells them apart in About, `--version` and `chess_status`.

/// The release version from the workspace `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The compile-time build label, if any.
pub const LABEL: Option<&str> = option_env!("PDFCRAFT_BUILD_LABEL");

/// The build's id: the short commit and the date the release script built, else `dev`.
pub const BUILD: &str = env!("PDFCRAFT_BUILD");

/// The version as shown to people: `0.7.1, build dev`, or `0.7.1 (titlebar), build 5f757af
/// 2026-11-11` for a labelled build of the release script.
pub fn version() -> String {
    describe(VERSION, LABEL, BUILD)
}

fn describe(version: &str, label: Option<&str>, build: &str) -> String {
    let version = match label.map(str::trim).filter(|l| !l.is_empty()) {
        Some(label) => format!("{version} ({label})"),
        None => version.to_owned(),
    };
    format!("{version}, build {build}")
}

#[cfg(test)]
mod tests {
    use super::{BUILD, describe};

    #[test]
    fn a_label_follows_the_version_and_a_blank_one_is_ignored() {
        assert_eq!(describe("0.7.1", None, "dev"), "0.7.1, build dev");
        assert_eq!(describe("0.7.1", Some(""), "dev"), "0.7.1, build dev");
        assert_eq!(describe("0.7.1", Some("  "), "dev"), "0.7.1, build dev");
        assert_eq!(describe("0.7.1", Some("titlebar"), "5f757af 2026-11-11"), "0.7.1 (titlebar), build 5f757af 2026-11-11");
    }

    /// Plan A9, E2: every build says which it is; without the release script's variable, `dev`.
    #[test]
    fn every_build_has_an_id() {
        assert!(!BUILD.is_empty() && BUILD.len() <= 48, "{BUILD}");
        assert!(BUILD.chars().all(|c| c.is_ascii_alphanumeric() || " .-_:+".contains(c)), "{BUILD}");
        if option_env!("PDFCRAFT_BUILD_ID").is_none_or(|v| v.trim().is_empty()) {
            assert_eq!(BUILD, "dev");
        }
        assert!(super::version().ends_with(&format!(", build {BUILD}")));
    }
}
