//! What this build calls itself. A local or patched build sets `PDFCRAFT_BUILD_LABEL` at compile
//! time (`PDFCRAFT_BUILD_LABEL=titlebar cargo build`, or through `packaging/linux/package.sh`), so
//! it can be told from the release of the same version. The version itself stays the release's:
//! the update check compares it with published releases.
//!
//! Every build also has an id ([`BUILD`], plan A9 E2): the short commit and the date, which the
//! release script sets through `PDFCRAFT_BUILD_ID` (the crate's `build.rs` keeps it plain), else
//! `dev`. The fork's user keeps builds side by side (`dist/app`, `dist/app/anterior-…`): the id is
//! what tells two builds of one release apart in About, `--version` and `chess_status`.
//!
//! The chess fork numbers its own releases ([`EDITION`], plan A9-V), and [`version`] shows that
//! number first: `1.0.0, build 1eda302 2026-11-19 (PdfCraft 0.9.0)`.

/// The release number of the chess edition, the fork: shown first in About, `--version`, the log
/// and `chess_status`, and the product version of the Windows executables (whose `build.rs` read it
/// from this line).
///
/// The coordinator raises it at each release that goes into `dist/app`: the minor for a release
/// with new features (`1.1.0`), the patch for fixes only (`1.0.1`). [`VERSION`] stays upstream's,
/// for the update check.
pub const EDITION: &str = "1.0.0";

/// The release version from the workspace `Cargo.toml`: upstream PdfCraft's, which the update check
/// compares with published releases.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The compile-time build label, if any.
pub const LABEL: Option<&str> = option_env!("PDFCRAFT_BUILD_LABEL");

/// The build's id: the short commit and the date the release script built, else `dev`.
pub const BUILD: &str = env!("PDFCRAFT_BUILD");

/// The version as shown to people: the edition, the build, then upstream's version and the label,
/// if any: `1.0.0, build dev (PdfCraft 0.9.0)`, or `1.0.0, build 5f757af 2026-11-11 (PdfCraft
/// 0.9.0, titlebar)` for a labelled build of the release script.
pub fn version() -> String {
    describe(EDITION, VERSION, LABEL, BUILD)
}

fn describe(edition: &str, version: &str, label: Option<&str>, build: &str) -> String {
    match label.map(str::trim).filter(|l| !l.is_empty()) {
        Some(label) => format!("{edition}, build {build} (PdfCraft {version}, {label})"),
        None => format!("{edition}, build {build} (PdfCraft {version})"),
    }
}

#[cfg(test)]
mod tests {
    use super::{BUILD, EDITION, VERSION, describe};

    /// Plan A9-V: a build of the release script says the edition, then the build, then upstream's
    /// version.
    #[test]
    fn a_release_build_shows_the_edition_then_the_build_then_upstreams_version() {
        assert_eq!(describe("1.0.0", "0.9.0", None, "1eda302 2026-11-19"), "1.0.0, build 1eda302 2026-11-19 (PdfCraft 0.9.0)");
        assert_eq!(describe("1.2.3", "0.9.0", None, "5f757af 2026-11-11"), "1.2.3, build 5f757af 2026-11-11 (PdfCraft 0.9.0)");
    }

    #[test]
    fn a_label_follows_upstreams_version_and_a_blank_one_is_ignored() {
        assert_eq!(describe("1.0.0", "0.9.0", Some("titlebar"), "1eda302 2026-11-19"), "1.0.0, build 1eda302 2026-11-19 (PdfCraft 0.9.0, titlebar)");
        assert_eq!(describe("1.0.0", "0.9.0", Some(" titlebar "), "dev"), "1.0.0, build dev (PdfCraft 0.9.0, titlebar)");
        for blank in ["", "  "] {
            assert_eq!(describe("1.0.0", "0.9.0", Some(blank), "dev"), "1.0.0, build dev (PdfCraft 0.9.0)", "{blank:?}");
        }
    }

    /// Without the release script's variable the build is `dev`, still after the edition.
    #[test]
    fn a_build_without_the_release_variable_is_dev() {
        assert_eq!(describe("1.0.0", "0.9.0", None, "dev"), "1.0.0, build dev (PdfCraft 0.9.0)");
    }

    /// The edition is three dot-separated numbers, each one a Windows version resource can hold
    /// (16 bits; the apps' `build.rs` make it the executables' product version).
    #[test]
    fn the_edition_is_three_numbers() {
        let parts: Vec<&str> = EDITION.split('.').collect();
        assert_eq!(parts.len(), 3, "{EDITION}");
        for part in parts {
            assert!(!part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()), "{EDITION}");
            assert!(part.parse::<u16>().is_ok(), "{EDITION}");
            assert!(part == "0" || !part.starts_with('0'), "no leading zero: {EDITION}");
        }
    }

    /// Plan A9, E2: every build says which it is; without the release script's variable, `dev`.
    #[test]
    fn every_build_has_an_id() {
        assert!(!BUILD.is_empty() && BUILD.len() <= 48, "{BUILD}");
        assert!(BUILD.chars().all(|c| c.is_ascii_alphanumeric() || " .-_:+".contains(c)), "{BUILD}");
        if option_env!("PDFCRAFT_BUILD_ID").is_none_or(|v| v.trim().is_empty()) {
            assert_eq!(BUILD, "dev");
        }
        let shown = super::version();
        assert!(shown.starts_with(&format!("{EDITION}, build {BUILD} (PdfCraft {VERSION}")), "{shown}");
        assert!(shown.ends_with(')'), "{shown}");
    }
}
