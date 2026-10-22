//! Numeric Annotation Glyphs: PGN code, printed glyph and the aliases publishers use.
//!
//! One table answers three questions: which glyphs in a text are annotations, which `$n` a glyph
//! is written as in PGN, and what to offer a user who wants to add one.
//!
//! Two things are not obvious:
//!
//! - **Publisher symbols do not survive Unicode normalization.** The symbol fonts of several
//!   chess publishers reuse Latin code points: `²` is "White is slightly better", `³` the same for
//!   Black, `µ` "Black is clearly better", `¹` "better is". NFKC turns `²` into `2`, so `Rad8³`
//!   would become `Rad83` and the evaluation would be gone. [`normalize_publisher_symbols`] must
//!   run on the raw text, before any normalization.
//! - **Not every annotation fits a glyph.** The novelty `$146` is printed `N`, which is also the
//!   knight. Such entries are marked unsafe as free glyphs and are only recognised glued to the
//!   end of a move (`12.Nf4N`). Most of the standard's codes have no sign at all ("slight
//!   space advantage", `$24`): their glyph is `None`, they are written `$n` and offered by name,
//!   and nothing in a text is ever read as one.

/// What an annotation talks about, for grouping in a palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NagGroup {
    /// The move's quality (`!`, `?`, `□`).
    Move,
    /// Who stands better, and compensation for material.
    Evaluation,
    /// Development, initiative, attack, counterplay, zugzwang and the clock.
    Dynamics,
    /// Signs about ideas and alternatives (`∆`, `⌓`, with, without).
    Intent,
    /// Space, and control of the center and the wings.
    Space,
    /// The first rank and the king's safety and placement.
    King,
    /// Pawn structure and the pawn signs.
    Pawns,
    /// Piece placement and coordination, and the bishop signs.
    Pieces,
    /// How the opening, middlegame and endgame were played.
    Phases,
    /// The positional signs: space, where the play is (a file, a diagonal, a wing), a weak
    /// point, the ending.
    Positional,
}

/// One annotation symbol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Nag {
    /// The PGN code, `$code`. For side-dependent symbols, the code when White made the move.
    pub code: u16,
    /// The code when Black made the move, for symbols whose number depends on the side
    /// (zugzwang is `$22` for White and `$23` for Black, with one glyph).
    pub black_code: Option<u16>,
    /// The canonical printed glyph, or `None` for an assessment books write out in words
    /// ("slight space advantage", `$24`): it is written `$n`, offered by its name, and never
    /// looked for in text.
    pub glyph: Option<&'static str>,
    /// English name; the interface translates it.
    pub name: &'static str,
    pub group: NagGroup,
    /// `true`: the symbol speaks of the side that made the move. `false` (zugzwang): of the side
    /// now to move.
    pub side_is_mover: bool,
    /// `false` when the glyph is not looked for in free text: too ambiguous (`N` is the knight),
    /// or drawn like another entry that owns it in text (`$238`'s `○` reads as `$26`).
    pub safe_glyph: bool,
    /// Recognised only glued to the end of a move.
    pub suffix_only: bool,
    /// Other spellings found in books: publisher symbol fonts, Informator ASCII, OCR leftovers.
    /// Only entries read in text have them (an alias becomes the glyph).
    pub aliases: &'static [&'static str],
}

impl Nag {
    /// The PGN code for the side that made the annotated move.
    pub fn code_for(&self, white_moved: bool) -> u16 {
        match self.black_code {
            Some(black) if !white_moved => black,
            _ => self.code,
        }
    }

    /// The glyph when it is read in text: alone (safe) or glued after a move (suffix only).
    /// `None` for an entry without a glyph and for one whose glyph another entry owns.
    pub fn read_glyph(&self) -> Option<&'static str> {
        self.glyph.filter(|_| self.safe_glyph || self.suffix_only)
    }

    /// What a palette inserts: the glyph, or `$n` when the glyph is unsafe or there is none.
    pub fn insert_text(&self) -> String {
        match self.glyph {
            Some(g) if self.safe_glyph => g.to_owned(),
            _ => format!("${}", self.code),
        }
    }
}

const fn nag(code: u16, glyph: &'static str, name: &'static str, group: NagGroup) -> Nag {
    Nag { code, black_code: None, glyph: Some(glyph), name, group, side_is_mover: true, safe_glyph: true, suffix_only: false, aliases: &[] }
}

/// An assessment without a printed sign.
const fn word(code: u16, name: &'static str, group: NagGroup) -> Nag {
    Nag { code, black_code: None, glyph: None, name, group, side_is_mover: true, safe_glyph: false, suffix_only: false, aliases: &[] }
}

const fn with_aliases(n: Nag, aliases: &'static [&'static str]) -> Nag {
    Nag { aliases, ..n }
}

const fn for_black(n: Nag, black_code: u16) -> Nag {
    Nag { black_code: Some(black_code), ..n }
}

/// White's code `n.code`, and Black's the next one, as the PGN standard numbers its pairs.
const fn pair(n: Nag) -> Nag {
    for_black(n, n.code + 1)
}

/// The table, in display order: the PGN standard's codes 1–139 (but 0, "no annotation"), then
/// the de-facto extension 140–146 and 238–255 that the public table of non-standard NAGs (the
/// Wikipedia article "Numeric Annotation Glyphs") attributes to the ChessPad editor. The names
/// are this table's own wording. The publisher aliases come from the key to symbols printed in
/// chess books, checked against the PGN files their publishers distribute with them.
///
/// Signs that are shared, or close to others, settled here:
///
/// - `=` is `$10`. The two equal-chances codes `$11` and `$12` have no sign of their own, and
///   `==` is ChessPad's equivalent move `$144`: the longest sign wins, so `==` is not two `=`,
///   and a promotion (`e8=Q`) is the move's own text, read before any sign.
/// - `○` reads as the standard space advantage, `$26`/`$27` by the side (the ISChess font draws
///   it on `W`). ChessPad's `$238` draws the same ring but is not read from text.
/// - The three bishop signs have no Unicode character. Their glyphs are stand-ins, the ones the
///   ISChess font's letters `m` `n` `o` are read as ([`crate::catalog::ischess_sign`]): `⧉` the
///   pair, `□■` opposite colors, `■■` the same color; `□■` is one sign, not the only move `□`
///   (the longest sign wins). They are read only glued after a move (`■` turns up in the text
///   of OCR'd diagrams), PGN writes them `$246`–`$248`, and the interface paints them.
/// - `⇔` (a file, `$239`) is not counterplay: that is `⇆` and its alias `⇄` (`$132`).
/// - `✕` (U+2715, a weak point, `$244`) is not the capture sign `×` (U+00D7), which no entry has.
/// - `⯹`, the Unicode sign for compensation, is an alias of `©` (`$44`).
pub const NAGS: &[Nag] = &[
    // Move quality.
    nag(1, "!", "Good move", NagGroup::Move),
    nag(2, "?", "Poor move", NagGroup::Move),
    nag(3, "!!", "Brilliant move", NagGroup::Move),
    nag(4, "??", "Blunder", NagGroup::Move),
    nag(5, "!?", "Interesting move", NagGroup::Move),
    nag(6, "?!", "Dubious move", NagGroup::Move),
    // `◻` (U+25FB) is the square an Informator symbol font emits.
    with_aliases(nag(7, "□", "Only move", NagGroup::Move), &["™", "◻"]),
    Nag { safe_glyph: false, suffix_only: true, ..nag(146, "N", "Novelty", NagGroup::Move) },
    nag(144, "==", "Equivalent move", NagGroup::Move),
    word(8, "Singular move", NagGroup::Move),
    word(9, "Worst move", NagGroup::Move),
    // Evaluation.
    nag(10, "=", "Equal position", NagGroup::Evaluation),
    nag(13, "∞", "Unclear position", NagGroup::Evaluation),
    with_aliases(nag(14, "⩲", "White is slightly better", NagGroup::Evaluation), &["²", "+/=", "+="]),
    with_aliases(nag(15, "⩱", "Black is slightly better", NagGroup::Evaluation), &["³", "=/+", "=+"]),
    with_aliases(nag(16, "±", "White is clearly better", NagGroup::Evaluation), &["+/-"]),
    // `µ` is the micro sign; NFKC turns it into Greek mu, so both are listed.
    with_aliases(nag(17, "∓", "Black is clearly better", NagGroup::Evaluation), &["µ", "μ", "-/+"]),
    with_aliases(nag(18, "+-", "White is winning", NagGroup::Evaluation), &["+–", "+—", "+−"]),
    with_aliases(nag(19, "-+", "Black is winning", NagGroup::Evaluation), &["–+", "—+", "−+"]),
    for_black(with_aliases(nag(44, "©", "Compensation for the material", NagGroup::Evaluation), &["=/∞", "⯹"]), 45),
    word(11, "Equal chances, quiet position", NagGroup::Evaluation),
    word(12, "Equal chances, active position", NagGroup::Evaluation),
    word(20, "White has a crushing advantage", NagGroup::Evaluation),
    word(21, "Black has a crushing advantage", NagGroup::Evaluation),
    pair(word(42, "Insufficient compensation for the material", NagGroup::Evaluation)),
    pair(word(46, "More than enough compensation for the material", NagGroup::Evaluation)),
    // Dynamics.
    Nag { side_is_mover: false, ..for_black(with_aliases(nag(22, "⨀", "Zugzwang", NagGroup::Dynamics), &["⊙", "ʘ"]), 23) },
    for_black(with_aliases(nag(32, "⟳", "Development advantage", NagGroup::Dynamics), &["‰"]), 33),
    for_black(with_aliases(nag(36, "↑", "With the initiative", NagGroup::Dynamics), &["ƒ"]), 37),
    for_black(with_aliases(nag(40, "→", "With an attack", NagGroup::Dynamics), &["‚"]), 41),
    for_black(with_aliases(nag(132, "⇆", "With counterplay", NagGroup::Dynamics), &["„", "⇄"]), 133),
    for_black(with_aliases(nag(138, "⨁", "Time trouble", NagGroup::Dynamics), &["⊕"]), 139),
    pair(word(30, "Slight development advantage", NagGroup::Dynamics)),
    pair(word(34, "Decisive development advantage", NagGroup::Dynamics)),
    pair(word(38, "Lasting initiative", NagGroup::Dynamics)),
    pair(word(130, "Slight counterplay", NagGroup::Dynamics)),
    pair(word(134, "Decisive counterplay", NagGroup::Dynamics)),
    pair(word(136, "Moderate time pressure", NagGroup::Dynamics)),
    // Intent. The ellipsis is deliberately not an alias of ∆ although some symbol keys say so:
    // nearly every chess PDF uses it for Black's move numbers (`7…h6`).
    with_aliases(nag(140, "∆", "With the idea", NagGroup::Intent), &["Δ", "△"]),
    nag(141, "∇", "Aimed against", NagGroup::Intent),
    with_aliases(nag(142, "⌓", "Better is", NagGroup::Intent), &["¹"]),
    nag(143, "≤", "Worse is", NagGroup::Intent),
    Nag { safe_glyph: false, ..nag(145, "RR", "Editorial comment", NagGroup::Intent) },
    nag(254, "∟", "With", NagGroup::Intent),
    nag(255, "⯾", "Without", NagGroup::Intent),
    // The positional signs (ChessPad's, and the standard's ring for space).
    pair(nag(26, "○", "Moderate space advantage", NagGroup::Positional)),
    Nag { safe_glyph: false, ..nag(238, "○", "Space advantage", NagGroup::Positional) },
    nag(239, "⇔", "Play along the file", NagGroup::Positional),
    nag(240, "⇗", "Play along the diagonal", NagGroup::Positional),
    nag(241, "⊞", "Play in the center", NagGroup::Positional),
    nag(242, "⟫", "Play on the kingside", NagGroup::Positional),
    nag(243, "⟪", "Play on the queenside", NagGroup::Positional),
    nag(244, "✕", "Weak point", NagGroup::Positional),
    nag(245, "⊥", "Endgame", NagGroup::Positional),
    // Space.
    pair(word(24, "Slight space advantage", NagGroup::Space)),
    pair(word(28, "Decisive space advantage", NagGroup::Space)),
    pair(word(48, "Slight control of the center", NagGroup::Space)),
    pair(word(50, "Moderate control of the center", NagGroup::Space)),
    pair(word(52, "Decisive control of the center", NagGroup::Space)),
    pair(word(54, "Slight control of the kingside", NagGroup::Space)),
    pair(word(56, "Moderate control of the kingside", NagGroup::Space)),
    pair(word(58, "Decisive control of the kingside", NagGroup::Space)),
    pair(word(60, "Slight control of the queenside", NagGroup::Space)),
    pair(word(62, "Moderate control of the queenside", NagGroup::Space)),
    pair(word(64, "Decisive control of the queenside", NagGroup::Space)),
    // The king.
    pair(word(66, "Vulnerable first rank", NagGroup::King)),
    pair(word(68, "Well protected first rank", NagGroup::King)),
    pair(word(70, "Poorly protected king", NagGroup::King)),
    pair(word(72, "Well protected king", NagGroup::King)),
    pair(word(74, "Poorly placed king", NagGroup::King)),
    pair(word(76, "Well placed king", NagGroup::King)),
    // Pawns. ChessPad's signs speak of the position, not of a side: one code each.
    nag(249, "⯺", "Connected pawns", NagGroup::Pawns),
    nag(250, "⯻", "Isolated pawns", NagGroup::Pawns),
    nag(251, "⯼", "Doubled pawns", NagGroup::Pawns),
    nag(252, "⯽", "Passed pawn", NagGroup::Pawns),
    word(253, "Pawn majority", NagGroup::Pawns),
    pair(word(78, "Very weak pawn structure", NagGroup::Pawns)),
    pair(word(80, "Moderately weak pawn structure", NagGroup::Pawns)),
    pair(word(82, "Moderately strong pawn structure", NagGroup::Pawns)),
    pair(word(84, "Very strong pawn structure", NagGroup::Pawns)),
    // Pieces. The three bishop signs have no Unicode character (books set them in a chess font):
    // their glyphs are the ISChess stand-ins (see the table's notes), and the interface paints them.
    Nag { safe_glyph: false, suffix_only: true, ..nag(246, "⧉", "Pair of bishops", NagGroup::Pieces) },
    Nag { safe_glyph: false, suffix_only: true, ..nag(247, "□■", "Bishops of opposite colors", NagGroup::Pieces) },
    Nag { safe_glyph: false, suffix_only: true, ..nag(248, "■■", "Bishops of the same color", NagGroup::Pieces) },
    pair(word(86, "Poor knight placement", NagGroup::Pieces)),
    pair(word(88, "Good knight placement", NagGroup::Pieces)),
    pair(word(90, "Poor bishop placement", NagGroup::Pieces)),
    pair(word(92, "Good bishop placement", NagGroup::Pieces)),
    pair(word(94, "Poor rook placement", NagGroup::Pieces)),
    pair(word(96, "Good rook placement", NagGroup::Pieces)),
    pair(word(98, "Poor queen placement", NagGroup::Pieces)),
    pair(word(100, "Good queen placement", NagGroup::Pieces)),
    pair(word(102, "Poor piece coordination", NagGroup::Pieces)),
    pair(word(104, "Good piece coordination", NagGroup::Pieces)),
    // How each phase was played.
    pair(word(106, "Very poorly played opening", NagGroup::Phases)),
    pair(word(108, "Poorly played opening", NagGroup::Phases)),
    pair(word(110, "Well played opening", NagGroup::Phases)),
    pair(word(112, "Very well played opening", NagGroup::Phases)),
    pair(word(114, "Very poorly played middlegame", NagGroup::Phases)),
    pair(word(116, "Poorly played middlegame", NagGroup::Phases)),
    pair(word(118, "Well played middlegame", NagGroup::Phases)),
    pair(word(120, "Very well played middlegame", NagGroup::Phases)),
    pair(word(122, "Very poorly played endgame", NagGroup::Phases)),
    pair(word(124, "Poorly played endgame", NagGroup::Phases)),
    pair(word(126, "Well played endgame", NagGroup::Phases)),
    pair(word(128, "Very well played endgame", NagGroup::Phases)),
];

/// The diagram marker: a diagram of the position after the move goes here. It is not in the
/// table above, nor in the PGN standard's (codes 0–139): `$220` "Diagram" and `$221` "Diagram
/// (from Black)" are a de-facto extension, listed among the non-standard glyphs in the public
/// table of NAGs (the Wikipedia article "Numeric Annotation Glyphs", which attributes them to the
/// ChessPad editor). Other programs may mark diagrams differently; a code they do not know is
/// still kept and written back as it was.
pub const DIAGRAM: u16 = 220;

/// The diagram marker, the board seen from Black's side (see [`DIAGRAM`]).
pub const DIAGRAM_FROM_BLACK: u16 = 221;

/// Whether `code` marks a diagram ([`DIAGRAM`] or [`DIAGRAM_FROM_BLACK`]).
pub fn is_diagram(code: u16) -> bool {
    matches!(code, DIAGRAM | DIAGRAM_FROM_BLACK)
}

/// The entry for a PGN code (either side's).
pub fn by_code(code: u16) -> Option<&'static Nag> {
    NAGS.iter().find(|n| n.code == code || n.black_code == Some(code))
}

/// The entry for a canonical glyph: the one that owns it in text when two draw it alike (`○` is
/// `$26`, not `$238`). Never an entry without a glyph, whatever `glyph` is.
pub fn by_glyph(glyph: &str) -> Option<&'static Nag> {
    let drawn = |n: &&Nag| n.glyph == Some(glyph);
    NAGS.iter().filter(drawn).find(|n| n.read_glyph().is_some()).or_else(|| NAGS.iter().find(drawn))
}

/// `code` as printed in text: its glyph when that is safe alone, else `$code` (unknown codes and
/// the assessments without a sign too).
pub fn printed(code: u16) -> String {
    match by_code(code) {
        Some(Nag { glyph: Some(g), safe_glyph: true, .. }) => (*g).to_owned(),
        _ => format!("${code}"),
    }
}

/// `code` as shown to a reader: its glyph whatever it is (`N`, `RR`), else `$code`.
pub fn shown(code: u16) -> String {
    by_code(code).and_then(|n| n.glyph).map_or_else(|| format!("${code}"), str::to_owned)
}

/// Aliases that count only before a move. `¹` means "better is" before a move (`¹18.Rd1`) but is
/// a footnote mark glued after one (`Nf3¹`) almost everywhere else.
const PREFIX_ONLY_ALIASES: &[&str] = &["¹"];

/// Every alias with its canonical glyph, longest first so `+/=` wins over a shorter match.
fn aliases_longest_first() -> Vec<(&'static str, &'static str)> {
    let mut v: Vec<(&str, &str)> = NAGS
        .iter()
        .filter_map(|n| Some((n.aliases, n.read_glyph()?)))
        .flat_map(|(aliases, glyph)| aliases.iter().map(move |a| (*a, glyph)))
        .filter(|(a, _)| !a.is_empty())
        .collect();
    v.sort_by_key(|(a, _)| std::cmp::Reverse(a.chars().count()));
    v
}

/// Replace publisher symbol-font aliases with canonical glyphs. Returns the text and how many
/// symbols were replaced. Run it on raw text, **before** any Unicode normalization.
pub fn normalize_publisher_symbols(text: &str) -> (String, usize) {
    let aliases = aliases_longest_first();
    let mut out = String::with_capacity(text.len());
    let mut count = 0usize;
    let mut rest = text;
    let mut prev: Option<char> = None;
    while let Some(c) = rest.chars().next() {
        let hit = aliases
            .iter()
            .find(|(alias, _)| rest.starts_with(alias) && (!PREFIX_ONLY_ALIASES.contains(alias) || !prev.is_some_and(|p| p.is_ascii_alphanumeric())));
        match hit {
            Some((alias, glyph)) => {
                out.push_str(glyph);
                count += 1;
                rest = rest.get(alias.len()..).unwrap_or("");
                prev = glyph.chars().last();
            }
            None => {
                out.push(c);
                rest = rest.get(c.len_utf8()..).unwrap_or("");
                prev = Some(c);
            }
        }
    }
    (out, count)
}

/// The NAG codes in the annotation tail of a move (`!?±` gives 5 and 16), for the side that
/// made it. Aliases are resolved, `+` and `#` and anything unknown are skipped, and each code
/// appears once.
pub fn nags_from_suffix(suffix: &str, white_moved: bool) -> Vec<u16> {
    let mut pieces: Vec<(&str, &Nag)> = Vec::new();
    for n in NAGS {
        // A sign not read in text has no aliases either, and an empty one would match anywhere.
        let Some(glyph) = n.read_glyph().filter(|g| !g.is_empty()) else { continue };
        pieces.push((glyph, n));
        pieces.extend(n.aliases.iter().filter(|a| !a.is_empty()).map(|a| (*a, n)));
    }
    pieces.sort_by_key(|(p, _)| std::cmp::Reverse(p.chars().count()));
    let mut out: Vec<u16> = Vec::new();
    let mut rest = suffix.trim();
    while let Some(c) = rest.chars().next() {
        match pieces.iter().find(|(p, _)| rest.starts_with(p)) {
            Some((p, n)) => {
                let code = n.code_for(white_moved);
                if !out.contains(&code) {
                    out.push(code);
                }
                rest = rest.get(p.len()..).unwrap_or("");
            }
            None => rest = rest.get(c.len_utf8()..).unwrap_or(""),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_and_read_glyphs_are_unique() {
        let codes = |n: &Nag| [Some(n.code), n.black_code].into_iter().flatten().collect::<Vec<_>>();
        for (i, a) in NAGS.iter().enumerate() {
            for b in NAGS.iter().skip(i + 1) {
                for code in codes(a) {
                    assert!(!codes(b).contains(&code), "${code} twice");
                }
                if let (Some(x), Some(y)) = (a.read_glyph(), b.read_glyph()) {
                    assert_ne!(x, y, "${} and ${} read alike", a.code, b.code);
                }
            }
        }
        // The two glyphs drawn alike on purpose: the one not read is ChessPad's.
        let shared: Vec<u16> = NAGS.iter().filter(|n| n.glyph.is_some() && n.read_glyph().is_none() && n.code != 145).map(|n| n.code).collect();
        assert_eq!(shared, [238]);
    }

    #[test]
    fn the_table_has_every_standard_code_and_the_chesspad_ones() {
        let mut all: Vec<u16> = NAGS.iter().flat_map(|n| [Some(n.code), n.black_code]).flatten().collect();
        all.sort_unstable();
        let expected: Vec<u16> = (1..=146).chain(238..=255).collect();
        assert_eq!(all, expected);
        for code in expected {
            assert!(by_code(code).is_some(), "${code}");
        }
        assert!(by_code(0).is_none() && by_code(147).is_none() && by_code(DIAGRAM).is_none());
    }

    #[test]
    fn no_sign_is_empty_and_signless_codes_are_never_read() {
        for n in NAGS {
            assert!(!n.name.is_empty());
            assert!(n.glyph.is_none_or(|g| !g.is_empty() && g.trim() == g), "${}", n.code);
            assert!(n.aliases.iter().all(|a| !a.is_empty()), "${}", n.code);
            if n.glyph.is_none() {
                assert!(!n.safe_glyph && !n.suffix_only && n.aliases.is_empty(), "${}", n.code);
                assert_eq!(n.insert_text(), format!("${}", n.code));
                assert_eq!(printed(n.code), format!("${}", n.code));
                assert_eq!(shown(n.code), format!("${}", n.code));
            }
        }
        assert!(by_glyph("").is_none());
        assert_eq!(nags_from_suffix(" ", true), Vec::<u16>::new());
        assert_eq!(normalize_publisher_symbols("a b").0, "a b");
    }

    #[test]
    fn side_dependent_codes() {
        let zugzwang = by_glyph("⨀").unwrap();
        assert_eq!(zugzwang.code_for(true), 22);
        assert_eq!(zugzwang.code_for(false), 23);
        assert!(!zugzwang.side_is_mover);
        assert_eq!(by_code(23).and_then(|n| n.glyph), Some("⨀"));
        assert_eq!(by_glyph("±").unwrap().code_for(false), 16);
        assert_eq!(by_glyph("N").unwrap().insert_text(), "$146");
        assert_eq!(by_glyph("±").unwrap().insert_text(), "±");
        // The standard's pairs: White's even code, Black's the next one.
        for (white, black) in [(24, 25), (42, 43), (66, 67), (104, 105), (128, 129), (136, 137)] {
            let n = by_code(black).unwrap();
            assert_eq!((n.code, n.code_for(true), n.code_for(false)), (white, white, black));
        }
        // Codes for one side only, and ChessPad's, have no pair.
        for code in [8, 11, 20, 21, 144, 238, 246, 253, 255] {
            assert_eq!(by_code(code).unwrap().code_for(false), code);
        }
    }

    #[test]
    fn publisher_symbols_become_glyphs_before_normalization() {
        assert_eq!(normalize_publisher_symbols("Rad8³"), ("Rad8⩱".to_owned(), 1));
        assert_eq!(normalize_publisher_symbols("Nxe4!µ"), ("Nxe4!∓".to_owned(), 1));
        assert_eq!(normalize_publisher_symbols("22.a3+–"), ("22.a3+-".to_owned(), 1));
        assert_eq!(normalize_publisher_symbols("+/= then =/+"), ("⩲ then ⩱".to_owned(), 2));
        assert_eq!(normalize_publisher_symbols("plain text"), ("plain text".to_owned(), 0));
        assert_eq!(normalize_publisher_symbols("⯹"), ("©".to_owned(), 1), "the Unicode compensation sign");
    }

    #[test]
    fn superscript_one_is_better_is_only_before_a_move() {
        assert_eq!(normalize_publisher_symbols("¹18.Rd1").0, "⌓18.Rd1");
        assert_eq!(normalize_publisher_symbols("Nf3¹").0, "Nf3¹");
        assert_eq!(normalize_publisher_symbols("(¹18.Rd1)").0, "(⌓18.Rd1)");
    }

    #[test]
    fn suffixes_give_codes() {
        assert_eq!(nags_from_suffix("!?±", true), vec![5, 16]);
        assert_eq!(nags_from_suffix("³", false), vec![15]);
        assert_eq!(nags_from_suffix("!µ", false), vec![1, 17]);
        assert_eq!(nags_from_suffix("+-", true), vec![18]);
        assert_eq!(nags_from_suffix("N", true), vec![146]);
        assert_eq!(nags_from_suffix("⨀", false), vec![23]);
        assert_eq!(nags_from_suffix("#!", true), vec![1]);
        assert_eq!(nags_from_suffix("!!", true), vec![3]);
        assert_eq!(nags_from_suffix("", true), Vec::<u16>::new());
        // The new signs.
        assert_eq!(nags_from_suffix("○", true), vec![26]);
        assert_eq!(nags_from_suffix("○", false), vec![27], "space by the side that moved");
        assert_eq!(nags_from_suffix("⇔⇗⊞⟫⟪✕⊥∟⯾", true), vec![239, 240, 241, 242, 243, 244, 245, 254, 255]);
        assert_eq!(nags_from_suffix("⯺⯻⯼⯽", false), vec![249, 250, 251, 252]);
        assert_eq!(nags_from_suffix("!⯹", false), vec![1, 45], "compensation's Unicode sign");
    }

    #[test]
    fn close_signs_stay_apart() {
        // `==` is one sign, `=` another; a promotion is the move's text, not a sign.
        assert_eq!(nags_from_suffix("==", true), vec![144]);
        assert_eq!(nags_from_suffix("=", true), vec![10]);
        assert_eq!(nags_from_suffix("=+", true), vec![15]);
        assert_eq!(nags_from_suffix("!=", true), vec![1, 10]);
        assert_eq!(by_glyph("=").map(|n| n.code), Some(10));
        assert_eq!(by_glyph("==").map(|n| n.code), Some(144));
        // A file is not counterplay.
        assert_eq!(nags_from_suffix("⇔", true), vec![239]);
        assert_eq!(nags_from_suffix("⇄", true), vec![132]);
        assert_eq!(nags_from_suffix("⇆", false), vec![133]);
        // The capture sign is no annotation; the weak point's cross is.
        assert_eq!(nags_from_suffix("×", true), Vec::<u16>::new());
        assert!(by_glyph("×").is_none());
        assert_eq!(by_glyph("✕").map(|n| n.code), Some(244));
        assert_eq!(normalize_publisher_symbols("Nxe5 N×e5").0, "Nxe5 N×e5");
        // The ring: the standard's space, ChessPad's drawn alike but not read.
        assert_eq!(by_glyph("○").map(|n| n.code), Some(26));
        assert_eq!(by_code(238).and_then(|n| n.glyph), Some("○"));
        assert_eq!(by_code(238).map(Nag::insert_text).as_deref(), Some("$238"));
        // The bishop stand-ins: `□■` is one sign, not the only move and a square.
        assert_eq!(nags_from_suffix("□■", true), vec![247]);
        assert_eq!(nags_from_suffix("□", true), vec![7]);
        assert_eq!(nags_from_suffix("■■⧉", true), vec![248, 246]);
    }

    #[test]
    fn printed_and_shown_text() {
        assert_eq!(printed(16), "±");
        assert_eq!(printed(146), "$146", "N alone is the knight");
        assert_eq!(shown(146), "N");
        assert_eq!(printed(27), "○");
        assert_eq!(printed(238), "$238");
        assert_eq!(shown(238), "○");
        assert_eq!(printed(24), "$24");
        assert_eq!(printed(999), "$999");
        assert_eq!(shown(0), "$0");
    }

    proptest::proptest! {
        #[test]
        fn never_panics(s in "\\PC{0,60}") {
            let _ = normalize_publisher_symbols(&s);
            let _ = nags_from_suffix(&s, true);
            let _ = by_glyph(&s);
        }

        /// Every code PGN has, either side's, is found, and keeps its number through `$n`.
        #[test]
        fn every_code_round_trips(code in 0u16..=300, white in proptest::bool::ANY) {
            let text = printed(code);
            match by_code(code) {
                Some(n) => {
                    proptest::prop_assert!(n.code == code || n.black_code == Some(code));
                    match n.read_glyph().filter(|_| n.safe_glyph) {
                        // A sign read back gives the code of the side that made the move.
                        Some(g) => {
                            proptest::prop_assert_eq!(&text, g);
                            proptest::prop_assert_eq!(nags_from_suffix(&text, white), vec![n.code_for(white)]);
                        }
                        None => proptest::prop_assert_eq!(text, format!("${code}")),
                    }
                }
                None => proptest::prop_assert_eq!(text, format!("${code}")),
            }
        }
    }
}
