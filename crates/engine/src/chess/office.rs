//! Chess ▸ Export ▸ Word and RTF (blueprint S15c; plan A8 P9a): the recognised document as a
//! flowing document, written by `pdfcraft_export::flow` (the PDF ▸ Word export's own path, which
//! rebuilds pages from their boxes, is left alone).
//!
//! [`to_flow`] keeps the book's structure: headings as Word headings, paragraphs as runs, the
//! moves and annotation glyphs marked so spelling checks skip them (`w:noProof`), piece letters
//! and signs in the user's figurine font (SemFig) and sign font (ISChess) when the export found
//! them, as the HTML export sets them ([`spans`]), and the document in the book's language
//! (`w:lang`). Each diagram is a picture, its text alternative saying the position in words
//! ([`pdfcraft_chess::describe`]), and a caption under it in Word's `Caption` style: the PGN's
//! comment for the diagram (`Pág. 17 · diagrama 1 de 2`), its FEN and its lichess link. Word gets
//! the board as SVG ([`pdfcraft_chess::svg::board_standalone`]: no `<use>`, decision D18), the
//! same drawing, which Word 2016 and later show and print at any size, and as a PNG of
//! [`WORD_DIAGRAM_PX`] for readers that cannot; RTF gets the PNG alone, at 300 ppi for the
//! [`DIAGRAM_PT`] it is set at ([`DEFAULT_DIAGRAM_PX`]). A position the book prints again is drawn
//! once and stored once (plan A9-X1b, R8-09: a 748-page book's Word file was 118.5 MB).
//!
//! [`diagram_png`] draws the board with `chess::draw` and the same operators the PDF replacement
//! uses ([`super::replace::drawing_ops`]) on a one-page PDF made here, renders it with the page
//! renderer, and writes it paletted ([`palette`]).

use std::collections::HashMap;
use std::sync::Arc;

use pdfcraft_chess::describe;
use pdfcraft_chess::draw::{BoardStyle, PieceSet, board};
use pdfcraft_chess::origin::PgnOrigin;
use pdfcraft_chess::placement::Placement;
use pdfcraft_chess::san::Lang;
use pdfcraft_chess::svg::board_standalone;
use pdfcraft_chessvision::doc::html::{Figurines, Span, SpanFont, spans};
use pdfcraft_chessvision::doc::{Block as DocBlock, ChessDoc, Inline, PageLabel};
use pdfcraft_chessvision::text_font::TextFont;
use pdfcraft_export::flow::{Figure, FlowBlock, FlowDoc, Run};
/// The writers [`to_flow`] feeds.
pub use pdfcraft_export::flow::{docx, fonts, rtf};
use pdfcraft_render::{PageRenderer, RenderConfig, RenderRequest};

use super::replace::drawing_ops;

pub mod palette;

/// The default side of a diagram image, in pixels: 300 ppi at the [`DIAGRAM_PT`] Word and RTF set
/// it at. RTF, Markdown and Anki's cards, whose picture is the only one.
pub const DEFAULT_DIAGRAM_PX: u32 = 900;
/// The default side of Word's diagram image, the picture a reader shows when it cannot draw the
/// SVG beside it (Word before 2016, some other readers; plan A9-X1b): 1.5 times the 288 px Word
/// shows a diagram at on a 96 dpi screen, 144 ppi printed. Word 2016 and later draw and print the
/// SVG. A book's 999 diagrams were 92 MB of 900 px RGBA pictures; this size is what keeps such a
/// book's file within 25 MB with the SVG kept.
pub const WORD_DIAGRAM_PX: u32 = 432;
/// The most colours of Word's diagram image (plan A9-X1b): the squares and the pieces' fills
/// exact, the edges between them in fewer levels than RTF's picture, which stands alone.
pub const WORD_DIAGRAM_COLORS: usize = 64;
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
    /// Word: each diagram also as SVG, which Word 2016 and later draw, the PNG its fallback of at
    /// most [`WORD_DIAGRAM_COLORS`] colours; RTF: the PNG alone, of up to 256 colours.
    pub svg: bool,
    /// The user's figurine font: its family is named on the runs that need it.
    pub text_font: Option<&'a TextFont>,
    /// The user's sign font (ISChess): the annotation glyphs it draws are written in it.
    pub sign_font: Option<&'a TextFont>,
    /// The book's language, as a BCP 47 tag.
    pub lang: &'a str,
    /// The number printed on each page, by page index (empty, or an empty label: the page's own
    /// number from 1).
    pub page_labels: &'a [String],
    /// How a diagram's alt text names the pieces: by letters or in words (plan A9-X2).
    pub naming: describe::Naming,
}

/// The board as a PNG of `px` × `px` pixels (clamped to 32..=[`MAX_DIAGRAM_PX`]), paletted: a
/// byte a pixel, at most [`palette::MAX_COLORS`] colours, the squares' own exact (plan A9-X1b).
pub fn diagram_png(placement: &Placement, pieces: &PieceSet, style: &BoardStyle, px: u32) -> Result<Vec<u8>, String> {
    diagram_png_of(placement, pieces, style, px, palette::MAX_COLORS)
}

/// [`diagram_png`] of at most `colors` colours.
fn diagram_png_of(placement: &Placement, pieces: &PieceSet, style: &BoardStyle, px: u32, colors: usize) -> Result<Vec<u8>, String> {
    let (w, h, rgba) = diagram_rgba(placement, pieces, style, px)?;
    palette::encode(w, h, &rgba, colors)
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
/// fonts where the HTML export sets them in those fonts, a sign's name after it in `lang` (or in
/// English, said so).
fn runs(inlines: &[Inline], o: &OfficeOptions<'_>, lang: Lang) -> Vec<Run> {
    let (figurines, signs) = (family(o.text_font), family(o.sign_font));
    // A font with no name cannot be named: its runs keep the document's font.
    let role = o.text_font.filter(|_| figurines.is_some()).map(|f| f.role);
    spans(inlines, o.figurines, role, signs.is_some(), lang)
        .into_iter()
        .map(|Span { text, chess, font, link, lang }| Run {
            text,
            font: match font {
                Some(SpanFont::Figurines) => figurines.clone(),
                Some(SpanFont::Signs) => signs.clone(),
                None => None,
            },
            lang: lang.map(str::to_owned),
            no_proof: chess,
            link,
        })
        .collect()
}

/// The most positions a Word or RTF file gets drawn (plan A9-X1b): a PNG and an SVG each, under
/// the 65,535 files a zip file holds with the parts beside them. A book prints a few thousand.
pub const MAX_BOARDS: usize = 30_000;
/// The most bytes of drawn positions, PNG and SVG, a Word or RTF file gets (1 GiB): far past any
/// book at the default size, and under the 4 GB a zip file holds.
pub const MAX_BOARD_BYTES: usize = 1 << 30;

/// How many positions, and how many bytes of them, [`to_flow`] draws at most.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    pub(crate) boards: usize,
    pub(crate) bytes: usize,
}

/// A position drawn for a figure: its PNG, and for Word its SVG. The same position is drawn once.
type Drawn = Result<(Vec<u8>, Option<String>), String>;

/// The document as a flowing document, and how many diagrams were drawn. A diagram whose image
/// cannot be made is set as text, and said in `notes`; so is one past [`MAX_BOARDS`] positions or
/// [`MAX_BOARD_BYTES`] bytes of them. A position is drawn once, however often the book prints it
/// (plan A9-X1b).
pub fn to_flow(doc: &ChessDoc, o: &OfficeOptions<'_>, notes: &mut Vec<String>) -> (FlowDoc, usize) {
    let limits = Limits { boards: MAX_BOARDS, bytes: MAX_BOARD_BYTES };
    #[cfg(test)]
    let limits = TEST_LIMITS.with(std::cell::Cell::get).unwrap_or(limits);
    to_flow_within(doc, o, notes, limits)
}

#[cfg(test)]
thread_local! {
    /// Smaller limits for the exports this thread writes, so a test reaches them through a whole
    /// export (its notes in the export's result) without 30,000 positions.
    pub(crate) static TEST_LIMITS: std::cell::Cell<Option<Limits>> = const { std::cell::Cell::new(None) };
}

/// [`to_flow`] within `limits`.
pub(crate) fn to_flow_within(doc: &ChessDoc, o: &OfficeOptions<'_>, notes: &mut Vec<String>, limits: Limits) -> (FlowDoc, usize) {
    let mut blocks = Vec::new();
    let mut drawn = 0;
    let mut boards: HashMap<Placement, Drawn> = HashMap::new();
    let mut bytes = 0usize;
    // Diagrams past the limits, set as their caption.
    let mut over = 0usize;
    for page in &doc.pages {
        let lang = page.text_language();
        let page_label = PageLabel::of(o.page_labels, page.index);
        let label = page_label.to_string();
        let of = page.blocks.iter().filter(|b| matches!(b, DocBlock::Diagram(_))).count();
        // The book's text: its number, running heads and a diagram's own text are not (plan
        // A9-L1b, A2-04).
        for (_, block) in page.book_blocks() {
            match block {
                DocBlock::Heading { level, inlines, .. } => blocks.push(FlowBlock::Heading { level: *level, runs: runs(inlines, o, lang) }),
                DocBlock::Paragraph { inlines, .. } => blocks.push(FlowBlock::Para(runs(inlines, o, lang))),
                DocBlock::Diagram(d) => {
                    let number = d.number.saturating_add(1);
                    // The position it shows, all told (plan A9-L6b).
                    let Some(shown) = d.shown() else {
                        blocks.push(FlowBlock::Para(vec![Run::text(describe::not_read(number, &label, lang))]));
                        continue;
                    };
                    // The caption: the PGN's comment for the diagram, its FEN, its link.
                    let origin = PgnOrigin {
                        page_index: page.index,
                        page_label: page_label.printed.clone(),
                        diagram: Some(number),
                        of: Some(of),
                        lang: Some(lang),
                        ..PgnOrigin::default()
                    };
                    let mut caption =
                        vec![Run::text(format!("{} · ", origin.comment())), Run { no_proof: true, ..Run::text(format!("FEN {}", shown.fen)) }];
                    if o.links
                        && let Some(url) = d.link()
                    {
                        caption.push(Run::text(" · "));
                        caption.push(Run { link: Some(url), ..Run::text(o.link_text) });
                    }
                    let placement = match pdfcraft_chess::fen::Fen::parse(shown.fen) {
                        Ok(f) => *f.placement(),
                        Err(e) => {
                            notes.push(format!("page {label}, diagram {number}: {e}; its FEN was set instead"));
                            blocks.push(FlowBlock::Para(caption));
                            continue;
                        }
                    };
                    if !boards.contains_key(&placement) {
                        if boards.len() >= limits.boards || bytes >= limits.bytes {
                            over = over.saturating_add(1);
                            blocks.push(FlowBlock::Para(caption));
                            continue;
                        }
                        // Word draws the SVG at any size; the PNG stays as its fallback. Neither
                        // names the position: the figure's text alternative does, and the same
                        // position is then the same picture (plan A9-X1b).
                        let colors = if o.svg { WORD_DIAGRAM_COLORS } else { palette::MAX_COLORS };
                        let made = diagram_png_of(&placement, o.pieces, &o.style, o.diagram_px, colors)
                            .map(|png| (png, o.svg.then(|| board_standalone(&placement, o.pieces, &o.style))));
                        if let Ok((png, svg)) = &made {
                            bytes = bytes.saturating_add(png.len()).saturating_add(svg.as_ref().map_or(0, String::len));
                        }
                        boards.insert(placement, made);
                    }
                    match boards.get(&placement) {
                        Some(Ok((png, svg))) => {
                            let alt = describe::describe_side_named(number, &label, shown.fen, shown.side_known, lang, o.naming);
                            let figure = Figure { png: png.clone(), svg: svg.clone(), side_pt: DIAGRAM_PT, alt, caption };
                            blocks.push(FlowBlock::Figure(figure));
                            drawn += 1;
                        }
                        Some(Err(e)) => {
                            notes.push(format!("page {label}, diagram {number}: {e}; its FEN was set instead"));
                            blocks.push(FlowBlock::Para(caption));
                        }
                        None => blocks.push(FlowBlock::Para(caption)),
                    }
                }
            }
        }
    }
    if over > 0 {
        notes.push(format!(
            "diagrams set as their FEN, past the most positions a file gets drawn ({}, or {} MB of pictures): {over}",
            limits.boards,
            limits.bytes / 1_000_000
        ));
    }
    let flow = FlowDoc { title: o.title.to_owned(), lang: o.lang.to_owned(), blocks };
    // Nothing is embedded: a font a run names must be installed where the file is read.
    let named = fonts(&flow);
    if !named.is_empty() {
        notes.push(format!("the document sets text in {}: it shows as meant only where the font is installed", named.join(", ")));
    }
    (flow, drawn)
}
