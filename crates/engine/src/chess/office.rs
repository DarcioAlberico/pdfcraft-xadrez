//! Chess ▸ Export ▸ Word and RTF (blueprint S15c): the recognised document through the Office
//! writers of `pdfcraft-export`.
//!
//! Word and RTF have no inline vector drawing a reader can be relied on to show, so each diagram
//! is a PNG: [`diagram_png`] draws the board with `chess::draw` and the same operators the PDF
//! replacement uses ([`super::replace::drawing_ops`]) on a one-page PDF made here, and renders it
//! with the page renderer. [`to_pages`] then lays the document out as the export writers take
//! it: one [`Page`] per PDF page, blocks stacked top to bottom in reading order (headings set
//! larger, so the writers make them headings), each diagram an image with its lichess address
//! under it, a link over that line so Word and RTF make it a real hyperlink. Nothing overlaps, so
//! the Word writer keeps every page flowing.
//!
//! Text is set with Unicode figurines by default; whether a reader shows ♞ depends on its fonts.

use std::sync::Arc;

use pdfcraft_chess::draw::{BoardStyle, PieceSet, board};
use pdfcraft_chess::placement::Placement;
use pdfcraft_chessvision::doc::html::{Figurines, plain_text_with};
use pdfcraft_chessvision::doc::{Block as DocBlock, ChessDoc};
use pdfcraft_export::{Block, Image, Link, Page};
/// The writers [`to_pages`] feeds.
pub use pdfcraft_export::{docx, rtf};
use pdfcraft_render::{PageRenderer, RenderConfig, RenderRequest};

use super::replace::drawing_ops;

/// The default side of a diagram image, in pixels.
pub const DEFAULT_DIAGRAM_PX: u32 = 480;
/// The largest side of a diagram image, in pixels.
pub const MAX_DIAGRAM_PX: u32 = 2400;
/// The side a diagram is set at, in points (three inches).
pub const DIAGRAM_PT: f64 = 216.0;
/// The body text size, and the heading sizes the writers recognise as levels 1 and 2.
const BODY: f64 = 11.0;
const H1: f64 = 20.0;
const H2: f64 = 15.0;
/// The laid-out page: A4 width, with an inch of margin.
const WIDTH: f64 = 595.0;
const LEFT: f64 = 72.0;
const RIGHT: f64 = WIDTH - 72.0;
const GAP: f64 = 6.0;

/// What the Office exports make.
#[derive(Clone, Debug)]
pub struct OfficeOptions<'a> {
    pub pieces: &'a PieceSet,
    pub style: BoardStyle,
    pub figurines: Figurines,
    /// The lichess address under each diagram that has one.
    pub links: bool,
    pub link_text: &'a str,
    /// The diagram images' side in pixels (clamped to 32..=[`MAX_DIAGRAM_PX`]).
    pub diagram_px: u32,
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

/// One thing to stack on a page.
enum Part {
    /// `link`: the address the paragraph links to.
    Text {
        text: String,
        size: f64,
        bold: bool,
        link: Option<String>,
    },
    Picture(Vec<u8>),
}

/// The height a paragraph of `text` at `size` takes in the text column, roughly: what decides
/// only where the next block goes, never what the writers print.
fn text_height(text: &str, size: f64) -> f64 {
    let per_line = ((RIGHT - LEFT) / (size * 0.5)).max(1.0);
    let lines = (text.chars().count() as f64 / per_line).ceil().max(1.0);
    lines * size * 1.3
}

/// The document as the export writers' pages, and how many diagrams were drawn. A diagram whose
/// image cannot be made is set as its FEN, and said in `notes`.
pub fn to_pages(doc: &ChessDoc, o: &OfficeOptions<'_>, notes: &mut Vec<String>) -> (Vec<Page>, usize) {
    let mut pages = Vec::with_capacity(doc.pages.len());
    let mut drawn = 0;
    for page in &doc.pages {
        let mut parts = Vec::new();
        for block in &page.blocks {
            match block {
                DocBlock::Heading { level, inlines, .. } => {
                    let size = if *level <= 1 { H1 } else { H2 };
                    parts.push(Part::Text { text: plain_text_with(inlines, o.figurines), size, bold: true, link: None });
                }
                DocBlock::Paragraph { inlines, .. } => {
                    parts.push(Part::Text { text: plain_text_with(inlines, o.figurines), size: BODY, bold: false, link: None })
                }
                DocBlock::Diagram(d) => {
                    let Some(reading) = &d.reading else {
                        parts.push(Part::Text { text: format!("[Diagram {} not read]", d.number + 1), size: BODY, bold: false, link: None });
                        continue;
                    };
                    let png = pdfcraft_chess::fen::parse(&reading.fen)
                        .map_err(|e| e.to_string())
                        .and_then(|f| diagram_png(&f.placement, o.pieces, &o.style, o.diagram_px));
                    match png {
                        Ok(bytes) => {
                            parts.push(Part::Picture(bytes));
                            drawn += 1;
                        }
                        Err(e) => {
                            notes.push(format!("page {}, diagram {}: {e}; its FEN was set instead", u64::from(page.index) + 1, d.number + 1));
                        }
                    }
                    parts.push(Part::Text { text: format!("FEN: {}", reading.fen), size: 8.0, bold: false, link: None });
                    if o.links
                        && let Some(url) = &d.lichess_url
                    {
                        parts.push(Part::Text { text: format!("{}: {url}", o.link_text), size: 8.0, bold: false, link: Some(url.clone()) });
                    }
                }
            }
        }
        // Stack the parts from the top; the page is as tall as they need.
        let heights: Vec<f64> = parts
            .iter()
            .map(|p| match p {
                Part::Text { text, size, .. } => text_height(text, *size),
                Part::Picture(_) => DIAGRAM_PT,
            })
            .collect();
        let height = (heights.iter().map(|h| h + GAP).sum::<f64>() + 144.0).max(842.0);
        let mut top = height - 72.0;
        let mut out = Page { links: Vec::new(), width: WIDTH, height, blocks: Vec::new(), images: Vec::new() };
        for (part, h) in parts.into_iter().zip(heights) {
            let rect = [LEFT, top - h, if matches!(part, Part::Picture(_)) { LEFT + DIAGRAM_PT } else { RIGHT }, top];
            match part {
                Part::Text { text, size, bold, link } => {
                    // The link's area is the paragraph's box, so the writers make it a hyperlink.
                    if let Some(uri) = link {
                        out.links.push(Link { rect, uri });
                    }
                    out.blocks.push(Block { text, rect, size, bold, italic: false, color: [0.0; 3], rotation: 0.0 })
                }
                Part::Picture(bytes) => out.images.push(Image { ext: "png", bytes, rect, size: [DIAGRAM_PT, DIAGRAM_PT], rotation: 0.0 }),
            }
            top -= h + GAP;
        }
        pages.push(out);
    }
    (pages, drawn)
}
