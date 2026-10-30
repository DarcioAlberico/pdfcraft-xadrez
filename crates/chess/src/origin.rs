//! Where in a book a game or a position came from, as PGN writes it (plan A8 P8a).
//!
//! One type for both PGN writers (the document's export and the board's game), so the same
//! origin always gives the same tags in the same order and the same opening comment.
//!
//! The tags are only ever added to: `PdfPage` stays the physical page, counted from 1, as the
//! `chess_export_pgn` contract has it; `PdfPageLabel` is the number printed on the page when it
//! differs, and `SourceTitle` the book's title. A game with no moves made for a diagram also gets
//! the title as its `Event`, with `Round "-"`; a game from the book keeps its own event.
//!
//! ChessBase and others drop tags they do not know, so the origin is also written as the game's
//! first comment, in the book's language: `Pág. 17 · diagrama 1 de 2`.

use crate::san::Lang;

/// The tags an origin writes, in the order they are written.
pub const ORIGIN_TAGS: [&str; 5] = ["PdfPage", "PdfPageLabel", "PdfDiagram", "PdfPuzzle", "SourceTitle"];

/// Longest title or page label written, in characters.
pub const MAX_ORIGIN_CHARS: usize = 256;

/// Where a game or a position came from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PgnOrigin {
    /// The book's title, when it has one.
    pub book_title: Option<String>,
    /// The page, counted from 0.
    pub page_index: u32,
    /// The number printed on the page ("xvii"), when the document labels its pages.
    pub page_label: Option<String>,
    /// The diagram on that page, counted from 1.
    pub diagram: Option<usize>,
    /// How many diagrams the page has.
    pub of: Option<usize>,
    /// The book's number for the puzzle ("Exercise 7").
    pub puzzle: Option<u32>,
    /// The book's language, for the comment.
    pub lang: Option<Lang>,
}

impl PgnOrigin {
    /// The physical page, counted from 1.
    pub fn page_number(&self) -> u64 {
        u64::from(self.page_index).saturating_add(1)
    }

    /// The printed label, when there is one and it is not just the physical number.
    fn label(&self) -> Option<String> {
        let label = clean(self.page_label.as_deref()?);
        (!label.is_empty() && label != self.page_number().to_string()).then_some(label)
    }

    fn title(&self) -> Option<String> {
        self.book_title.as_deref().map(clean).filter(|t| !t.is_empty())
    }

    /// The tags, in [`ORIGIN_TAGS`] order, leaving out the ones the origin has no value for.
    pub fn tags(&self) -> Vec<(String, String)> {
        let mut out = vec![("PdfPage".to_owned(), self.page_number().to_string())];
        if let Some(label) = self.label() {
            out.push(("PdfPageLabel".to_owned(), label));
        }
        if let Some(d) = self.diagram {
            out.push(("PdfDiagram".to_owned(), d.to_string()));
        }
        if let Some(p) = self.puzzle {
            out.push(("PdfPuzzle".to_owned(), p.to_string()));
        }
        if let Some(title) = self.title() {
            out.push(("SourceTitle".to_owned(), title));
        }
        out
    }

    /// For a game with no moves made for a diagram: its `Event` (the book's title) and `Round`.
    /// `None` without a title, leaving the roster as it is.
    pub fn position_roster(&self) -> Option<(String, String)> {
        self.title().map(|t| (t, "-".to_owned()))
    }

    /// The game's first comment: `Page 17 · diagram 1 of 2` in the book's language (English
    /// when it is not known), with the printed page number when there is one.
    pub fn comment(&self) -> String {
        let page = self.label().unwrap_or_else(|| self.page_number().to_string());
        let w = words(self.lang.unwrap_or(Lang::En));
        let mut out = format!("{} {page}", w.page);
        if let Some(d) = self.diagram {
            out.push_str(" · ");
            match self.of.filter(|&n| n > 1) {
                Some(n) => out.push_str(&(w.diagram_of)(d, n)),
                None => out.push_str(&format!("{} {d}", w.diagram)),
            }
        }
        out
    }
}

/// Single spaces, no control characters, at most [`MAX_ORIGIN_CHARS`].
fn clean(text: &str) -> String {
    text.split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_ORIGIN_CHARS)
        .collect()
}

pub(crate) struct Words {
    pub(crate) page: &'static str,
    pub(crate) diagram: &'static str,
    diagram_of: fn(usize, usize) -> String,
    /// What marks a line of an exported PGN that only the book has (plan "Carregar PGN do livro", R4).
    only_in_book: &'static str,
}

pub(crate) fn words(lang: Lang) -> Words {
    macro_rules! of {
        ($diagram:literal, $of:literal) => {
            |d, n| format!(concat!($diagram, " {} ", $of, " {}"), d, n)
        };
    }
    match lang {
        Lang::En => Words { page: "Page", diagram: "diagram", diagram_of: of!("diagram", "of"), only_in_book: "only in the book" },
        Lang::Pt => Words { page: "Pág.", diagram: "diagrama", diagram_of: of!("diagrama", "de"), only_in_book: "só no livro" },
        Lang::De => Words { page: "S.", diagram: "Diagramm", diagram_of: of!("Diagramm", "von"), only_in_book: "nur im Buch" },
        Lang::Es => Words { page: "Pág.", diagram: "diagrama", diagram_of: of!("diagrama", "de"), only_in_book: "solo en el libro" },
        Lang::Fr => Words { page: "P.", diagram: "diagramme", diagram_of: of!("diagramme", "sur"), only_in_book: "seulement dans le livre" },
        Lang::It => Words { page: "Pag.", diagram: "diagramma", diagram_of: of!("diagramma", "di"), only_in_book: "solo nel libro" },
        Lang::Ru => Words {
            page: "С.", diagram: "диаграмма", diagram_of: of!("диаграмма", "из"), only_in_book: "только в книге"
        },
        Lang::Nl => Words { page: "Blz.", diagram: "diagram", diagram_of: of!("diagram", "van"), only_in_book: "alleen in het boek" },
        Lang::Pl => Words { page: "S.", diagram: "diagram", diagram_of: of!("diagram", "z"), only_in_book: "tylko w książce" },
        Lang::Cs => Words { page: "S.", diagram: "diagram", diagram_of: of!("diagram", "z"), only_in_book: "pouze v knize" },
        Lang::Hu => Words { page: "Old.", diagram: "diagram", diagram_of: |d, n| format!("diagram {d}/{n}"), only_in_book: "csak a könyvben" },
        Lang::Da => Words { page: "S.", diagram: "diagram", diagram_of: of!("diagram", "af"), only_in_book: "kun i bogen" },
        Lang::Sv => Words { page: "S.", diagram: "diagram", diagram_of: of!("diagram", "av"), only_in_book: "endast i boken" },
        Lang::No => Words { page: "S.", diagram: "diagram", diagram_of: of!("diagram", "av"), only_in_book: "bare i boka" },
        Lang::Fi => Words { page: "S.", diagram: "kaavio", diagram_of: |d, n| format!("kaavio {d}/{n}"), only_in_book: "vain kirjassa" },
        Lang::Ro => Words { page: "Pag.", diagram: "diagrama", diagram_of: of!("diagrama", "din"), only_in_book: "doar în carte" },
        Lang::Is => Words { page: "Bls.", diagram: "stöðumynd", diagram_of: of!("stöðumynd", "af"), only_in_book: "aðeins í bókinni" },
        Lang::Ca => Words { page: "Pàg.", diagram: "diagrama", diagram_of: of!("diagrama", "de"), only_in_book: "només al llibre" },
    }
}

/// The comment on the first move of a line that the book prints and its own PGN does not have
/// ("only in the book"), in `lang`.
pub fn only_in_the_book(lang: Lang) -> &'static str {
    words(lang).only_in_book
}

/// `tags` with the origin's tags replaced: every [`ORIGIN_TAGS`] tag is taken out, and the
/// origin's are put where the first of them was (at the end when there was none).
pub fn with_origin_tags(tags: &[(String, String)], origin: &PgnOrigin) -> Vec<(String, String)> {
    let at = tags.iter().position(|(n, _)| ORIGIN_TAGS.contains(&n.as_str()));
    let mut out: Vec<(String, String)> = tags.iter().filter(|(n, _)| !ORIGIN_TAGS.contains(&n.as_str())).cloned().collect();
    let at = at.map_or(out.len(), |a| tags.iter().take(a).filter(|(n, _)| !ORIGIN_TAGS.contains(&n.as_str())).count());
    let tail = out.split_off(at.min(out.len()));
    out.extend(origin.tags());
    out.extend(tail);
    out
}

/// The text with the origin's comment first, and nothing doubled when it is already there.
pub fn with_origin_comment(comment: &str, origin: &PgnOrigin) -> String {
    let first = origin.comment();
    let rest = comment.trim();
    if rest.starts_with(&first) {
        rest.to_owned()
    } else if rest.is_empty() {
        first
    } else {
        format!("{first}. {rest}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin() -> PgnOrigin {
        PgnOrigin {
            book_title: Some("Construa  Seu\nXadrez".to_owned()),
            page_index: 22,
            page_label: Some("17".to_owned()),
            diagram: Some(1),
            of: Some(2),
            puzzle: Some(7),
            lang: Some(Lang::Pt),
        }
    }

    #[test]
    fn the_tags_are_added_in_one_order_and_the_page_stays_physical() {
        let o = origin();
        let names: Vec<_> = o.tags().into_iter().collect();
        assert_eq!(
            names,
            [
                ("PdfPage".to_owned(), "23".to_owned()),
                ("PdfPageLabel".to_owned(), "17".to_owned()),
                ("PdfDiagram".to_owned(), "1".to_owned()),
                ("PdfPuzzle".to_owned(), "7".to_owned()),
                ("SourceTitle".to_owned(), "Construa Seu Xadrez".to_owned()),
            ]
        );
        // A label that is just the physical number, and no title: neither tag.
        let plain = PgnOrigin { page_index: 4, page_label: Some("5".to_owned()), ..PgnOrigin::default() };
        assert_eq!(plain.tags(), [("PdfPage".to_owned(), "5".to_owned())]);
        assert_eq!(plain.position_roster(), None);
        assert_eq!(o.position_roster(), Some(("Construa Seu Xadrez".to_owned(), "-".to_owned())));
    }

    #[test]
    fn the_comment_is_in_the_books_language_with_the_printed_page() {
        assert_eq!(origin().comment(), "Pág. 17 · diagrama 1 de 2");
        let en = PgnOrigin { page_index: 0, page_label: Some("xvii".to_owned()), diagram: Some(3), of: Some(1), ..PgnOrigin::default() };
        assert_eq!(en.comment(), "Page xvii · diagram 3");
        let text = PgnOrigin { page_index: 9, lang: Some(Lang::De), ..PgnOrigin::default() };
        assert_eq!(text.comment(), "S. 10");
        for lang in Lang::ALL {
            let c = PgnOrigin { lang: Some(lang), ..origin() }.comment();
            assert!(c.contains("17") && c.contains('1') && c.contains('2'), "{lang:?}: {c}");
        }
    }

    #[test]
    fn origin_tags_replace_the_old_ones_in_place() {
        let tags = vec![
            ("Annotator".to_owned(), "x".to_owned()),
            ("PdfPage".to_owned(), "1".to_owned()),
            ("PdfDiagram".to_owned(), "9".to_owned()),
            ("Opening".to_owned(), "y".to_owned()),
        ];
        let out = with_origin_tags(&tags, &PgnOrigin { page_index: 2, diagram: Some(1), ..PgnOrigin::default() });
        let names: Vec<&str> = out.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["Annotator", "PdfPage", "PdfDiagram", "Opening"]);
        assert_eq!(out.get(1).map(|(_, v)| v.as_str()), Some("3"));
        let none = with_origin_tags(&[("Opening".to_owned(), "y".to_owned())], &PgnOrigin::default());
        assert_eq!(none.last().map(|(n, _)| n.as_str()), Some("PdfPage"));
        // The comment is not doubled.
        let o = origin();
        let once = with_origin_comment("A sharp line.", &o);
        assert_eq!(once, "Pág. 17 · diagrama 1 de 2. A sharp line.");
        assert_eq!(with_origin_comment(&once, &o), once);
    }

    #[test]
    fn long_and_odd_values_are_bounded() {
        let o = PgnOrigin { book_title: Some("\u{0}x ".repeat(1000)), page_label: Some(" \t ".to_owned()), ..PgnOrigin::default() };
        let tags = o.tags();
        let title = tags.iter().find(|(n, _)| n == "SourceTitle").map(|(_, v)| v.chars().count());
        assert_eq!(title, Some(MAX_ORIGIN_CHARS));
        assert!(!tags.iter().any(|(n, _)| n == "PdfPageLabel"));
        let max = PgnOrigin { page_index: u32::MAX, ..PgnOrigin::default() };
        assert_eq!(max.page_number(), u64::from(u32::MAX) + 1);
    }
}
