//! A diagram in words, for a reader who cannot see it (plan A8 P9b and P9d): the text
//! alternative of an exported diagram, in the book's language.
//!
//! [`title`] names the diagram, its page and the side to move (`Diagrama 1, pág. 17: brancas
//! jogam`); [`pieces`] lists each side's pieces with the language's letters, the king first and
//! the pawns last (`Brancas: Rg1, Df3; peões a2, b2. Pretas: Rg8; peões g7.`). [`describe`]
//! gives both, and [`not_read`] the words for a diagram with no position.

use crate::fen;
use crate::origin::words;
use crate::placement::{Color, PieceKind, Placement};
use crate::san::Lang;

/// The words a description needs that the origin's comment does not.
struct Sides {
    white: &'static str,
    black: &'static str,
    white_to_move: &'static str,
    black_to_move: &'static str,
    pawns: &'static str,
    not_read: &'static str,
}

fn sides(lang: Lang) -> Sides {
    let s = |white, black, white_to_move, black_to_move, pawns, not_read| Sides { white, black, white_to_move, black_to_move, pawns, not_read };
    match lang {
        Lang::En => s("White", "Black", "White to move", "Black to move", "pawns", "not read"),
        Lang::Pt => s("Brancas", "Pretas", "brancas jogam", "pretas jogam", "peões", "não lido"),
        Lang::De => s("Weiß", "Schwarz", "Weiß am Zug", "Schwarz am Zug", "Bauern", "nicht gelesen"),
        Lang::Es => s("Blancas", "Negras", "juegan las blancas", "juegan las negras", "peones", "no leído"),
        Lang::Fr => s("Blancs", "Noirs", "les Blancs jouent", "les Noirs jouent", "pions", "non lu"),
        Lang::It => s("Bianco", "Nero", "muove il Bianco", "muove il Nero", "pedoni", "non letto"),
        Lang::Ru => s("Белые", "Чёрные", "ход белых", "ход чёрных", "пешки", "не распознана"),
        Lang::Nl => s("Wit", "Zwart", "wit aan zet", "zwart aan zet", "pionnen", "niet gelezen"),
        Lang::Pl => s("Białe", "Czarne", "ruch białych", "ruch czarnych", "pionki", "nieodczytany"),
        Lang::Cs => s("Bílý", "Černý", "na tahu je bílý", "na tahu je černý", "pěšci", "nepřečtený"),
        Lang::Hu => s("Világos", "Sötét", "világos lép", "sötét lép", "gyalogok", "nem olvasott"),
        Lang::Da => s("Hvid", "Sort", "hvid i trækket", "sort i trækket", "bønder", "ikke læst"),
        Lang::Sv => s("Vit", "Svart", "vit drar", "svart drar", "bönder", "inte läst"),
        Lang::No => s("Hvit", "Svart", "hvit i trekket", "svart i trekket", "bønder", "ikke lest"),
        Lang::Fi => s("Valkea", "Musta", "valkea siirtää", "musta siirtää", "sotilaat", "ei luettu"),
        Lang::Ro => s("Albul", "Negrul", "mută albul", "mută negrul", "pioni", "necitit"),
        Lang::Is => s("Hvítt", "Svart", "hvítur á leik", "svartur á leik", "peð", "ekki lesin"),
        Lang::Ca => s("Blanques", "Negres", "juguen les blanques", "juguen les negres", "peons", "no llegit"),
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

/// Each side's pieces, the king first and the pawns last, each kind's squares from a1 to h8:
/// `White: Kg1, Qf3; pawns a2, b2. Black: Kg8; pawns g7.` A side with no piece is left out.
pub fn pieces(placement: &Placement, lang: Lang) -> String {
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
    }

    #[test]
    fn every_language_has_its_words() {
        for lang in Lang::ALL {
            let text = describe(1, "2", FEN, lang);
            assert!(text.contains("1") && text.contains("2") && text.contains("g7"), "{lang:?}: {text}");
            assert!(!not_read(1, "2", lang).contains("()"), "{lang:?}");
        }
    }
}
