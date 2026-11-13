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
