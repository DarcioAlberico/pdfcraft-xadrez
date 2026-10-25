//! The piece set the user chose (plan HC2): one for the boards, one for diagrams and exports.
//!
//! A [`PieceChoice`] is kept in the preferences as a short text ([`PieceChoice::token`]) and
//! turned into pieces by [`resolve`]. [`candidates`] lists what can be chosen on this machine:
//! the shipped sets, the diagram fonts in the `fonts` folder beside the executable, the `pieces`
//! folder there and each folder of piece images inside it, an installed Chess Merida and the plain
//! shapes. `$PDFCRAFT_CHESS_PIECES` still takes precedence over any choice, as it always did.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use pdfcraft_chess::draw::PieceSet;

use super::{PieceError, PieceSource, bundled, from_bundled, from_folder, from_font_file, is_piece_folder, missing_pieces};

/// The variable that names the pieces outside the preferences.
const ENV: &str = "PDFCRAFT_CHESS_PIECES";

/// Most folders of piece images looked at inside the `pieces` folder.
const MAX_FOLDERS: usize = 64;

/// Most fonts tried in the `fonts` folder beside the executable.
const MAX_FONTS: usize = 64;

/// What a role's pieces are.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum PieceChoice {
    /// What PdfCraft finds on its own ([`super::find`] or [`super::find_for_board`]).
    #[default]
    Auto,
    /// The plain shapes drawn in code.
    Geometric,
    /// A set PdfCraft ships, by its id.
    Bundled(String),
    /// A chess diagram font file.
    Font(PathBuf),
    /// A folder of twelve piece images.
    Folder(PathBuf),
}

impl PieceChoice {
    /// The choice as the preferences keep it: `auto`, `geometric`, `bundled:<id>`,
    /// `font:<path>` or `folder:<path>`.
    pub fn token(&self) -> String {
        match self {
            PieceChoice::Auto => "auto".to_owned(),
            PieceChoice::Geometric => "geometric".to_owned(),
            PieceChoice::Bundled(id) => format!("bundled:{id}"),
            PieceChoice::Font(path) => format!("font:{}", path.to_string_lossy()),
            PieceChoice::Folder(path) => format!("folder:{}", path.to_string_lossy()),
        }
    }

    /// The choice a [`PieceChoice::token`] names; `None` for anything else.
    pub fn from_token(token: &str) -> Option<PieceChoice> {
        match token {
            "auto" => return Some(PieceChoice::Auto),
            "geometric" => return Some(PieceChoice::Geometric),
            _ => {}
        }
        let (kind, rest) = token.split_once(':')?;
        if rest.is_empty() {
            return None;
        }
        match kind {
            "bundled" => Some(PieceChoice::Bundled(rest.to_owned())),
            "font" => Some(PieceChoice::Font(PathBuf::from(rest))),
            "folder" => Some(PieceChoice::Folder(PathBuf::from(rest))),
            _ => None,
        }
    }
}

/// Where a set is drawn: the two roles have a choice each.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    /// The analysis board, the study board and the editor's palette.
    Board,
    /// Diagrams redrawn on the page and in every export.
    Diagrams,
}

/// Whether `$PDFCRAFT_CHESS_PIECES` names the pieces, so that no choice applies.
pub fn environment_overrides() -> bool {
    std::env::var_os(ENV).is_some()
}

/// The pieces `choice` names for `role`. A chosen set that cannot be read is reported and plain
/// shapes drawn instead, as [`super::find`] does with the variable's.
pub fn resolve(choice: &PieceChoice, role: Role) -> (PieceSet, PieceSource, Option<PieceError>) {
    resolve_with(choice, role, std::env::var_os(ENV))
}

/// The pieces `choice` names for `role`, whether or not the variable is set: what a picker
/// shows for each set.
pub fn load(choice: &PieceChoice, role: Role) -> (PieceSet, PieceSource, Option<PieceError>) {
    resolve_with(choice, role, None)
}

fn resolve_with(choice: &PieceChoice, role: Role, env: Option<OsString>) -> (PieceSet, PieceSource, Option<PieceError>) {
    let found = || match role {
        Role::Board => super::find_for_board(),
        Role::Diagrams => super::find(),
    };
    if env.is_some() {
        return found();
    }
    let plain = |e| (PieceSet::geometric(), PieceSource::Geometric, Some(e));
    match choice {
        PieceChoice::Auto => found(),
        PieceChoice::Geometric => (PieceSet::geometric(), PieceSource::Geometric, None),
        PieceChoice::Bundled(id) => bundled::by_id(id).map_or_else(|| plain(PieceError::UnknownSet(id.clone())), from_bundled),
        PieceChoice::Font(path) => match from_font_file(path) {
            Ok(set) => {
                let family = set.name.clone();
                (set, PieceSource::Font { path: Some(path.clone()), family }, None)
            }
            Err(e) => plain(e),
        },
        PieceChoice::Folder(dir) => from_folder(dir.clone()),
    }
}

/// What kind of set a [`Candidate`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CandidateKind {
    Auto,
    Shipped,
    Font,
    Folder,
    Geometric,
}

/// A set that can be chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub choice: PieceChoice,
    pub kind: CandidateKind,
    /// The set's name (a shipped set's, a font's family, a folder's name).
    pub name: String,
    /// A shipped set's author and SPDX licence.
    pub author: Option<&'static str>,
    pub licence: Option<&'static str>,
    /// A font's or folder's path.
    pub path: Option<PathBuf>,
    /// The pieces a folder lacks (`bQ`…): such a folder is listed so the user sees why it
    /// cannot be chosen.
    pub missing: Vec<String>,
}

/// Every set that can be chosen on this machine, in the order to offer them: automatic, the
/// shipped sets, diagram fonts beside the executable and an installed Chess Merida, piece folders
/// beside the executable, the plain shapes. Fonts that cannot be read are left out; a folder
/// missing pieces is listed with [`Candidate::missing`].
pub fn candidates() -> Vec<Candidate> {
    let fonts = crate::text_font::beside_executable();
    let pieces = super::beside_executable();
    let merida: Vec<PathBuf> = super::installed_fonts()
        .into_iter()
        .filter(|p| p.file_stem().and_then(|s| s.to_str()).map(pdfcraft_chess::catalog::normalize_font_name).unwrap_or_default().contains("merida"))
        .collect();
    candidates_in(fonts.as_deref(), pieces.as_deref(), &merida)
}

fn candidates_in(fonts_dir: Option<&Path>, pieces_dir: Option<&Path>, installed: &[PathBuf]) -> Vec<Candidate> {
    let plain =
        |choice, kind, name: &str| Candidate { choice, kind, name: name.to_owned(), author: None, licence: None, path: None, missing: Vec::new() };
    let mut out = vec![plain(PieceChoice::Auto, CandidateKind::Auto, "")];
    out.extend(bundled::SETS.iter().map(|s| Candidate {
        choice: PieceChoice::Bundled(s.id.to_owned()),
        kind: CandidateKind::Shipped,
        name: s.name.to_owned(),
        author: Some(s.author),
        licence: Some(s.licence),
        path: None,
        missing: Vec::new(),
    }));
    let mut fonts = fonts_dir.map(|d| files(d, MAX_FONTS, |p| has_extension(p, &["ttf", "otf"]))).unwrap_or_default();
    fonts.extend(installed.iter().cloned());
    for path in fonts {
        if out.iter().any(|c| c.path.as_ref() == Some(&path)) {
            continue;
        }
        if let Ok(set) = from_font_file(&path) {
            out.push(Candidate { name: set.name, path: Some(path.clone()), ..plain(PieceChoice::Font(path), CandidateKind::Font, "") });
        }
    }
    if let Some(dir) = pieces_dir {
        let mut folders = vec![dir.to_path_buf()];
        folders.extend(files(dir, MAX_FOLDERS, Path::is_dir));
        for folder in folders.into_iter().filter(|f| is_piece_folder(f)) {
            let name = folder.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_owned();
            let missing = missing_pieces(&folder);
            out.push(Candidate { name, path: Some(folder.clone()), missing, ..plain(PieceChoice::Folder(folder), CandidateKind::Folder, "") });
        }
    }
    out.push(plain(PieceChoice::Geometric, CandidateKind::Geometric, ""));
    out
}

/// Up to `max` entries of `dir` that `keep` accepts, by name.
fn files(dir: &Path, max: usize, keep: impl Fn(&Path) -> bool) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut found: Vec<PathBuf> = entries.flatten().take(10_000).map(|e| e.path()).filter(|p| keep(p)).collect();
    found.sort();
    found.truncate(max);
    found
}

fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| extensions.contains(&e.to_ascii_lowercase().as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of twelve SVG pieces (a square each), named `name` inside `parent`.
    fn piece_folder(parent: &Path, name: &str, count: usize) -> PathBuf {
        let dir = parent.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><path d="M2 2 H8 V8 H2 Z" fill="black"/></svg>"#;
        for stem in ["wK", "wQ", "wR", "wB", "wN", "wP", "bK", "bQ", "bR", "bB", "bN", "bP"].iter().take(count) {
            std::fs::write(dir.join(format!("{stem}.svg")), svg).unwrap();
        }
        dir
    }

    fn scratch(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pdfcraft-piece-choice-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn tokens_read_back_as_the_choice_they_came_from() {
        for choice in [
            PieceChoice::Auto,
            PieceChoice::Geometric,
            PieceChoice::Bundled("chessnut".into()),
            PieceChoice::Font(PathBuf::from("C:/Fonts/Chess Merida.ttf")),
            PieceChoice::Folder(PathBuf::from("/home/u/pieces:odd")),
        ] {
            assert_eq!(PieceChoice::from_token(&choice.token()), Some(choice.clone()), "{choice:?}");
        }
        for bad in ["", "bundled:", "font:", "other:x", "Auto", "folder"] {
            assert_eq!(PieceChoice::from_token(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_choice_is_resolved_unless_the_variable_names_the_pieces() {
        let dir = scratch("resolve");
        let (set, source, error) = resolve_with(&PieceChoice::Bundled("firi".into()), Role::Board, None);
        assert_eq!((set.name.as_str(), &source, error.is_none()), ("Firi", &PieceSource::Bundled { id: "firi" }, true));
        let (_, source, error) = resolve_with(&PieceChoice::Bundled("cburnett".into()), Role::Diagrams, None);
        assert_eq!(source, PieceSource::Geometric);
        assert!(matches!(error, Some(PieceError::UnknownSet(ref id)) if id == "cburnett"), "{error:?}");
        let folder = piece_folder(&dir, "mine", 12);
        let (_, source, error) = resolve_with(&PieceChoice::Folder(folder.clone()), Role::Diagrams, None);
        assert_eq!((source, error.is_none()), (PieceSource::Images { dir: folder.clone() }, true));
        let (_, source, error) = resolve_with(&PieceChoice::Font(dir.join("gone.ttf")), Role::Diagrams, None);
        assert_eq!((source, error.is_some()), (PieceSource::Geometric, true));
        assert_eq!(resolve_with(&PieceChoice::Geometric, Role::Board, None).1, PieceSource::Geometric);
        // The variable takes precedence over the choice: what it names is found instead.
        let (_, source, _) = resolve_with(&PieceChoice::Bundled("firi".into()), Role::Board, Some(OsString::from("x")));
        assert_ne!(source, PieceSource::Bundled { id: "firi" });
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn candidates_list_shipped_sets_readable_fonts_and_folders_with_what_they_lack() {
        let dir = scratch("candidates");
        let fonts = dir.join("fonts");
        std::fs::create_dir_all(&fonts).unwrap();
        // A text font is no diagram font, and a stray file is no font.
        std::fs::write(fonts.join("Inter.ttf"), include_bytes!("../../../../assets/fonts/Inter-Regular.ttf")).unwrap();
        std::fs::write(fonts.join("notes.txt"), "x").unwrap();
        let pieces = dir.join("pieces");
        piece_folder(&pieces, "wood", 12);
        piece_folder(&pieces, "half", 11);
        let list = candidates_in(Some(&fonts), Some(&pieces), &[dir.join("missing.ttf")]);
        let kinds: Vec<CandidateKind> = list.iter().map(|c| c.kind).collect();
        assert_eq!(kinds.first(), Some(&CandidateKind::Auto));
        assert_eq!(kinds.last(), Some(&CandidateKind::Geometric));
        let shipped: Vec<&str> = list.iter().filter(|c| c.kind == CandidateKind::Shipped).map(|c| c.name.as_str()).collect();
        assert_eq!(shipped.len(), bundled::SETS.len());
        assert!(list.iter().filter(|c| c.kind == CandidateKind::Shipped).all(|c| c.author.is_some() && c.licence.is_some()));
        let folders: Vec<(&str, &[String])> =
            list.iter().filter(|c| c.kind == CandidateKind::Folder).map(|c| (c.name.as_str(), c.missing.as_slice())).collect();
        assert_eq!(folders, [("half", &["bP".to_owned()][..]), ("wood", &[][..])]);
        assert!(list.iter().all(|c| c.kind != CandidateKind::Font), "{list:?}");
        // Every candidate's choice reads back from its token.
        assert!(list.iter().all(|c| PieceChoice::from_token(&c.choice.token()).as_ref() == Some(&c.choice)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
