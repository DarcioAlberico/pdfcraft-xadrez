//! The book's exercises as a sheet to print (plan A9-N12a): the captions, the grid's pages and
//! the key.

use super::exercises::puzzle_book;
use super::*;
use crate::doc::markdown::PgnBook;
use crate::doc::worksheet::{Layout, PerPage, worksheet};

fn book(labels: &[&str]) -> PgnBook {
    PgnBook { title: Some("Puzzles".into()), labels: labels.iter().map(|l| (*l).to_owned()).collect() }
}

/// The card's book (plan A9-N12a over N10's): three exercises with their solutions and a fourth
/// numbered diagram whose solution was not read, which goes on the sheet too (decision D30).
#[test]
fn the_exercises_and_the_one_without_a_solution_go_on_the_sheet() {
    let doc = puzzle_book();
    let sheet = worksheet(&doc, &book(&["7", "8", "9"]), &Layout { per_page: PerPage::Four, solutions: true, lang: None });
    assert_eq!(sheet.pages.len(), 1, "four on one page");
    let captions: Vec<&str> = sheet.items().map(|i| i.caption.as_str()).collect();
    assert_eq!(
        captions,
        ["Nº 1 · p. 7 · White to move", "Nº 2 · p. 7 · White to move", "Nº 3 · p. 7 · Black to move", "Nº 4 · p. 8 · White to move"]
    );
    let key: Vec<&str> = sheet.solutions.iter().map(|(l, _)| l.as_str()).collect();
    assert_eq!(key[0], "Nº 1 · p. 7: 3.Qh5 Nf6 4.Qxf7#", "numbered from the position");
    assert_eq!(key[2], "Nº 3 · p. 7: 1...Qf1#");
    assert_eq!(key[3], "Nº 4 · p. 8: solution not read", "{key:?}");
    assert!(sheet.left_out.is_empty(), "{:?}", sheet.left_out);
    // Without the solutions, no key.
    assert!(worksheet(&doc, &book(&[]), &Layout { solutions: false, ..Layout::default() }).solutions.is_empty());
}

/// The captions and the key are in the book's language, the English kept beside them.
#[test]
fn the_caption_is_in_the_books_language() {
    let mut doc = puzzle_book();
    for page in &mut doc.pages {
        page.text_lang = LangTag::parse("pt");
    }
    let sheet = worksheet(&doc, &book(&["7", "8", "9"]), &Layout { solutions: true, ..Layout::default() });
    let first = sheet.items().next().unwrap();
    assert_eq!((first.caption.as_str(), first.caption_en.as_str()), ("Nº 1 · p. 7 · Brancas jogam", "Nº 1 · p. 7 · White to move"));
    assert_eq!(sheet.solutions[3].0, "Nº 4 · p. 8: solução não lida");
}

/// A page the book labels `xvii` is `p. xvii`; a diagram printed from Black's side is drawn so.
#[test]
fn the_page_is_as_the_book_numbers_it_and_the_board_as_it_prints_it() {
    let mut doc = puzzle_book();
    for page in &mut doc.pages {
        for block in &mut page.blocks {
            if let Block::Diagram(d) = block
                && let Some(r) = d.reading.as_mut()
                && d.puzzle == Some(2)
            {
                r.orientation = Some(Color::Black);
            }
        }
    }
    let sheet = worksheet(&doc, &book(&["xvii", "xviii", "xix"]), &Layout::default());
    let items: Vec<_> = sheet.items().collect();
    assert!(items[0].caption.starts_with("Nº 1 · p. xvii ·"), "{}", items[0].caption);
    assert_eq!(items.iter().map(|i| i.orientation).collect::<Vec<_>>(), [Color::White, Color::Black, Color::White, Color::White]);
}

/// Ten exercises take 3, 2 and 2 pages at 4, 6 and 9 a page.
#[test]
fn the_grid_takes_four_six_or_nine_a_page() {
    let mut b = DocBuilder::new();
    for p in 0..5u32 {
        let (n1, n2) = (2 * p + 1, 2 * p + 2);
        let text = page_text(vec![line(45.0, &format!("№{n1}. A – B")), line(245.0, &format!("№{n2}. C – D"))]);
        let d = [reading(0, 60.0, SCHOLAR, true, false), reading(1, 260.0, SCHOLAR, true, false)];
        b.add_page(&input(p, &text, &d, &[], &[]));
    }
    let doc = b.finish();
    for (per_page, pages) in [(PerPage::Four, 3), (PerPage::Six, 2), (PerPage::Nine, 2)] {
        let sheet = worksheet(&doc, &book(&[]), &Layout { per_page, ..Layout::default() });
        assert_eq!((sheet.items().count(), sheet.pages.len()), (10, pages), "{per_page:?}");
        assert!(sheet.pages.iter().all(|p| p.len() <= per_page.count()));
    }
    assert_eq!((PerPage::of(6), PerPage::of(5)), (Some(PerPage::Six), None));
}

/// Plan A9-N12b2: the exercises past the cap are counted and placed, in the book's order, so the
/// export dialog counts those of the pages it believes as it counts the others.
#[test]
fn the_exercises_past_the_cap_say_where_they_are() {
    let mut b = DocBuilder::new();
    for p in 0..5u32 {
        let (n1, n2) = (2 * p + 1, 2 * p + 2);
        let text = page_text(vec![line(45.0, &format!("№{n1}. A – B")), line(245.0, &format!("№{n2}. C – D"))]);
        let d = [reading(0, 60.0, SCHOLAR, true, false), reading(1, 260.0, SCHOLAR, true, false)];
        b.add_page(&input(p, &text, &d, &[], &[]));
    }
    let doc = b.finish();
    let sheet = crate::doc::worksheet::capped(&doc, &book(&[]), &Layout::default(), 7);
    assert_eq!((sheet.items().count(), sheet.over), (7, 3));
    let at: Vec<(usize, usize)> = sheet.over_at.iter().map(|a| (a.page, a.index)).collect();
    assert_eq!(at, [(3, 1), (4, 0), (4, 1)], "the eighth, ninth and tenth, where they are");
    assert!(worksheet(&doc, &book(&[]), &Layout::default()).over_at.is_empty(), "none past the real cap");
}

/// Plan A9-N12a2: the numbered diagram whose solution was not read, its side to move assumed,
/// says both in the key, in each language the key is written in; the English beside it always.
#[test]
fn the_key_says_the_side_was_assumed_in_its_language() {
    let mut doc = puzzle_book();
    for page in &mut doc.pages {
        for block in &mut page.blocks {
            if let Block::Diagram(d) = block
                && d.puzzle == Some(4)
            {
                if let Some(r) = d.reading.as_mut() {
                    r.side_source = crate::doc::SideSource::Assumed;
                }
                if let Some(s) = d.settled.as_mut() {
                    s.side = crate::doc::SideSource::Assumed;
                }
            }
        }
    }
    for (lang, said) in [
        (Lang::Pt, "solução não lida (lado suposto)"),
        (Lang::Es, "solución no leída (turno supuesto)"),
        (Lang::De, "Lösung nicht gelesen (Zugrecht angenommen)"),
        (Lang::Fr, "solution non lue (trait supposé)"),
        (Lang::It, "soluzione non letta (tratto presunto)"),
        (Lang::Nl, "oplossing niet gelezen (aan zet aangenomen)"),
        (Lang::En, "solution not read (side to move assumed)"),
        // A language the key has no words for: English.
        (Lang::Ru, "solution not read (side to move assumed)"),
    ] {
        let sheet = worksheet(&doc, &book(&["7", "8", "9"]), &Layout { solutions: true, lang: Some(lang), ..Layout::default() });
        assert!(sheet.items().nth(3).is_some_and(|i| i.side_assumed && i.solution.is_none()), "{lang:?}");
        assert_eq!(
            sheet.solutions[3],
            (format!("Nº 4 · p. 8: {said}"), "Nº 4 · p. 8: solution not read (side to move assumed)".to_owned()),
            "{lang:?}"
        );
    }
}
