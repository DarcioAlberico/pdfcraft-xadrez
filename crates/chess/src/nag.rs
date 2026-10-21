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
//!   end of a move (`12.Nf4N`).

/// What an annotation talks about, for grouping in a palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NagGroup {
    Move,
    Evaluation,
    Dynamics,
    Intent,
}

/// One annotation symbol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Nag {
    /// The PGN code, `$code`. For side-dependent symbols, the code when White made the move.
    pub code: u16,
    /// The code when Black made the move, for symbols whose number depends on the side
    /// (zugzwang is `$22` for White and `$23` for Black, with one glyph).
    pub black_code: Option<u16>,
    /// The canonical printed glyph.
    pub glyph: &'static str,
    /// English name; the interface translates it.
    pub name: &'static str,
    pub group: NagGroup,
    /// `true`: the symbol speaks of the side that made the move. `false` (zugzwang): of the side
    /// now to move.
    pub side_is_mover: bool,
    /// `false` when the glyph is too ambiguous to look for in free text (`N` is the knight).
    pub safe_glyph: bool,
    /// Recognised only glued to the end of a move.
    pub suffix_only: bool,
    /// Other spellings found in books: publisher symbol fonts, Informator ASCII, OCR leftovers.
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

    /// What a palette inserts: the glyph, or `$n` when the glyph is unsafe.
    pub fn insert_text(&self) -> String {
        if self.safe_glyph { self.glyph.to_owned() } else { format!("${}", self.code) }
    }
}

const fn nag(code: u16, glyph: &'static str, name: &'static str, group: NagGroup) -> Nag {
    Nag { code, black_code: None, glyph, name, group, side_is_mover: true, safe_glyph: true, suffix_only: false, aliases: &[] }
}

const fn with_aliases(n: Nag, aliases: &'static [&'static str]) -> Nag {
    Nag { aliases, ..n }
}

const fn for_black(n: Nag, black_code: u16) -> Nag {
    Nag { black_code: Some(black_code), ..n }
}

/// The table, in display order. The publisher aliases come from the key to symbols printed in
/// chess books, checked against the PGN files their publishers distribute with them.
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
    for_black(with_aliases(nag(44, "©", "Compensation for the material", NagGroup::Evaluation), &["=/∞"]), 45),
    // Dynamics.
    Nag { side_is_mover: false, ..for_black(with_aliases(nag(22, "⨀", "Zugzwang", NagGroup::Dynamics), &["⊙", "ʘ"]), 23) },
    for_black(with_aliases(nag(32, "⟳", "Development advantage", NagGroup::Dynamics), &["‰"]), 33),
    for_black(with_aliases(nag(36, "↑", "With the initiative", NagGroup::Dynamics), &["ƒ"]), 37),
    for_black(with_aliases(nag(40, "→", "With an attack", NagGroup::Dynamics), &["‚"]), 41),
    for_black(with_aliases(nag(132, "⇆", "With counterplay", NagGroup::Dynamics), &["„", "⇄"]), 133),
    for_black(with_aliases(nag(138, "⨁", "Time trouble", NagGroup::Dynamics), &["⊕"]), 139),
    // Intent. The ellipsis is deliberately not an alias of ∆ although some symbol keys say so:
    // nearly every chess PDF uses it for Black's move numbers (`7…h6`).
    with_aliases(nag(140, "∆", "With the idea", NagGroup::Intent), &["Δ", "△"]),
    nag(141, "∇", "Aimed against", NagGroup::Intent),
    with_aliases(nag(142, "⌓", "Better is", NagGroup::Intent), &["¹"]),
    nag(143, "≤", "Worse is", NagGroup::Intent),
    Nag { safe_glyph: false, ..nag(145, "RR", "Editorial comment", NagGroup::Intent) },
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

/// The entry for a canonical glyph.
pub fn by_glyph(glyph: &str) -> Option<&'static Nag> {
    NAGS.iter().find(|n| n.glyph == glyph)
}

/// Aliases that count only before a move. `¹` means "better is" before a move (`¹18.Rd1`) but is
/// a footnote mark glued after one (`Nf3¹`) almost everywhere else.
const PREFIX_ONLY_ALIASES: &[&str] = &["¹"];

/// Every alias with its canonical glyph, longest first so `+/=` wins over a shorter match.
fn aliases_longest_first() -> Vec<(&'static str, &'static str)> {
    let mut v: Vec<(&str, &str)> = NAGS.iter().flat_map(|n| n.aliases.iter().map(move |a| (*a, n.glyph))).collect();
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
        if n.safe_glyph || n.suffix_only {
            pieces.push((n.glyph, n));
        }
        for a in n.aliases {
            pieces.push((a, n));
        }
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
    fn codes_and_glyphs_are_unique() {
        for (i, a) in NAGS.iter().enumerate() {
            for b in NAGS.iter().skip(i + 1) {
                assert_ne!(a.glyph, b.glyph);
                assert_ne!(a.code, b.code);
                assert!(a.black_code.is_none() || a.black_code != b.black_code);
            }
        }
    }

    #[test]
    fn side_dependent_codes() {
        let zugzwang = by_glyph("⨀").unwrap();
        assert_eq!(zugzwang.code_for(true), 22);
        assert_eq!(zugzwang.code_for(false), 23);
        assert!(!zugzwang.side_is_mover);
        assert_eq!(by_code(23).map(|n| n.glyph), Some("⨀"));
        assert_eq!(by_glyph("±").unwrap().code_for(false), 16);
        assert_eq!(by_glyph("N").unwrap().insert_text(), "$146");
        assert_eq!(by_glyph("±").unwrap().insert_text(), "±");
    }

    #[test]
    fn publisher_symbols_become_glyphs_before_normalization() {
        assert_eq!(normalize_publisher_symbols("Rad8³"), ("Rad8⩱".to_owned(), 1));
        assert_eq!(normalize_publisher_symbols("Nxe4!µ"), ("Nxe4!∓".to_owned(), 1));
        assert_eq!(normalize_publisher_symbols("22.a3+–"), ("22.a3+-".to_owned(), 1));
        assert_eq!(normalize_publisher_symbols("+/= then =/+"), ("⩲ then ⩱".to_owned(), 2));
        assert_eq!(normalize_publisher_symbols("plain text"), ("plain text".to_owned(), 0));
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
    }

    proptest::proptest! {
        #[test]
        fn never_panics(s in "\\PC{0,60}") {
            let _ = normalize_publisher_symbols(&s);
            let _ = nags_from_suffix(&s, true);
        }
    }
}
