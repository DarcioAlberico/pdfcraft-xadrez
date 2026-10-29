//! Chess ▸ Export ▸ Word and RTF (blueprint S15c; plan A8 P9a): the recognised document as a
//! flowing document, written by `pdfcraft_export::flow` (the PDF ▸ Word export's own path, which
//! rebuilds pages from their boxes, is left alone).
//!
//! [`to_flow`] keeps the book's structure: headings as Word headings, paragraphs as runs, the
//! moves and annotation glyphs marked so spelling checks skip them (`w:noProof`), piece letters
//! and signs in the user's figurine font (SemFig) and sign font (ISChess) when the export found
//! them, as the HTML export sets them ([`spans`]), and the document in the book's language
//! (`w:lang`). Each diagram is a picture, drawn by [`diagram_png`] at 300 ppi for the
//! [`DIAGRAM_PT`] it is set at, its text alternative saying the position in words
//! ([`pdfcraft_chess::describe`]), and a caption under it in Word's `Caption` style: the PGN's
//! comment for the diagram (`Pág. 17 · diagrama 1 de 2`), its FEN and its lichess link. Word also
//! gets the board as SVG, the same drawing, which it shows at any size (the PNG is the fallback).
//!
//! [`diagram_png`] draws the board with `chess::draw` and the same operators the PDF replacement
//! uses ([`super::replace::drawing_ops`]) on a one-page PDF made here, and renders it with the
//! page renderer.

use std::sync::Arc;

use pdfcraft_chess::describe;
use pdfcraft_chess::draw::{BoardStyle, PieceSet, board};
use pdfcraft_chess::origin::PgnOrigin;
use pdfcraft_chess::placement::Placement;
use pdfcraft_chess::san::Lang;
use pdfcraft_chess::svg::to_svg;
use pdfcraft_chessvision::doc::html::{Figurines, Span, SpanFont, spans};
use pdfcraft_chessvision::doc::{Block as DocBlock, ChessDoc, Inline};
use pdfcraft_chessvision::text_font::TextFont;
use pdfcraft_export::flow::{Figure, FlowBlock, FlowDoc, Run};
/// The writers [`to_flow`] feeds.
pub use pdfcraft_export::flow::{docx, fonts, rtf};
use pdfcraft_render::{PageRenderer, RenderConfig, RenderRequest};

use super::replace::drawing_ops;

/// The default side of a diagram image, in pixels: 300 ppi at the [`DIAGRAM_PT`] Word and RTF set
/// it at.
pub const DEFAULT_DIAGRAM_PX: u32 = 900;
/// The largest side of a diagram image, in pixels.
pub const MAX_DIAGRAM_PX: u32 = 2400;
/// The side a diagram is set at, in points (three inches).
pub const DIAGRAM_PT: f64 = 216.0;

/// What the Office exports make.
#[derive(Clone, Debug)]
pub struct OfficeOptions<'a> {
    pub title: &'a str,
    pub pieces: &'a PieceSet,
    pub style: BoardStyle,
    pub figurines: Figurines,
    /// The lichess address under each diagram that has one.
    pub links: bool,
    pub link_text: &'a str,
    /// The diagram images' side in pixels (clamped to 32..=[`MAX_DIAGRAM_PX`]).
    pub diagram_px: u32,
    /// The user's figurine font: its family is named on the runs that need it.
    pub text_font: Option<&'a TextFont>,
    /// The user's sign font (ISChess): the annotation glyphs it draws are written in it.
    pub sign_font: Option<&'a TextFont>,
    /// The book's language, as a BCP 47 tag.
    pub lang: &'a str,
    /// The number printed on each page, by page index (empty, or an empty label: the page's own
    /// number from 1).
    pub page_labels: &'a [String],
}

/// The board as a PNG of `px` × `px` pixels (clamped to 32..=[`MAX_DIAGRAM_PX`]).
pub fn diagram_png(placement: &Placement, pieces: &PieceSet, style: &BoardStyle, px: u32) -> Result<Vec<u8>, String> {
    let (w, h, rgba) = diagram_rgba(placement, pieces, style, px)?;
    crate::export::encode_png(w, h, &rgba)
}

/// The board as premultiplied RGBA pixels, `px` × `px` (clamped to 32..=[`MAX_DIAGRAM_PX`]): what
/// [`diagram_png`] encodes, for a screen to show directly.
pub fn diagram_rgba(placement: &Placement, pieces: &PieceSet, style: &BoardStyle, px: u32) -> Result<(u32, u32, Vec<u8>), String> {
    let drawing = board(placement, pieces, style);
    let side = f64::from(drawing.width);
    if !(side.is_finite() && side > 0.0) {
        return Err("the board has no size to draw".into());
    }
    // Drawing units have their origin at the top left, y down: flip into the page.
    let ops = drawing_ops(&drawing, [1.0, 0.0, 0.0, -1.0, 0.0, side]);
    let pdf = one_page_pdf(side, &ops);
    let px = px.clamp(32, MAX_DIAGRAM_PX);
    let mut renderer = PageRenderer::new(Arc::new(pdf), RenderConfig::default());
    let shot = renderer.render(RenderRequest { page: 0, scale: (f64::from(px) / side) as f32, ..Default::default() });
    if let Some(e) = shot.error {
        return Err(format!("the diagram could not be drawn: {e}"));
    }
    Ok((shot.width, shot.height, shot.rgba.to_vec()))
}

/// A one-page PDF of `side` × `side` points drawing `ops`, with Helvetica as `/PCHelv` for the
/// board's coordinate labels.
fn one_page_pdf(side: f64, ops: &[u8]) -> Vec<u8> {
    let mut content = b"q 1 g 0 0 ".to_vec();
    content.extend_from_slice(format!("{side} {side} re f Q\n").as_bytes());
    content.extend_from_slice(ops);
    let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
    stream.extend_from_slice(&content);
    stream.extend_from_slice(b"\nendstream");
    let objs: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!("<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 {side} {side}] >>").into_bytes(),
        b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /PCHelv 5 0 R >> >> >>".to_vec(),
        stream,
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec(),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::with_capacity(objs.len());
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(o);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    out
}

/// A font's family name, when it has one Word or RTF can name.
fn family(font: Option<&TextFont>) -> Option<String> {
    font.map(|f| f.family.trim().to_owned()).filter(|f| !f.is_empty())
}

/// A paragraph's inlines as runs: moves and glyphs skipped by spelling checks, in the user's
/// fonts where the HTML export sets them in those fonts.
fn runs(inlines: &[Inline], o: &OfficeOptions<'_>) -> Vec<Run> {
    let (figurines, signs) = (family(o.text_font), family(o.sign_font));
    // A font with no name cannot be named: its runs keep the document's font.
    let role = o.text_font.filter(|_| figurines.is_some()).map(|f| f.role);
    spans(inlines, o.figurines, role, signs.is_some())
        .into_iter()
        .map(|Span { text, chess, font, link }| Run {
            text,
            font: match font {
                Some(SpanFont::Figurines) => figurines.clone(),
                Some(SpanFont::Signs) => signs.clone(),
                None => None,
            },
            lang: None,
            no_proof: chess,
            link,
        })
        .collect()
}

/// The document as a flowing document, and how many diagrams were drawn. A diagram whose image
/// cannot be made is set as text, and said in `notes`.
pub fn to_flow(doc: &ChessDoc, o: &OfficeOptions<'_>, notes: &mut Vec<String>) -> (FlowDoc, usize) {
    let mut blocks = Vec::new();
    let mut drawn = 0;
    for page in &doc.pages {
        let lang = Lang::from_tag(&page.lang).unwrap_or(Lang::En);
        let printed = usize::try_from(page.index).ok().and_then(|i| o.page_labels.get(i)).map(|l| l.trim()).filter(|l| !l.is_empty());
        let label = printed.map_or_else(|| (u64::from(page.index) + 1).to_string(), str::to_owned);
        let of = page.blocks.iter().filter(|b| matches!(b, DocBlock::Diagram(_))).count();
        for block in &page.blocks {
            match block {
                DocBlock::Heading { level, inlines, .. } => blocks.push(FlowBlock::Heading { level: *level, runs: runs(inlines, o) }),
                DocBlock::Paragraph { inlines, .. } => blocks.push(FlowBlock::Para(runs(inlines, o))),
                DocBlock::Diagram(d) => {
                    let number = d.number.saturating_add(1);
                    let Some(reading) = &d.reading else {
                        blocks.push(FlowBlock::Para(vec![Run::text(describe::not_read(number, &label, lang))]));
                        continue;
                    };
                    // The caption: the PGN's comment for the diagram, its FEN, its link.
                    let origin = PgnOrigin {
                        page_index: page.index,
                        page_label: printed.map(str::to_owned),
                        diagram: Some(number),
                        of: Some(of),
                        lang: Some(lang),
                        ..PgnOrigin::default()
                    };
                    let mut caption =
                        vec![Run::text(format!("{} · ", origin.comment())), Run { no_proof: true, ..Run::text(format!("FEN {}", reading.fen)) }];
                    if o.links
                        && let Some(url) = &d.lichess_url
                    {
                        caption.push(Run::text(" · "));
                        caption.push(Run { link: Some(url.clone()), ..Run::text(o.link_text) });
                    }
                    let drawn_board = pdfcraft_chess::fen::parse(&reading.fen).map_err(|e| e.to_string()).and_then(|f| {
                        let png = diagram_png(&f.placement, o.pieces, &o.style, o.diagram_px)?;
                        Ok((png, f.placement))
                    });
                    match drawn_board {
                        Ok((png, placement)) => {
                            let alt = describe::describe(number, &label, &reading.fen, lang);
                            // Word draws the SVG at any size; the PNG stays as its fallback.
                            let svg = Some(to_svg(&board(&placement, o.pieces, &o.style), &alt));
                            blocks.push(FlowBlock::Figure(Figure { png, svg, side_pt: DIAGRAM_PT, alt, caption }));
                            drawn += 1;
                        }
                        Err(e) => {
                            notes.push(format!("page {}, diagram {number}: {e}; its FEN was set instead", u64::from(page.index) + 1));
                            blocks.push(FlowBlock::Para(caption));
                        }
                    }
                }
            }
        }
    }
    let flow = FlowDoc { title: o.title.to_owned(), lang: o.lang.to_owned(), blocks };
    // Nothing is embedded: a font a run names must be installed where the file is read.
    let named = fonts(&flow);
    if !named.is_empty() {
        notes.push(format!("the document sets text in {}: it shows as meant only where the font is installed", named.join(", ")));
    }
    (flow, drawn)
}
