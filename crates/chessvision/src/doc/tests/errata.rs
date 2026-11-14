//! The notation errata list (plan A9-N19): a row per printed mark the move does not bear out, per
//! number out of sequence, and per move in conflict with the book's PGN on a born-digital page,
//! with its page and move, as CSV and Markdown; each row a misprint or a row to check (A9-N19b).

use super::*;
use crate::doc::errata::{Erratum, Issue, Kind, errata, to_csv, to_markdown};
use crate::doc::reference::{ReferenceIndex, link};

/// One page of text, page index `index`, read from its text layer or by OCR.
fn page_doc(index: u32, text: &str, ocr: bool) -> ChessDoc {
    let text = page_text(vec![line(230.0, text)]);
    build_page(&PageInput { ocr, ..input(index, &text, &[], &[], &[]) })
}

fn issues(rows: &[Erratum]) -> Vec<(&str, Issue, &str)> {
    rows.iter().map(|e| (e.printed.as_str(), e.issue, e.expected.as_str())).collect()
}

/// The card's check (plan A9-N19): a knight printed taking on an empty d8 and a check printed on a
/// move that gives none are two CSV rows, each with its page and its move, and nothing else is.
#[test]
fn a_capture_on_an_empty_square_and_a_check_too_many_are_two_rows_with_page_and_move() {
    let doc = page_doc(4, "1.Nf3 d5 2.Ng5+ Qd6 3.Ne6 Qb4 4.Nxd8 Kxd8", false);
    assert!(moves(&doc.pages[0]).iter().all(|(_, s)| *s == MoveStatus::Valid), "{:?}", moves(&doc.pages[0]));
    let rows = errata(&doc, None);
    assert_eq!(issues(&rows), [("2.Ng5+", Issue::CheckMark, "Ng5"), ("4.Nxd8", Issue::CaptureMark, "Nd8")]);
    let labels: Vec<String> = ["i", "ii", "iii", "iv", "17"].map(String::from).to_vec();
    let csv = to_csv(&rows, &labels);
    assert_eq!(
        csv,
        "page,page_label,move,printed,issue,expected,detail,read_from,kind\r\n\
         5,17,2.,2.Ng5+,check_mark,Ng5,\"printed with a check, but Ng5 gives none\",text,misprint\r\n\
         5,17,4.,4.Nxd8,capture_mark,Nd8,\"printed as a capture, but Nd8 takes nothing\",text,misprint\r\n"
    );
    // A black move printed without its number is said by the game's.
    let black = errata(&page_doc(0, "1.e4 Nf6 2.Nc3 Nxd5", false), None);
    assert_eq!(black.iter().map(|e| (e.number.as_str(), e.printed.as_str(), e.issue)).collect::<Vec<_>>(), [("2...", "Nxd5", Issue::CaptureMark)]);
    // The same page read by OCR says so: a mark there may be a misread character.
    let ocr = errata(&page_doc(4, "1.Nf3 d5 2.Ng5+ Qd6 3.Ne6 Qb4 4.Nxd8 Kxd8", true), None);
    assert_eq!(ocr.len(), 2);
    assert!(ocr.iter().all(|e| e.ocr) && to_csv(&ocr, &[]).lines().skip(1).all(|l| l.ends_with(",ocr,misprint")), "{ocr:?}");
}

/// A mate mark on a move that only checks, and a number out of sequence, are rows of their own; a
/// mark left out (`Bb5` giving check, printed without it) is none.
#[test]
fn a_mate_too_many_and_a_number_out_of_sequence_are_rows() {
    let doc = page_doc(0, "1.e4 e5 2.Qh5 Nc6 3.Qxf7# Kxf7", false);
    assert_eq!(issues(&errata(&doc, None)), [("3.Qxf7#", Issue::MateMark, "Qxf7+")]);
    let doc = page_doc(0, "1.e4 e5 2.Nf3 Nc6 5.Bb5 a6", false);
    let rows = errata(&doc, None);
    assert_eq!(issues(&rows), [("5.Bb5", Issue::MoveNumber, "3.")]);
    assert_eq!(rows[0].detail, "numbered 5. where 3. was expected");
    assert_eq!(rows[0].number, "5.", "an unplayed move keeps its printed number");
    let quiet = page_doc(0, "1.e4 e5 2.Nf3 d6 3.Bb5", false);
    assert!(errata(&quiet, None).is_empty(), "a check left out is no erratum");
}

/// The book's 1...c5 is not in its PGN, which plays 2.Nf3 after 1...e5: on a born-digital page
/// the 2.Nf3 the reader played after 1...c5 is the book's erratum, decided by the PGN, with the
/// PGN's move and the squares where the positions differ. On a page read by OCR it may be the
/// reading's, and is no row; nor is a move whose word the reader repaired.
#[test]
fn a_conflict_with_the_books_pgn_is_an_erratum_on_a_born_digital_page_only() {
    let index = ReferenceIndex::from_pgn("[Event \"Chapter 1\"]\n\n1.e4 e5 2.Nf3 Nc6 *\n").unwrap();
    let text = "1.e4 e5 ( 1...c5 2.Nf3 ) 2.Nf3";
    let mut doc = page_doc(2, text, false);
    let linked = link(&mut doc, &index);
    assert_eq!(linked.alignment.counts.conflict, 1, "{:?}", linked.alignment);
    let rows = errata(&doc, Some(&index));
    assert_eq!(issues(&rows), [("2.Nf3", Issue::BookPgn, "2.Nf3")]);
    assert_eq!(rows[0].pgn, Some(reference::RefNode { game: 0, node: 3 }));
    assert_eq!(
        rows[0].detail,
        "the book's PGN (game 1, Chapter 1) plays 2.Nf3 from another position; the positions after it differ on \
         c7 (- here, p in the PGN), e7 (p here, - in the PGN), c5 (p here, - in the PGN), e5 (- here, p in the PGN)"
    );
    // Without the PGN at hand, the row still says where in it.
    let bare = errata(&doc, None);
    assert_eq!((bare[0].expected.as_str(), bare[0].detail.as_str()), ("", "the book's PGN (game 1, move 3) plays this move from another position"));
    // Read by OCR, the same conflict is no erratum.
    let mut ocr = page_doc(2, text, true);
    link(&mut ocr, &index);
    assert!(
        ocr.pages[0]
            .blocks
            .iter()
            .flat_map(|b| b.inlines())
            .any(|i| matches!(i, Inline::Move(m) if m.reference.as_ref().is_some_and(|r| r.kind == reference::LinkKind::Conflict)))
    );
    assert!(errata(&ocr, Some(&index)).is_empty());
    // A word the reader repaired is not the page's own.
    for page in &mut doc.pages {
        for block in &mut page.blocks {
            if let Block::Paragraph { inlines, .. } = block {
                for inline in inlines {
                    if let Inline::Move(m) = inline {
                        m.read_review.push(ReviewFlag::TextRepaired {
                            from: "2.Nf3".into(),
                            to: "2.Nf3".into(),
                            why: "test".into(),
                            confidence: 0.9,
                        });
                    }
                }
            }
        }
    }
    assert!(errata(&doc, Some(&index)).is_empty());
}

/// What a PDF prints is anyone's: a field a spreadsheet would run as a formula is begun with `'`,
/// quotes and line breaks are quoted (RFC 4180), and a Markdown cell keeps a `|` as a character.
#[test]
fn the_csv_and_the_markdown_keep_what_is_printed_as_text() {
    let row = Erratum {
        at: MoveAt::new(0, 0, 0),
        issue: Issue::BookPgn,
        number: String::new(),
        printed: "=HYPERLINK(\"x\")".into(),
        expected: "-1".into(),
        detail: "a | b\nc".into(),
        ocr: false,
        pgn: None,
    };
    let csv = to_csv(std::slice::from_ref(&row), &[]);
    assert_eq!(csv.lines().nth(1), Some("1,1,,\"'=HYPERLINK(\"\"x\"\")\",book_pgn,'-1,\"a | b"));
    assert_eq!(csv.lines().nth(2), Some("c\",text,check"));
    let md = to_markdown(std::slice::from_ref(&row), &[], "Book | one");
    assert!(md.starts_with("# Notation errata: Book \\| one\n\n| Page |"), "{md}");
    assert!(md.contains("| a \\| bc | text layer | to check |"), "control characters go and the bar stays text: {md}");
    assert_eq!(to_markdown(&[], &[], "Book"), "# Notation errata: Book\n\nNo errata found.\n");
}

/// Plan A9-N19b: a move cited in prose that the walk played on the wrong line conflicts with the
/// book's PGN on a born-digital page, and that is the reading's straying, not the book's misprint:
/// the row is one to check. A capture mark on a move that takes nothing is the book's, and comes
/// first, though it is printed after.
#[test]
fn a_move_cited_in_prose_against_the_books_pgn_is_to_check_and_a_mark_is_a_misprint() {
    let index = ReferenceIndex::from_pgn("[Event \"Chapter 1\"]\n\n1.e4 e5 2.Nf3 Nc6 3.Bb5 a6 4.Ba4 Nf6 *\n").unwrap();
    let mut doc = page_doc(0, "1.e4 e5 2.Nf3 Nc6! Not 1...c5 2.Nf3 here. 3.Bb5 a6 4.Bxa4 Nf6", false);
    link(&mut doc, &index);
    assert!(doc.pages.iter().all(crate::doc::errata::born_digital));
    let rows = errata(&doc, Some(&index));
    let got: Vec<(&str, Issue, Kind)> = rows.iter().map(|e| (e.printed.as_str(), e.issue, e.issue.kind())).collect();
    assert_eq!(got, [("4.Bxa4", Issue::CaptureMark, Kind::Misprint), ("2.Nf3", Issue::BookPgn, Kind::Check)]);
    let csv = to_csv(&rows, &[]);
    let kinds: Vec<&str> = csv.lines().map(|l| l.rsplit(',').next().unwrap_or_default()).collect();
    assert_eq!(kinds, ["kind", "misprint", "check"], "{csv}");
    let md = to_markdown(&rows, &[], "Book");
    assert!(md.contains("| Read from | Kind |") && md.contains("| text layer | misprint |") && md.contains("| text layer | to check |"), "{md}");
    // A number out of sequence may be a line label the walk did not attach: to check too.
    let numbers = errata(&page_doc(0, "1.e4 e5 2.Nf3 Nc6 5.Bb5 a6", false), None);
    assert_eq!(numbers.iter().map(|e| (e.issue, e.issue.kind())).collect::<Vec<_>>(), [(Issue::MoveNumber, Kind::Check)]);
    assert_eq!(Kind::ALL.map(|k| Kind::from_name(k.name())), [Some(Kind::Misprint), Some(Kind::Check)]);
    assert_eq!(Kind::from_name("all"), None);
}
