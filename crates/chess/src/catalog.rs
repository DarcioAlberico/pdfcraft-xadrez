//! Font catalogues: which character of a chess diagram font or figurine font stands for which
//! piece, square or symbol.
//!
//! Most chess books from publishers are vector PDFs whose diagrams are set in a chess font. The
//! position is then literally in the file as text: a rank of the board is a run of eight
//! characters in a font such as Chess Merida. Reading it back is exact, with no model and no
//! rasterizing. This catalogue says what each character means, family by family.
//!
//! **Layouts, not fonts.** Chess fonts do not share an encoding, but they cluster into a few
//! layouts shared by whole families. A [`Layout`] is pure data. A [`Family`] names a layout and
//! the font-name patterns that select it. Adding a family is adding a row.
//!
//! **Evidence.** Every family carries a [`Confidence`]. Only `Verified` and `Inferred` families
//! are read as diagrams ([`Family::readable`]); an `Unverified` one is recognised (so its text is
//! not taken for prose) but not read unless the caller opts in. Characters whose meaning was not
//! checked are simply absent.
//!
//! **Figurine fonts** (SemFig, Figurine CB…) set moves inline: their text holds K/Q/R/B/N and
//! the glyph drawn is a piece, but there is no board. They are catalogued so that their text is
//! never read as a diagram, and so chess text can map their letters to pieces.
//!
//! The data comes from the user's Caïssa catalogue (verified glyph by glyph against installed
//! fonts) and, for the figurine font names, from the PDFimport plugin by Wagner Escorcio, used
//! with his permission (`docs/chess/provenance.md`). No font is included or needed.

use crate::placement::{Color, Piece, PieceKind};

/// How far a family's mapping was checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Confidence {
    /// Checked glyph by glyph against the font file.
    Verified,
    /// The font was not available, but its character set matches a verified family exactly.
    Inferred,
    /// Known from documentation or the shape of its name only.
    Unverified,
}

/// Whether a font draws diagrams or inline figurines.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontKind {
    Diagram,
    Figurine,
}

/// The colour of the square a glyph draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SquareColor {
    Light,
    Dark,
}

/// What one character means inside a diagram of a given family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GlyphKind {
    /// A piece stands on the square.
    Piece(Piece),
    /// An empty square.
    Empty,
    /// An empty square with a dot, cross or circle on it.
    Marker,
    /// A zero-advance background printed under a piece glyph on a dark square.
    Background,
    /// Frame or rule around the diagram.
    Border,
    /// A file or rank label in the frame: `Some('a')`…, or `None` when which one is unverified.
    Coordinate(Option<char>),
    /// Arcs, arrows, publisher marks: not part of the board.
    Decoration,
}

/// A character's role in a family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GlyphRole {
    pub kind: GlyphKind,
    /// The colour of the square the glyph draws, when it draws one.
    pub square: Option<SquareColor>,
    /// Overprint glyphs with zero advance: they paint the square under a piece and never take a
    /// cell of the 8×8 grid.
    pub zero_advance: bool,
}

impl GlyphRole {
    /// Whether the glyph stands on exactly one square of the board.
    pub fn occupies_cell(&self) -> bool {
        matches!(self.kind, GlyphKind::Piece(_) | GlyphKind::Empty | GlyphKind::Marker) && !self.zero_advance
    }
}

/// One encoding shared by a family of fonts. Piece strings list pawn, knight, bishop, rook,
/// queen and king, in that order; an empty string means the layout has no such set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub light_white: &'static str,
    pub light_black: &'static str,
    pub dark_white: &'static str,
    pub dark_black: &'static str,
    /// A second style of the same pieces (ChessBase ships a silhouette set for small sizes).
    pub alt_light_white: &'static str,
    pub alt_light_black: &'static str,
    pub empty_light: &'static str,
    pub empty_dark: &'static str,
    pub marker_light: &'static str,
    pub marker_dark: &'static str,
    /// Zero-advance dark-square backgrounds.
    pub background_dark: &'static str,
    pub border: &'static str,
    pub decoration: &'static str,
    /// Characters that print a known coordinate: pairs of (character, label).
    pub coordinates: &'static [(char, char)],
    /// Characters that print some coordinate whose identity is unverified.
    pub coordinates_unknown: &'static str,
    /// Pieces with no square colour (the Unicode chess block).
    pub neutral_white: &'static str,
    pub neutral_black: &'static str,
}

const EMPTY_LAYOUT: Layout = Layout {
    light_white: "",
    light_black: "",
    dark_white: "",
    dark_black: "",
    alt_light_white: "",
    alt_light_black: "",
    empty_light: "",
    empty_dark: "",
    marker_light: "",
    marker_dark: "",
    background_dark: "",
    border: "",
    decoration: "",
    coordinates: &[],
    coordinates_unknown: "",
    neutral_white: "",
    neutral_black: "",
};

const PIECE_ORDER: [PieceKind; 6] = [PieceKind::Pawn, PieceKind::Knight, PieceKind::Bishop, PieceKind::Rook, PieceKind::Queen, PieceKind::King];

impl Layout {
    /// The role of `c` in this layout. When a character is listed twice, the first list in this
    /// order wins: pieces (light, dark, alternative), empty squares, markers, backgrounds,
    /// borders, decorations, coordinates, colourless pieces.
    pub fn role(&self, c: char) -> Option<GlyphRole> {
        let piece_sets = [
            (self.light_white, Color::White, Some(SquareColor::Light)),
            (self.light_black, Color::Black, Some(SquareColor::Light)),
            (self.dark_white, Color::White, Some(SquareColor::Dark)),
            (self.dark_black, Color::Black, Some(SquareColor::Dark)),
            (self.alt_light_white, Color::White, Some(SquareColor::Light)),
            (self.alt_light_black, Color::Black, Some(SquareColor::Light)),
        ];
        for (set, color, square) in piece_sets {
            if let Some(kind) = piece_in(set, c) {
                return Some(GlyphRole { kind: GlyphKind::Piece(Piece::new(color, kind)), square, zero_advance: false });
            }
        }
        let plain = |kind, square| Some(GlyphRole { kind, square, zero_advance: false });
        let simple = [
            (self.empty_light, GlyphKind::Empty, Some(SquareColor::Light)),
            (self.empty_dark, GlyphKind::Empty, Some(SquareColor::Dark)),
            (self.marker_light, GlyphKind::Marker, Some(SquareColor::Light)),
            (self.marker_dark, GlyphKind::Marker, Some(SquareColor::Dark)),
        ];
        for (set, kind, square) in simple {
            if set.contains(c) {
                return plain(kind, square);
            }
        }
        if self.background_dark.contains(c) {
            return Some(GlyphRole { kind: GlyphKind::Background, square: Some(SquareColor::Dark), zero_advance: true });
        }
        if self.border.contains(c) {
            return plain(GlyphKind::Border, None);
        }
        if self.decoration.contains(c) {
            return plain(GlyphKind::Decoration, None);
        }
        if let Some((_, label)) = self.coordinates.iter().find(|(ch, _)| *ch == c) {
            return plain(GlyphKind::Coordinate(Some(*label)), None);
        }
        if self.coordinates_unknown.contains(c) {
            return plain(GlyphKind::Coordinate(None), None);
        }
        for (set, color) in [(self.neutral_white, Color::White), (self.neutral_black, Color::Black)] {
            if let Some(kind) = piece_in(set, c) {
                return plain(GlyphKind::Piece(Piece::new(color, kind)), None);
            }
        }
        None
    }

    fn piece_sets(&self) -> [&'static str; 8] {
        [
            self.light_white,
            self.light_black,
            self.dark_white,
            self.dark_black,
            self.alt_light_white,
            self.alt_light_black,
            self.neutral_white,
            self.neutral_black,
        ]
    }
}

fn piece_in(set: &str, c: char) -> Option<PieceKind> {
    set.chars().zip(PIECE_ORDER).find(|(ch, _)| *ch == c).map(|(_, k)| k)
}

// ---------------------------------------------------------------------------------------------
// Layouts
// ---------------------------------------------------------------------------------------------

/// Armando H. Marroquin's 1998 diagram fonts and everything that copied them. Case selects the
/// square colour and the letter the piece: light square `pnbrqk` white, `omvtwl` black; dark
/// square the same in upper case. Empty light square: space (also `*`); empty dark: `+`.
pub const MARROQUIN: Layout = Layout {
    light_white: "pnbrqk",
    light_black: "omvtwl",
    dark_white: "PNBRQK",
    dark_black: "OMVTWL",
    empty_light: " *",
    empty_dark: "+",
    marker_light: ".x",
    marker_dark: ":X",
    border: "!\"#$%()/12345789",
    decoration: "ADFSadfs?",
    ..EMPTY_LAYOUT
};

/// Eric Bentzen's Chess Alpha: Marroquin's case convention, but `n` is the **black bishop** and
/// the knights are on `h` and `j`. Reading Alpha with the Marroquin table silently swaps knights
/// and bishops, which is why the catalogue is per family.
pub const ALPHA: Layout = Layout {
    light_white: "phbrqk",
    light_black: "ojntwl",
    dark_white: "PHBRQK",
    dark_black: "OJNTWL",
    empty_light: " ",
    empty_dark: "+",
    marker_light: ")",
    marker_dark: "9",
    border: "!\"#$%&'(12345678",
    ..EMPTY_LAYOUT
};

/// Chess Utrecht swaps the axes: case selects the piece colour, the letter group the square.
pub const UTRECHT: Layout = Layout {
    light_white: "pnbrqk",
    light_black: "PNBRQK",
    dark_white: "omvtwl",
    dark_black: "OMVTWL",
    empty_light: " ",
    empty_dark: "/",
    border: "12345678",
    ..EMPTY_LAYOUT
};

/// OpenChessFont: Marroquin's letters with the square polarity of the case reversed.
pub const OPENCHESS: Layout = Layout {
    light_white: "PNBRQK",
    light_black: "OMVTWL",
    dark_white: "pnbrqk",
    dark_black: "omvtwl",
    empty_light: "z",
    empty_dark: "x",
    ..EMPTY_LAYOUT
};

/// ChessBase diagram fonts (DiagramTTF and the Linares, Zurich, Hastings diagram cuts). A piece
/// on a dark square is two glyphs: a zero-advance background, then the ordinary piece glyph.
/// Black pieces come in a detailed style (lower case) and a silhouette one (`ZSVTWM`).
pub const CHESSBASE_DIAGRAM: Layout = Layout {
    light_white: "PNLRQK",
    light_black: "pnlrqk",
    alt_light_black: "ZSVTWM",
    empty_light: " ",
    empty_dark: "+",
    // Every zero-advance glyph of DiagramTTFritz. The first six pair with pawn, knight,
    // bishop, rook, queen and king (measured by overprinting); only "dark square" is used.
    background_dark: "utswvm*/:`z",
    marker_light: "Oo<[{,.@",
    marker_dark: "]>;}J",
    border: "09iI\\|)=~",
    decoration: "XYxy^_j",
    coordinates: &[
        ('a', 'a'),
        ('b', 'b'),
        ('c', 'c'),
        ('d', 'd'),
        ('e', 'e'),
        ('f', 'f'),
        ('g', 'g'),
        ('h', 'h'),
        ('A', 'a'),
        ('B', 'b'),
        ('C', 'c'),
        ('D', 'd'),
        ('E', 'e'),
        ('F', 'f'),
        ('G', 'g'),
        ('H', 'h'),
    ],
    coordinates_unknown: "12345678!\"#$%&'(",
    ..EMPTY_LAYOUT
};

/// The LaTeX `skak` package's diagram font (SkakNew-Diagram). Case selects the piece colour on
/// light squares; on dark squares the letters shift to `OMATLJ` / `omatlj`. Empty squares are
/// `0` (light) and `Z` (dark). Read off a book's text layer and checked square by square against
/// the printed page.
pub const SKAK: Layout = Layout {
    light_white: "PNBRQK",
    light_black: "pnbrqk",
    dark_white: "OMATLJ",
    dark_black: "omatlj",
    empty_light: "0",
    empty_dark: "Z",
    ..EMPTY_LAYOUT
};

/// The Unicode chess block. No square colour: the board is drawn separately.
pub const UNICODE: Layout = Layout { neutral_white: "♙♘♗♖♕♔", neutral_black: "♟♞♝♜♛♚", ..EMPTY_LAYOUT };

/// Inline figurine fonts: no board, never read as a diagram. Their K/Q/R/B/N letters are the
/// English piece letters; [`figurine_letter`] maps them.
pub const FIGURINE_SAN: Layout = EMPTY_LAYOUT;

// ---------------------------------------------------------------------------------------------
// Families
// ---------------------------------------------------------------------------------------------

/// How a family is recognised from a font name, matched against [`normalize_font_name`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamePattern {
    /// The bare name contains this text.
    Contains(&'static str),
    /// The bare name is exactly this.
    Exact(&'static str),
    /// The bare name starts with this.
    Prefix(&'static str),
    /// The bare name contains the first text and, after it, the second.
    Then(&'static str, &'static str),
}

impl NamePattern {
    fn matches(self, bare: &str) -> bool {
        match self {
            NamePattern::Contains(s) => bare.contains(s),
            NamePattern::Exact(s) => bare == s,
            NamePattern::Prefix(s) => bare.starts_with(s),
            NamePattern::Then(a, b) => bare.find(a).and_then(|i| bare.get(i + a.len()..)).is_some_and(|rest| rest.contains(b)),
        }
    }

    /// How specific the pattern is: longer patterns are tried first, so that a family name wins
    /// over a generic rule that would also match.
    fn specificity(self) -> usize {
        match self {
            NamePattern::Contains(s) => s.len(),
            NamePattern::Exact(s) => s.len() + 2,
            NamePattern::Prefix(s) => s.len() + 1,
            NamePattern::Then(a, b) => a.len() + b.len() + 2,
        }
    }
}

/// A chess font family and how to read text set in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Family {
    pub key: &'static str,
    pub display_name: &'static str,
    pub patterns: &'static [NamePattern],
    pub layout: &'static Layout,
    pub kind: FontKind,
    pub confidence: Confidence,
    /// Where the mapping comes from and how it was checked.
    pub source: &'static str,
}

impl Family {
    /// The role of `c`, after folding the Windows symbol range (U+F020–U+F0FF) to ASCII.
    pub fn role(&self, c: char) -> Option<GlyphRole> {
        self.layout.role(normalize_char(c))
    }

    /// Whether a position can be read from this family without guessing.
    pub fn readable(&self) -> bool {
        self.kind == FontKind::Diagram && matches!(self.confidence, Confidence::Verified | Confidence::Inferred)
    }
}

const VERIFIED: &str =
    "Checked glyph by glyph against the installed font file (rendered, and cross-checked with a silhouette classifier calibrated on Chess Merida).";
const SYMBOL_CMAP: &str = "Checked glyph by glyph against the installed font file; the font has only a (3,0) symbol cmap in U+F020-U+F0EF.";

use Confidence::{Inferred, Unverified, Verified};
use FontKind::{Diagram, Figurine};
use NamePattern::{Contains, Exact, Prefix, Then};

const fn family(
    key: &'static str,
    display_name: &'static str,
    patterns: &'static [NamePattern],
    layout: &'static Layout,
    kind: FontKind,
    confidence: Confidence,
    source: &'static str,
) -> Family {
    Family { key, display_name, patterns, layout, kind, confidence, source }
}

/// Every catalogued family.
pub const FAMILIES: &[Family] = &[
    // Marroquin layout, verified font by font.
    family("merida", "Chess Merida", &[Contains("chessmerida"), Exact("merida"), Contains("meridachess")], &MARROQUIN, Diagram, Verified, VERIFIED),
    family("cases", "Chess Cases", &[Contains("chesscases")], &MARROQUIN, Diagram, Verified, VERIFIED),
    family("leipzig", "Chess Leipzig", &[Contains("chessleipzig")], &MARROQUIN, Diagram, Verified, VERIFIED),
    family("lucena", "Chess Lucena", &[Contains("chesslucena")], &MARROQUIN, Diagram, Verified, VERIFIED),
    family("marroquin", "Chess Marroquin", &[Contains("chessmarroquin"), Exact("marroquin")], &MARROQUIN, Diagram, Verified, VERIFIED),
    family("motif", "Chess Motif", &[Contains("chessmotif")], &MARROQUIN, Diagram, Verified, SYMBOL_CMAP),
    family("kingdom", "Chess Kingdom", &[Contains("chesskingdom")], &MARROQUIN, Diagram, Verified, SYMBOL_CMAP),
    family("condal", "Chess Condal", &[Contains("chesscondal")], &MARROQUIN, Diagram, Verified, VERIFIED),
    family("maya", "Chess Maya", &[Contains("chessmaya")], &MARROQUIN, Diagram, Verified, VERIFIED),
    family("mediaeval", "Chess Mediaeval", &[Contains("chessmediaeval"), Contains("chessmedieval")], &MARROQUIN, Diagram, Verified, VERIFIED),
    family("magnetic", "Chess Magnetic", &[Contains("chessmagnetic")], &MARROQUIN, Diagram, Verified, VERIFIED),
    family("adventurer", "Chess Adventurer", &[Contains("chessadventurer")], &MARROQUIN, Diagram, Verified, VERIFIED),
    family("alfonsox", "Chess Alfonso-X", &[Contains("chessalfonso")], &MARROQUIN, Diagram, Verified, SYMBOL_CMAP),
    family("harlequin", "Chess Harlequin", &[Contains("chessharlequin")], &MARROQUIN, Diagram, Verified, SYMBOL_CMAP),
    family("line", "Chess Line", &[Contains("chessline")], &MARROQUIN, Diagram, Verified, SYMBOL_CMAP),
    family("millennia", "Chess Millennia", &[Contains("chessmillennia")], &MARROQUIN, Diagram, Verified, VERIFIED),
    family("chess7", "Chess 7", &[Exact("chess7")], &MARROQUIN, Diagram, Verified, VERIFIED),
    // Marroquin layout, named in chess-font literature but not checked.
    family(
        "berlin",
        "Chess Berlin",
        &[Contains("chessberlin"), Exact("berlin")],
        &MARROQUIN,
        Diagram,
        Unverified,
        "Named in chess-font literature; the file was not available, the layout was not checked.",
    ),
    family(
        "uscf",
        "USCF",
        &[Exact("uscf")],
        &MARROQUIN,
        Diagram,
        Unverified,
        "US Chess Federation set; the file was not available, the layout was not checked.",
    ),
    family(
        "tilburg",
        "Chess Tilburg",
        &[Contains("tilburg")],
        &MARROQUIN,
        Diagram,
        Unverified,
        "The file was not available; the layout was not checked.",
    ),
    // Other verified layouts.
    family(
        "alpha",
        "Chess Alpha",
        &[Contains("chessalpha")],
        &ALPHA,
        Diagram,
        Verified,
        "Checked glyph by glyph against the installed font file. Note: 'n' is the black bishop, not the knight.",
    ),
    family(
        "utrecht",
        "Chess Utrecht",
        &[Contains("chessutrecht")],
        &UTRECHT,
        Diagram,
        Verified,
        "Checked glyph by glyph against the installed font file. Case selects the piece colour; the letter group, the square colour.",
    ),
    family(
        "openchess",
        "OpenChessFont",
        &[Contains("openchessfont")],
        &OPENCHESS,
        Diagram,
        Verified,
        "Checked glyph by glyph against the installed font file. Case polarity reversed from Marroquin.",
    ),
    family(
        "chessbase_diagram",
        "ChessBase DiagramTTF",
        &[Contains("diagramttf"), Prefix("diattf"), Contains("diagramttfritz")],
        &CHESSBASE_DIAGRAM,
        Diagram,
        Verified,
        "Checked glyph by glyph against the installed font file. Dark squares are a zero-advance background glyph plus the piece.",
    ),
    family(
        "linares_diagram",
        "Linares Diagram",
        &[Contains("linaresdiagram")],
        &CHESSBASE_DIAGRAM,
        Diagram,
        Unverified,
        "Only subsets extracted from PDFs were available; layout presumed to be DiagramTTF's (same origin), not checked.",
    ),
    family(
        "zurich_diagram",
        "Zurich Diagram",
        &[Contains("zurichdiagram")],
        &CHESSBASE_DIAGRAM,
        Diagram,
        Unverified,
        "Only subsets extracted from PDFs were available; layout presumed, not checked.",
    ),
    family(
        "hastings_diagram",
        "Hastings Diagram",
        &[Contains("hastingsdiagram")],
        &CHESSBASE_DIAGRAM,
        Diagram,
        Unverified,
        "Only subsets extracted from PDFs were available; layout presumed, not checked.",
    ),
    family(
        "skak_diagram",
        "SkakNew Diagram",
        &[Contains("skaknewdiagram"), Contains("skakdiagram")],
        &SKAK,
        Diagram,
        Inferred,
        "Encoding of the LaTeX skak package, read from a book's text layer and checked square by square against the printed page.",
    ),
    family("unicode", "Unicode Chess Symbols", &[], &UNICODE, Diagram, Verified, "Unicode block U+2654-U+265F; independent of the font."),
    // Inline figurine fonts: recognised so that they are never read as diagrams.
    family("semfig", "SemFig", &[Contains("semfig")], &FIGURINE_SAN, Figurine, Verified, "Inline figurine family; draws no board."),
    family(
        "figurinecb",
        "Figurine CB",
        &[Contains("figurinecb"), Then("figurine", "time")],
        &FIGURINE_SAN,
        Figurine,
        Verified,
        "ChessBase inline figurine.",
    ),
    family("linares_figurine", "Linares Figurine", &[Contains("linaresfigurine")], &FIGURINE_SAN, Figurine, Verified, "Inline figurine."),
    family("zurich_figurine", "Zurich Figurine", &[Contains("zurichfigurine")], &FIGURINE_SAN, Figurine, Verified, "Inline figurine."),
    family("hastings_figurine", "Hastings Figurine", &[Contains("hastingsfigurine")], &FIGURINE_SAN, Figurine, Verified, "Inline figurine."),
    family("verfig", "Verfig", &[Contains("verfig")], &FIGURINE_SAN, Figurine, Verified, "Inline figurine."),
    family(
        "skaknew",
        "SkakNew",
        &[Contains("skaknew"), Contains("skak")],
        &FIGURINE_SAN,
        Figurine,
        Verified,
        "Inline figurine of the LaTeX skak package.",
    ),
    family(
        "chess_generic_figurine",
        "Generic figurine",
        &[Contains("figurin"), Contains("chessfig"), Exact("dgt")],
        &FIGURINE_SAN,
        Figurine,
        Unverified,
        "Recognised by name only, so its text is not read as a diagram.",
    ),
];

/// The family registered under `key`.
pub fn family_by_key(key: &str) -> Option<&'static Family> {
    FAMILIES.iter().find(|f| f.key == key)
}

/// The chess font family a PDF font name belongs to. An empty name is the Unicode family: text in
/// U+2654–U+265F needs no special font.
pub fn lookup_family(font_name: &str) -> Option<&'static Family> {
    let bare = normalize_font_name(font_name);
    if bare.is_empty() {
        return family_by_key("unicode");
    }
    let mut patterns: Vec<(NamePattern, &'static Family)> = FAMILIES.iter().flat_map(|f| f.patterns.iter().map(move |p| (*p, f))).collect();
    // Stable sort: equally specific patterns keep catalogue order.
    patterns.sort_by_key(|(p, _)| std::cmp::Reverse(p.specificity()));
    patterns.into_iter().find(|(p, _)| p.matches(&bare)).map(|(_, f)| f)
}

/// Fragments that mean "some chess font" without naming a family.
const GENERIC_CHESS_NAMES: &[&str] = &[
    "semfig",
    "figurin",
    "chessfig",
    "chessbase",
    "diagramttf",
    "dgt",
    "linares",
    "marroquin",
    "zurich",
    "berlin",
    "cheq",
    "uscf",
    "tilburg",
    "utrecht",
    "leipzig",
    "lucena",
    "merida",
    "skak",
];

/// Whether a font name looks like some chess font, catalogued or not. Keeps an unknown chess
/// font from being taken for body text, and lets unknown families be logged so the catalogue
/// can grow. `chessboard` alone is not a chess font name.
pub fn looks_like_chess_font(font_name: &str) -> bool {
    let bare = normalize_font_name(font_name);
    if bare.is_empty() {
        return false;
    }
    if GENERIC_CHESS_NAMES.iter().any(|g| bare.contains(g)) {
        return true;
    }
    bare.match_indices("chess").any(|(i, _)| !bare.get(i + 5..).is_some_and(|rest| rest.starts_with("board")))
}

/// Style words dropped from the end of a font name.
const STYLE_SUFFIXES: &[&str] = &["regular", "roman", "book", "normal", "bold", "italic", "oblique", "medium", "light", "mt", "ms", "psmt"];

/// The bare, comparable form of a PDF font name: no `ABCDEF+` subset prefix (ISO 32000-2
/// §9.9.2), nothing after a comma, no spaces, `_`, `-` or `.`, lower case, and no trailing style
/// words. `ABCDEF+Chess-Merida,Bold` and `ChessMerida-Regular` both become `chessmerida`.
pub fn normalize_font_name(font_name: &str) -> String {
    let name = strip_subset_prefix(font_name.trim());
    let name = name.split(',').next().unwrap_or("");
    let bare: String = name.chars().filter(|c| !c.is_whitespace() && !matches!(c, '_' | '-' | '.')).flat_map(char::to_lowercase).collect();
    let mut trimmed = bare.as_str();
    while let Some(rest) = STYLE_SUFFIXES.iter().find_map(|s| trimmed.strip_suffix(s)) {
        trimmed = rest;
    }
    if trimmed.is_empty() { bare } else { trimmed.to_owned() }
}

/// `name` without a subset tag: six upper-case ASCII letters and `+`.
pub fn strip_subset_prefix(name: &str) -> &str {
    let bytes = name.as_bytes();
    let tagged = bytes.len() > 7 && bytes.get(..6).is_some_and(|b| b.iter().all(u8::is_ascii_uppercase)) && bytes.get(6) == Some(&b'+');
    // The first seven bytes are ASCII, so byte 7 is a char boundary.
    if tagged { name.get(7..).unwrap_or(name) } else { name }
}

/// Fold the Windows symbol range down to ASCII. Fonts with only a (3,0) symbol cmap (Chess
/// Motif, Kingdom, Alfonso-X…) make extractors return U+F020–U+F0FF where the file means ASCII.
pub fn normalize_char(c: char) -> char {
    let code = u32::from(c);
    if (0xF000..=0xF0FF).contains(&code) { char::from_u32(code - 0xF000).unwrap_or(c) } else { c }
}

/// The piece a letter of an inline figurine font draws. Those fonts put the glyph on the English
/// SAN letters; the case does not change the piece.
pub fn figurine_letter(c: char) -> Option<PieceKind> {
    Some(match normalize_char(c).to_ascii_uppercase() {
        'K' => PieceKind::King,
        'Q' => PieceKind::Queen,
        'R' => PieceKind::Rook,
        'B' => PieceKind::Bishop,
        'N' => PieceKind::Knight,
        'P' => PieceKind::Pawn,
        _ => return None,
    })
}

/// Whether a font is ISChess, a symbol font in the Chess Informant tradition: every letter draws
/// a sign or a piece ([`ischess_piece`], [`ischess_sign`]).
pub fn is_ischess(font_name: &str) -> bool {
    normalize_font_name(font_name).contains("ischess")
}

/// The piece an ISChess letter draws: the pieces sit on `A`–`L` in White/Black pairs (`A` ♙,
/// `B` ♟, `C` ♘ … `K` ♔, `L` ♚), so the font tells the colours apart.
pub fn ischess_piece(c: char) -> Option<Piece> {
    let (kind, color) = match normalize_char(c) {
        'A' => (PieceKind::Pawn, Color::White),
        'B' => (PieceKind::Pawn, Color::Black),
        'C' => (PieceKind::Knight, Color::White),
        'D' => (PieceKind::Knight, Color::Black),
        'E' => (PieceKind::Bishop, Color::White),
        'F' => (PieceKind::Bishop, Color::Black),
        'G' => (PieceKind::Rook, Color::White),
        'H' => (PieceKind::Rook, Color::Black),
        'I' => (PieceKind::Queen, Color::White),
        'J' => (PieceKind::Queen, Color::Black),
        'K' => (PieceKind::King, Color::White),
        'L' => (PieceKind::King, Color::Black),
        _ => return None,
    };
    Some(Piece::new(color, kind))
}

/// What the other ISChess letters draw. Signs with a PGN annotation code are written as the
/// annotation table ([`crate::nag`]) writes them, so they read back as that annotation (`O` ±,
/// `Q` +-, `p` ⯺ connected pawns); the three bishop signs, which have no Unicode character, as
/// the table's stand-ins for them (`m` ⧉, `n` □■, `o` ■■); the rest are the nearest Unicode
/// signs. `!` draws `∞` over `=`, which the user identified with compensation from a book's own
/// key to symbols (black-box, 2026-10-22): it is `⯹`, which reads as `©`, so a book's `!` after a
/// move is compensation, never "good move". Read off the glyph table printed in the font
/// itself, as the PDFimport plugin by Wagner Escorcio records it (used with his permission,
/// `docs/chess/provenance.md`), and checked against the font file (`IS-TT-01.TTF`, family
/// ISChess).
const ISCHESS_SIGNS: &[(char, &str)] = &[
    ('a', "!?"),
    ('b', "?!"),
    ('c', "∆"),
    ('d', "□"),
    ('e', "⌓"),
    ('f', "⇔"),
    ('g', "⇗"),
    ('h', "⊞"),
    ('i', "⟫"),
    ('j', "⟪"),
    ('k', "✕"),
    ('l', "⊥"),
    ('m', "⧉"),
    ('n', "□■"),
    ('o', "■■"),
    ('p', "⯺"),
    ('q', "⯻"),
    ('r', "⯼"),
    ('s', "⯽"),
    ('t', ">"),
    ('u', "⨁"),
    ('v', "∟"),
    ('w', "⯾"),
    ('x', "‖"),
    ('y', "−"),
    ('z', "▲"),
    ('M', "⩲"),
    ('N', "⩱"),
    ('O', "±"),
    ('P', "∓"),
    ('Q', "+-"),
    ('R', "-+"),
    ('S', "="),
    ('T', "∞"),
    ('U', "©"),
    ('V', "⟳"),
    ('W', "○"),
    ('X', "→"),
    ('Y', "↑"),
    ('Z', "⇆"),
    ('[', "⨀"),
    // `∞` over `=`: compensation, as the user identified it (see above), not the plain `!`.
    ('!', "⯹"),
];

/// The sign an ISChess letter draws, as text ([`ISCHESS_SIGNS`]); `None` for a piece letter or
/// anything else.
pub fn ischess_sign(c: char) -> Option<&'static str> {
    let c = normalize_char(c);
    ISCHESS_SIGNS.iter().find(|(l, _)| *l == c).map(|(_, s)| *s)
}

/// Check that every piece string of every layout lists exactly six distinct characters. Part of
/// the tests; public so a caller adding a layout can run it too.
pub fn layout_is_well_formed(layout: &Layout) -> bool {
    layout.piece_sets().iter().all(|set| {
        let chars: Vec<char> = set.chars().collect();
        chars.is_empty() || (chars.len() == 6 && chars.iter().enumerate().all(|(i, c)| !chars.iter().skip(i + 1).any(|d| d == c)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piece(c: char) -> Piece {
        Piece::from_fen_char(c).unwrap()
    }

    #[test]
    fn every_layout_is_well_formed() {
        for f in FAMILIES {
            assert!(layout_is_well_formed(f.layout), "{}", f.key);
        }
        for f in FAMILIES {
            assert_eq!(FAMILIES.iter().filter(|g| g.key == f.key).count(), 1, "{}", f.key);
        }
    }

    #[test]
    fn marroquin_case_selects_the_square() {
        let merida = family_by_key("merida").unwrap();
        let role = merida.role('n').unwrap();
        assert_eq!(role.kind, GlyphKind::Piece(piece('N')));
        assert_eq!(role.square, Some(SquareColor::Light));
        assert_eq!(merida.role('M').unwrap().kind, GlyphKind::Piece(piece('n')));
        assert_eq!(merida.role('M').unwrap().square, Some(SquareColor::Dark));
        assert_eq!(merida.role('+').unwrap().kind, GlyphKind::Empty);
        assert!(merida.role('+').unwrap().occupies_cell());
        assert_eq!(merida.role('1').unwrap().kind, GlyphKind::Border);
        assert_eq!(merida.role('Z'), None);
    }

    #[test]
    fn alpha_n_is_the_black_bishop() {
        let alpha = family_by_key("alpha").unwrap();
        assert_eq!(alpha.role('n').unwrap().kind, GlyphKind::Piece(piece('b')));
        assert_eq!(alpha.role('h').unwrap().kind, GlyphKind::Piece(piece('N')));
    }

    #[test]
    fn chessbase_backgrounds_take_no_cell() {
        let cb = family_by_key("chessbase_diagram").unwrap();
        let bg = cb.role('t').unwrap();
        assert_eq!(bg.kind, GlyphKind::Background);
        assert!(bg.zero_advance);
        assert!(!bg.occupies_cell());
        // `L` is the white bishop, which takes precedence over nothing; `B` is a coordinate.
        assert_eq!(cb.role('L').unwrap().kind, GlyphKind::Piece(piece('B')));
        assert_eq!(cb.role('B').unwrap().kind, GlyphKind::Coordinate(Some('b')));
        assert_eq!(cb.role('S').unwrap().kind, GlyphKind::Piece(piece('n')));
    }

    #[test]
    fn symbol_range_folds_to_ascii() {
        let motif = family_by_key("motif").unwrap();
        assert_eq!(motif.role('\u{F06E}'), motif.role('n'));
        assert_eq!(normalize_char('\u{F041}'), 'A');
        assert_eq!(normalize_char('é'), 'é');
    }

    #[test]
    fn font_names_normalize() {
        assert_eq!(normalize_font_name("ABCDEF+Chess-Merida,Bold"), "chessmerida");
        assert_eq!(normalize_font_name("ChessMerida-Regular"), "chessmerida");
        assert_eq!(normalize_font_name("CHESS_MERIDA"), "chessmerida");
        assert_eq!(normalize_font_name("Bold"), "bold");
        assert_eq!(normalize_font_name(""), "");
        assert_eq!(strip_subset_prefix("ABCDEF+X"), "X");
        assert_eq!(strip_subset_prefix("ABCDE+X"), "ABCDE+X");
        assert_eq!(strip_subset_prefix("ABCDEF+"), "ABCDEF+");
        assert_eq!(strip_subset_prefix("ÁBCDEF+X"), "ÁBCDEF+X");
    }

    #[test]
    fn families_are_found_by_name() {
        let key = |n: &str| lookup_family(n).map(|f| f.key);
        assert_eq!(key("ABCDEF+ChessMerida"), Some("merida"));
        assert_eq!(key("Merida"), Some("merida"));
        assert_eq!(key("Chess Alpha 2"), Some("alpha"));
        assert_eq!(key("DiaTTFri"), Some("chessbase_diagram"));
        assert_eq!(key("SkakNew-Diagram"), Some("skak_diagram"));
        assert_eq!(key("SkakNew-Figurine"), Some("skaknew"));
        assert_eq!(key("FigurineCB AriesSP"), Some("figurinecb"));
        assert_eq!(key("FigurineTimesBold"), Some("figurinecb"));
        assert_eq!(key("SemFigBold"), Some("semfig"));
        assert_eq!(key("USCF"), Some("uscf"));
        assert_eq!(key("Chess 7"), Some("chess7"));
        assert_eq!(key(""), Some("unicode"));
        assert_eq!(key("Times-Roman"), None);
        assert!(!lookup_family("ChessMerida").unwrap().patterns.is_empty());
        assert!(lookup_family("Chess Berlin").is_some_and(|f| !f.readable()));
        assert!(lookup_family("SemFig").is_some_and(|f| !f.readable()));
        assert!(lookup_family("Chess Leipzig").is_some_and(Family::readable));
    }

    #[test]
    fn generic_chess_names() {
        assert!(looks_like_chess_font("ABCDEF+SomeChessFont"));
        assert!(looks_like_chess_font("ChessBase Sans"));
        assert!(!looks_like_chess_font("Chessboard Sans"));
        assert!(looks_like_chess_font("Chessboard Chess"));
        assert!(!looks_like_chess_font("Times New Roman"));
        assert!(!looks_like_chess_font(""));
    }

    #[test]
    fn figurine_letters() {
        assert_eq!(figurine_letter('N'), Some(PieceKind::Knight));
        assert_eq!(figurine_letter('\u{F051}'), Some(PieceKind::Queen));
        assert_eq!(figurine_letter('x'), None);
    }

    proptest::proptest! {
        #[test]
        fn never_panics(s in "\\PC{0,40}", c in proptest::char::any()) {
            let _ = lookup_family(&s);
            let _ = looks_like_chess_font(&s);
            for f in FAMILIES {
                let _ = f.role(c);
            }
        }
    }

    #[test]
    fn ischess_letters() {
        assert!(is_ischess("ABCDEF+ISChess") && is_ischess("IS-Chess") && !is_ischess("SemFig"));
        // Pieces in White/Black pairs on A–L.
        assert_eq!(ischess_piece('A'), Some(Piece::new(Color::White, PieceKind::Pawn)));
        assert_eq!(ischess_piece('D'), Some(Piece::new(Color::Black, PieceKind::Knight)));
        assert_eq!(ischess_piece('L'), Some(Piece::new(Color::Black, PieceKind::King)));
        assert_eq!(ischess_piece('M'), None);
        // Every other letter is a sign, and no letter is both.
        for c in ('a'..='z').chain('M'..='Z') {
            assert!(ischess_sign(c).is_some() && ischess_piece(c).is_none(), "{c}");
        }
        assert_eq!(ischess_sign('O'), Some("±"));
        assert_eq!(ischess_sign('\u{F04F}'), Some("±"), "symbol cmap");
        // Signs with an annotation code are spelled as the annotation table spells them.
        for (letter, code) in [('a', 5), ('d', 7), ('M', 14), ('Q', 18), ('R', 19), ('U', 44), ('Z', 132), ('u', 138), ('c', 140)] {
            assert_eq!(ischess_sign(letter), crate::nag::by_code(code).and_then(|n| n.glyph), "{letter}");
        }
        // Checked against the font's drawings: the ring is space, zugzwang the ringed dot, the
        // ChessPad signs on their letters (the cross a weak point, never the capture sign).
        let chesspad = [('W', 26), ('[', 22), ('e', 142), ('f', 239), ('g', 240), ('h', 241), ('i', 242), ('j', 243), ('k', 244), ('l', 245)];
        let pawns_and_bishops = [('m', 246), ('n', 247), ('o', 248), ('p', 249), ('q', 250), ('r', 251), ('s', 252), ('v', 254), ('w', 255)];
        for (letter, code) in chesspad.into_iter().chain(pawns_and_bishops) {
            let sign = ischess_sign(letter).unwrap();
            assert_eq!(Some(sign), crate::nag::by_code(code).and_then(|n| n.glyph), "{letter}");
            assert_eq!(crate::nag::by_glyph(sign).map(|n| n.code), Some(code), "{letter} reads back");
        }
        assert_eq!(ischess_sign('k'), Some("✕"), "not ×");
        assert_eq!(ischess_sign('!'), Some("⯹"), "∞ over =: compensation, not good move");
        assert_eq!(ischess_sign('\u{F021}'), Some("⯹"), "symbol cmap");
        assert_eq!(crate::nag::nags_from_suffix(ischess_sign('!').unwrap_or("!"), false), [45]);
    }
}
