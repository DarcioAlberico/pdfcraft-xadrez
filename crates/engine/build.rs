//! The build's id (plan A9, E2): the release script sets `PDFCRAFT_BUILD_ID` to the short commit
//! and the date it builds (`5f757af 2026-11-11`), and `build_info::BUILD` carries it into About,
//! `--version` and `chess_status`, so two builds kept side by side can be told apart. Letters,
//! digits, spaces and `.-_:+` are kept, at most 48 of them; without the variable, or with nothing
//! left of it, the build is `dev`.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-env-changed=PDFCRAFT_BUILD_ID");
    let given = std::env::var("PDFCRAFT_BUILD_ID").unwrap_or_default();
    let kept: String = given.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '-' | '_' | ':' | '+')).take(48).collect();
    let id = kept.split_whitespace().collect::<Vec<_>>().join(" ");
    println!("cargo::rustc-env=PDFCRAFT_BUILD={}", if id.is_empty() { "dev" } else { id.as_str() });
}
