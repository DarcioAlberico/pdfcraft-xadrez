//! A diagram in words, for a reader who cannot see it (plan A8 P9b and P9d): the text
//! alternative of an exported diagram, in the book's language.
//!
//! [`title`] names the diagram, its page and the side to move (`Diagrama 1, pág. 17: brancas
//! jogam`); [`pieces`] lists each side's pieces with the language's letters, the king first and
//! the pawns last (`Brancas: Rg1, Df3; peões a2, b2. Pretas: Rg8; peões g7.`). [`describe`]
//! gives both, and [`not_read`] the words for a diagram with no position. [`Naming::Words`]
//! names the pieces in words instead (`Brancas: rei g1, dama f3; …`), which a speech synthesiser
//! says better than a letter run into a square (plan A9-X2).

use crate::fen;
use crate::origin::words;
use crate::placement::{Color, PieceKind, Placement};
use crate::san::Lang;

/// How a description names the pieces: by the language's letters (`Kg1`, the default) or in
/// words (`king g1`; plan A9-X2), for a screen reader whose speech says `Kg1` as one word.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Naming {
    #[default]
    Letters,
    Words,
}

impl Naming {
    /// The name the tools and the settings use (`letters`, `words`).
    pub fn name(self) -> &'static str {
        match self {
            Naming::Letters => "letters",
            Naming::Words => "words",
        }
    }

    /// The naming called `name` ([`Naming::name`]).
    pub fn from_name(name: &str) -> Option<Naming> {
        match name {
            "letters" => Some(Naming::Letters),
            "words" => Some(Naming::Words),
            _ => None,
        }
    }
}

/// The words a description needs that the origin's comment does not.
struct Sides {
    white: &'static str,
    black: &'static str,
    white_to_move: &'static str,
    black_to_move: &'static str,
    pawns: &'static str,
    not_read: &'static str,
    /// The book did not say who is to move (plan A9-L6b, A1-14): the reader is not told White.
    side_not_given: &'static str,
}

fn sides(lang: Lang) -> Sides {
    let s = |white, black, white_to_move, black_to_move, pawns, not_read, side_not_given| Sides {
        white,
        black,
        white_to_move,
        black_to_move,
        pawns,
        not_read,
        side_not_given,
    };
    match lang {
        Lang::En => s("White", "Black", "White to move", "Black to move", "pawns", "not read", "side to move not given"),
        Lang::Pt => s("Brancas", "Pretas", "brancas jogam", "pretas jogam", "peões", "não lido", "lado não informado"),
        Lang::De => s("Weiß", "Schwarz", "Weiß am Zug", "Schwarz am Zug", "Bauern", "nicht gelesen", "Zugrecht nicht angegeben"),
        Lang::Es => s("Blancas", "Negras", "juegan las blancas", "juegan las negras", "peones", "no leído", "turno no indicado"),
        Lang::Fr => s("Blancs", "Noirs", "les Blancs jouent", "les Noirs jouent", "pions", "non lu", "trait non indiqué"),
        Lang::It => s("Bianco", "Nero", "muove il Bianco", "muove il Nero", "pedoni", "non letto", "tratto non indicato"),
        Lang::Ru => s("Белые", "Чёрные", "ход белых", "ход чёрных", "пешки", "не распознана", "очерёдность хода не указана"),
        Lang::Nl => s("Wit", "Zwart", "wit aan zet", "zwart aan zet", "pionnen", "niet gelezen", "wie aan zet is niet vermeld"),
        Lang::Pl => s("Białe", "Czarne", "ruch białych", "ruch czarnych", "pionki", "nieodczytany", "strona na posunięciu nie podana"),
        Lang::Cs => s("Bílý", "Černý", "na tahu je bílý", "na tahu je černý", "pěšci", "nepřečtený", "kdo je na tahu, neuvedeno"),
        Lang::Hu => s("Világos", "Sötét", "világos lép", "sötét lép", "gyalogok", "nem olvasott", "a lépés joga nincs megadva"),
        Lang::Da => s("Hvid", "Sort", "hvid i trækket", "sort i trækket", "bønder", "ikke læst", "trækket ikke angivet"),
        Lang::Sv => s("Vit", "Svart", "vit drar", "svart drar", "bönder", "inte läst", "draget inte angivet"),
        Lang::No => s("Hvit", "Svart", "hvit i trekket", "svart i trekket", "bønder", "ikke lest", "trekket ikke oppgitt"),
        Lang::Fi => s("Valkea", "Musta", "valkea siirtää", "musta siirtää", "sotilaat", "ei luettu", "siirtovuoroa ei mainita"),
        Lang::Ro => s("Albul", "Negrul", "mută albul", "mută negrul", "pioni", "necitit", "la mutare: nespecificat"),
        Lang::Is => s("Hvítt", "Svart", "hvítur á leik", "svartur á leik", "peð", "ekki lesin", "ekki tilgreint hver á leik"),
        Lang::Ca => s("Blanques", "Negres", "juguen les blanques", "juguen les negres", "peons", "no llegit", "torn no indicat"),
    }
}

/// The pieces in words, king, queen, rook, bishop and knight, as a list names them
/// ([`Naming::Words`]): the nouns the language's letters stand for.
fn piece_words(lang: Lang) -> [&'static str; 5] {
    match lang {
        Lang::En => ["king", "queen", "rook", "bishop", "knight"],
        Lang::Pt => ["rei", "dama", "torre", "bispo", "cavalo"],
        Lang::De => ["König", "Dame", "Turm", "Läufer", "Springer"],
        Lang::Es => ["rey", "dama", "torre", "alfil", "caballo"],
        Lang::Fr => ["roi", "dame", "tour", "fou", "cavalier"],
        Lang::It => ["re", "donna", "torre", "alfiere", "cavallo"],
        Lang::Ru => ["король", "ферзь", "ладья", "слон", "конь"],
        Lang::Nl => ["koning", "dame", "toren", "loper", "paard"],
        Lang::Pl => ["król", "hetman", "wieża", "goniec", "skoczek"],
        Lang::Cs => ["král", "dáma", "věž", "střelec", "jezdec"],
        Lang::Hu => ["király", "vezér", "bástya", "futó", "huszár"],
        Lang::Da => ["konge", "dronning", "tårn", "løber", "springer"],
        Lang::Sv => ["kung", "dam", "torn", "löpare", "springare"],
        Lang::No => ["konge", "dronning", "tårn", "løper", "springer"],
        Lang::Fi => ["kuningas", "kuningatar", "torni", "lähetti", "ratsu"],
        Lang::Ro => ["rege", "damă", "turn", "nebun", "cal"],
        Lang::Is => ["kóngur", "drottning", "hrókur", "biskup", "riddari"],
        Lang::Ca => ["rei", "dama", "torre", "alfil", "cavall"],
    }
}

/// The first letter in upper case.
fn capital(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// `Diagram 1, page 17`: diagram `number` (from 1) of the page printed `page`.
fn name(number: usize, page: &str, lang: Lang) -> String {
    let w = words(lang);
    format!("{} {number}, {} {page}", capital(w.diagram), w.page.to_lowercase())
}

/// `Diagram 1, page 17: White to move`; without the side when the diagram does not say it.
pub fn title(number: usize, page: &str, side: Option<Color>, lang: Lang) -> String {
    let s = sides(lang);
    match side {
        Some(Color::White) => format!("{}: {}", name(number, page, lang), s.white_to_move),
        Some(Color::Black) => format!("{}: {}", name(number, page, lang), s.black_to_move),
        None => name(number, page, lang),
    }
}

/// `White to move` (`brancas jogam`) in `lang`, as [`title`] says it: what an exercise asks
/// first (plan A9-N10).
pub fn to_move(side: Color, lang: Lang) -> &'static str {
    let s = sides(lang);
    if side == Color::White { s.white_to_move } else { s.black_to_move }
}

/// `Diagram 1, page 17: side to move not given`: a diagram whose side to move the book did not
/// say and nothing else settled (plan A9-L6b, A1-14): the reader is not told White, which was only
/// assumed.
pub fn title_side_not_given(number: usize, page: &str, lang: Lang) -> String {
    format!("{}: {}", name(number, page, lang), sides(lang).side_not_given)
}

/// Each side's pieces, the king first and the pawns last, each kind's squares from a1 to h8:
/// `White: Kg1, Qf3; pawns a2, b2. Black: Kg8; pawns g7.` A side with no piece is left out.
pub fn pieces(placement: &Placement, lang: Lang) -> String {
    pieces_named(placement, lang, Naming::Letters)
}

/// [`pieces`] with the pieces named as `naming` says: in words, `White: king g1, queen f3; pawns
/// a2, b2.`
pub fn pieces_named(placement: &Placement, lang: Lang, naming: Naming) -> String {
    let s = sides(lang);
    let mut out = Vec::new();
    for (color, side) in [(Color::White, s.white), (Color::Black, s.black)] {
        let mut officers = Vec::new();
        let mut pawns = Vec::new();
        for kind in [PieceKind::King, PieceKind::Queen, PieceKind::Rook, PieceKind::Bishop, PieceKind::Knight, PieceKind::Pawn] {
            let mut squares: Vec<_> = placement.pieces().filter(|(_, p)| p.color == color && p.kind == kind).map(|(sq, _)| sq).collect();
            squares.sort_by_key(|sq| (sq.file(), sq.rank()));
            for sq in squares {
                if kind == PieceKind::Pawn {
                    pawns.push(sq.to_string());
                } else if naming == Naming::Words {
                    officers.push(format!("{} {sq}", piece_name(lang, kind)));
                } else {
                    officers.push(format!("{}{sq}", lang.letter(kind)));
                }
            }
        }
        let mut parts = Vec::new();
        if !officers.is_empty() {
            parts.push(officers.join(", "));
        }
        if !pawns.is_empty() {
            parts.push(format!("{} {}", s.pawns, pawns.join(", ")));
        }
        if !parts.is_empty() {
            out.push(format!("{side}: {}.", parts.join("; ")));
        }
    }
    out.join(" ")
}

/// A piece's name in words ([`piece_words`]); the pawns are listed under their own word.
fn piece_name(lang: Lang, kind: PieceKind) -> &'static str {
    let [king, queen, rook, bishop, knight] = piece_words(lang);
    match kind {
        PieceKind::King => king,
        PieceKind::Queen => queen,
        PieceKind::Rook => rook,
        PieceKind::Bishop => bishop,
        PieceKind::Knight => knight,
        PieceKind::Pawn => sides(lang).pawns,
    }
}

/// [`title`] and [`pieces`] of the position `fen`: `Diagram 1, page 17: White to move. White:
/// Kg1…`. Only the title when the FEN cannot be read.
pub fn describe(number: usize, page: &str, fen: &str, lang: Lang) -> String {
    match fen::parse(fen) {
        Ok(p) => {
            let list = pieces(&p.placement, lang);
            let head = title(number, page, p.side_to_move, lang);
            if list.is_empty() { format!("{head}.") } else { format!("{head}. {list}") }
        }
        Err(_) => format!("{}.", name(number, page, lang)),
    }
}

/// [`describe`] of a diagram whose side to move may be only assumed: when `side_known` is false,
/// the title says it was not given ([`title_side_not_given`]) instead of `White to move`.
pub fn describe_side(number: usize, page: &str, fen: &str, side_known: bool, lang: Lang) -> String {
    describe_side_named(number, page, fen, side_known, lang, Naming::Letters)
}

/// [`describe_side`] with the pieces named as `naming` says ([`pieces_named`]).
pub fn describe_side_named(number: usize, page: &str, fen: &str, side_known: bool, lang: Lang, naming: Naming) -> String {
    match fen::parse(fen) {
        Ok(p) => {
            let list = pieces_named(&p.placement, lang, naming);
            let head = if side_known { title(number, page, p.side_to_move, lang) } else { title_side_not_given(number, page, lang) };
            if list.is_empty() { format!("{head}.") } else { format!("{head}. {list}") }
        }
        Err(_) => format!("{}.", name(number, page, lang)),
    }
}

/// `Page 17` (`Pág. 17`): the page printed `page`, as a table of contents names it.
pub fn page(page: &str, lang: Lang) -> String {
    format!("{} {page}", words(lang).page)
}

/// `Diagram 1, page 17 (not read)`: a diagram no position was read from.
pub fn not_read(number: usize, page: &str, lang: Lang) -> String {
    format!("{} ({})", name(number, page, lang), sides(lang).not_read)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEN: &str = "6k1/6p1/8/8/8/5Q2/PP6/6K1 w - - 0 1";

    #[test]
    fn a_diagram_is_named_and_its_pieces_listed_in_the_books_language() {
        assert_eq!(describe(1, "17", FEN, Lang::En), "Diagram 1, page 17: White to move. White: Kg1, Qf3; pawns a2, b2. Black: Kg8; pawns g7.");
        assert_eq!(describe(1, "17", FEN, Lang::Pt), "Diagrama 1, pág. 17: brancas jogam. Brancas: Rg1, Df3; peões a2, b2. Pretas: Rg8; peões g7.");
        assert_eq!(
            describe(2, "xvii", &FEN.replace(" w ", " b "), Lang::De),
            "Diagramm 2, s. xvii: Schwarz am Zug. Weiß: Kg1, Df3; Bauern a2, b2. Schwarz: Kg8; Bauern g7."
        );
        // No side said, an empty board, a FEN that cannot be read.
        assert_eq!(title(3, "4", None, Lang::En), "Diagram 3, page 4");
        assert_eq!(describe(1, "1", "8/8/8/8/8/8/8/8 w - - 0 1", Lang::En), "Diagram 1, page 1: White to move.");
        assert_eq!(describe(1, "1", "not a fen", Lang::En), "Diagram 1, page 1.");
        assert_eq!(not_read(1, "17", Lang::Pt), "Diagrama 1, pág. 17 (não lido)");
        assert_eq!((page("xvii", Lang::En), page("17", Lang::Pt)), ("Page xvii".to_owned(), "Pág. 17".to_owned()));
    }

    #[test]
    fn every_language_has_its_words() {
        for lang in Lang::ALL {
            let text = describe(1, "2", FEN, lang);
            assert!(text.contains("1") && text.contains("2") && text.contains("g7"), "{lang:?}: {text}");
            assert!(!not_read(1, "2", lang).contains("()"), "{lang:?}");
            // In words, each piece is a word of its own before its square, and no letter is run
            // into one (plan A9-X2).
            let words = describe_side_named(1, "2", FEN, true, lang, Naming::Words);
            assert!(
                words.contains(" g1") && words.contains(" f3") && !words.contains(&format!("{}g1", lang.letter(PieceKind::King))),
                "{lang:?}: {words}"
            );
        }
    }

    /// Plan A9-X2: the pieces in words for a speech synthesiser, the pawns as before; the letters
    /// stay the default.
    #[test]
    fn the_pieces_can_be_named_in_words() {
        let words = |lang| describe_side_named(1, "17", FEN, true, lang, Naming::Words);
        assert_eq!(words(Lang::En), "Diagram 1, page 17: White to move. White: king g1, queen f3; pawns a2, b2. Black: king g8; pawns g7.");
        assert_eq!(words(Lang::Pt), "Diagrama 1, pág. 17: brancas jogam. Brancas: rei g1, dama f3; peões a2, b2. Pretas: rei g8; peões g7.");
        assert_eq!(
            describe_side_named(1, "17", FEN, false, Lang::En, Naming::Words),
            "Diagram 1, page 17: side to move not given. White: king g1, queen f3; pawns a2, b2. Black: king g8; pawns g7."
        );
        assert_eq!(describe_side_named(1, "17", FEN, true, Lang::En, Naming::Letters), describe(1, "17", FEN, Lang::En));
        assert_eq!(Naming::default(), Naming::Letters);
        for n in [Naming::Letters, Naming::Words] {
            assert_eq!(Naming::from_name(n.name()), Some(n));
        }
        assert_eq!(Naming::from_name("Words"), None);
    }
}
