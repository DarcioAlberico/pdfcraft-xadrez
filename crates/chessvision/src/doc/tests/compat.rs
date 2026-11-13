//! What the document's JSON is to builds before and after a change of its shape (plan A9 §2.10).

/// The document's [`Block`](crate::doc::Block) as written before plan A9-L1a gave text blocks a
/// `role` (a literal copy of `crates/chessvision/src/doc.rs` at `53a46b42`, its inlines and
/// diagram unchanged since): what an earlier build reads a document's blocks as.
mod block_v1 {
    use serde::{Deserialize, Serialize};

    use crate::doc::{DiagramBlock, Inline, UserRect};

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    pub enum Block {
        Heading { level: u8, inlines: Vec<Inline>, source_rect: Option<UserRect> },
        Paragraph { inlines: Vec<Inline>, source_rect: Option<UserRect> },
        Diagram(DiagramBlock),
    }
}

use super::*;

/// A page with every role: a diagram's text, a game's header (players and event) and the page's
/// number, between two games.
pub(super) fn page_with_roles() -> ChessDoc {
    let text = page_text(vec![
        line(20.0, "1.e4 e5 2.Nf3 Nc6"),
        line(150.0, "4*+*+p+*+5"),
        line(240.0, "(1) Carlsen,Magnus (2864)— Anand,Viswanathan (2786) [E46]"),
        line(300.0, "Tal Memorial Moscow RUS (5.2), 2013[Arkadij Naiditsch]"),
        line(360.0, "1.d4 Nf6 2.c4 e6"),
        line(760.0, "15"),
    ]);
    let after = "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3";
    build_page(&input(0, &text, &[reading(0, 60.0, after, true, false)], &[], &[]))
}

/// Plan A9-L1a, `role` in the document's JSON: an earlier build reads the blocks of a document
/// written now as it read every block (a folio, a header's line and a diagram's text are
/// paragraphs to it, as they were), and what it writes back is read now with every role the
/// book's text, which walking again gives back but for the diagram's text.
#[test]
fn an_earlier_build_reads_the_roles_as_plain_blocks() {
    let doc = page_with_roles();
    let json = doc.to_json().unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let blocks = value["pages"][0]["blocks"].clone();
    // What an earlier build reads, and writes back.
    let old: Vec<block_v1::Block> = serde_json::from_value(blocks).unwrap();
    assert_eq!(old.len(), doc.pages[0].blocks.len());
    assert!(matches!(old[2], block_v1::Block::Paragraph { .. }), "the diagram's text is a paragraph to it");
    let rewritten = serde_json::to_value(&old).unwrap();
    assert!(!rewritten.to_string().contains("\"role\""), "it drops the roles when it writes");
    // Read now: the same blocks, every one the book's text.
    let now: Vec<Block> = serde_json::from_value(rewritten).unwrap();
    let mut expected = doc.pages[0].blocks.clone();
    for b in &mut expected {
        b.set_role(BlockRole::Content);
    }
    assert_eq!(now, expected);
    // A document an earlier build wrote (no `role` anywhere) reads with the book's text only.
    let earlier = r#"{"format":1,"pages":[{"index":0,"lang":"en","text_layer":"good","blocks":[
        {"type":"heading","level":1,"inlines":[{"type":"text","text":"Chapter 1","source_rect":null}],"source_rect":null},
        {"type":"paragraph","inlines":[{"type":"text","text":"15","source_rect":null}],"source_rect":null}]}],"games":[]}"#;
    let read = ChessDoc::from_json(earlier).unwrap();
    assert!(read.pages[0].blocks.iter().all(|b| b.role() == Some(BlockRole::Content)));
}

/// The document's `DiagramReading` as written before plan A9-L6b (a literal copy at `53a46b42`):
/// what an earlier build reads a diagram's reading as.
mod reading_v1 {
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub struct DiagramReading {
        pub fen: String,
        pub legal: bool,
        pub accepted: bool,
        pub min_confidence: f32,
        pub repaired: bool,
        pub side_assumed: bool,
    }
}

/// Plan A9-L6b, the reading's facts once: an earlier build reads a reading written now (`legal`
/// and `side_assumed` are still written, from the reading), and a reading it wrote reads now with
/// its side's source and, for a board printed from Black, the orientation its link said.
#[test]
fn an_earlier_build_reads_the_reading_and_its_readings_read_now() {
    let read = |fen: &str, assumed: bool| reading(0, 60.0, fen, true, assumed);
    let doc = build_page(&input(0, &page_text(vec![]), &[read(SCHOLAR, true)], &[], &[]));
    let Block::Diagram(d) = &doc.pages[0].blocks[0] else { panic!("a diagram") };
    let json = serde_json::to_value(d.reading.as_ref().unwrap()).unwrap();
    let old: reading_v1::DiagramReading = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(
        old,
        reading_v1::DiagramReading { fen: SCHOLAR.into(), legal: true, accepted: true, min_confidence: 0.99, repaired: false, side_assumed: true }
    );
    assert_eq!(json["side_source"], "assumed");
    // What an earlier build wrote: no source, no orientation.
    let written = serde_json::to_value(&old).unwrap();
    let now: DiagramReading = serde_json::from_value(written).unwrap();
    assert_eq!((now.side_source, now.orientation, now.legal()), (crate::doc::SideSource::Assumed, None, true));
    let said: DiagramReading = serde_json::from_value(
        serde_json::json!({"fen": SCHOLAR, "legal": true, "accepted": true, "min_confidence": 1.0, "repaired": false, "side_assumed": false}),
    )
    .unwrap();
    assert_eq!(said.side_source, crate::doc::SideSource::Printed);
    // A board printed from Black's side, written before: its link says it.
    let mut block = d.clone();
    block.reading = Some(now);
    block.lichess_url = Some("https://lichess.org/analysis/x?color=black".into());
    assert_eq!(block.orientation(), pdfcraft_chess::placement::Color::Black);
}
