//! A recognised chess document as one HTML file (blueprint S15a).
//!
//! The page reads as the book does: headings, paragraphs with their moves, and each diagram
//! redrawn as an inline SVG board (`pdfcraft_chess::draw`, the drawing the PDF replacement uses)
//! with a link to the position on lichess under it. The file is static and self-contained: no
//! script, no stylesheet or font fetched, no external image.
//!
//! What a program reading it back can rely on:
//!
//! - each page is a `<section class="page" data-page="n">` (1-based), in order;
//! - each diagram is a `<figure class="diagram" data-fen="…">` (the FEN as read; `alt`/`title`
//!   say it too), in the document's one reading order; a diagram that was not read is a
//!   `<figure class="diagram unread">` with no `data-fen`;
//! - each move is a `<span class="move" data-san="…">` (English SAN), with `data-fen` (the
//!   position after it) when it was replayed, and `data-status` when it was not valid;
//! - each annotation is a `<span class="nag" data-nag="n">` with the printed glyph;
//! - anything flagged for review carries the class `review`.
//!
//! Only `http` and `https` addresses become links. Every size is bounded by the document's own.

use std::fmt::Write as _;

use pdfcraft_chess::draw::{BoardStyle, PieceSet, board};
use pdfcraft_chess::san::{FigurineSet, Lang, San, Style};
use pdfcraft_chess::svg::to_svg;

use super::{Block, ChessDoc, DiagramBlock, Inline, MoveInline, MoveStatus};

/// How diagrams appear.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DiagramMode {
    /// A redrawn board as inline SVG, with the FEN as its accessible name.
    #[default]
    Svg,
    /// The FEN as text only.
    Fen,
}

/// How piece letters in moves are printed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Figurines {
    /// Unicode chess glyphs (♞f3), the print convention.
    Unicode,
    /// The letters of a language (Cf3 in Portuguese).
    Letters(Lang),
    /// As the book printed them.
    AsPrinted,
}

/// What [`write`] makes.
#[derive(Clone, Debug)]
pub struct HtmlOptions<'a> {
    pub title: &'a str,
    pub pieces: &'a PieceSet,
    pub style: BoardStyle,
    pub diagrams: DiagramMode,
    pub figurines: Figurines,
    /// A link to lichess under each diagram that has one.
    pub links: bool,
    /// The text of that link.
    pub link_text: &'a str,
}

/// Escape for text and for attribute values in double quotes.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c if c.is_control() && c != '\n' && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}

/// An address that may be a link: `http` or `https`, nothing a browser would run.
fn safe_url(url: &str) -> bool {
    let lower = url.trim_start().to_ascii_lowercase();
    (lower.starts_with("http://") || lower.starts_with("https://")) && !url.chars().any(|c| c.is_control() || c.is_whitespace())
}

/// The move as it is printed in the file.
fn move_text(m: &MoveInline, figurines: Figurines) -> String {
    let number = m.number.map(|n| n.text()).unwrap_or_default();
    let body = match (figurines, San::read(&m.san, Lang::En)) {
        (Figurines::AsPrinted, _) | (_, Err(_)) => m.text.clone(),
        (Figurines::Unicode, Ok(san)) => san.write(Lang::En, Style::Figurine(FigurineSet::Solid)),
        (Figurines::Letters(lang), Ok(san)) => san.write(lang, Style::Letters),
    };
    format!("{number}{body}")
}

fn status_word(s: MoveStatus) -> &'static str {
    match s {
        MoveStatus::Valid => "valid",
        MoveStatus::Illegal => "illegal",
        MoveStatus::Ambiguous => "ambiguous",
        MoveStatus::OutOfSequence => "out_of_sequence",
        MoveStatus::NotReached => "not_reached",
        MoveStatus::Unchecked => "unchecked",
    }
}

/// The inline's markup and its text as it reads (for spacing).
fn inline_html(inline: &Inline, o: &HtmlOptions<'_>) -> (String, String) {
    let review = if inline.review().is_empty() { "" } else { " review" };
    match inline {
        Inline::Text { text, .. } => {
            let html = if review.is_empty() { esc(text) } else { format!("<span class=\"review\">{}</span>", esc(text)) };
            (html, text.clone())
        }
        Inline::Move(m) => {
            let shown = move_text(m, o.figurines);
            let mut attrs = format!(" data-san=\"{}\"", esc(&m.san));
            if let Some(fen) = &m.fen {
                let _ = write!(attrs, " data-fen=\"{}\"", esc(fen));
            }
            if m.status != MoveStatus::Valid {
                let _ = write!(attrs, " data-status=\"{}\"", status_word(m.status));
            }
            (format!("<span class=\"move{review}\"{attrs}>{}</span>", esc(&shown)), shown)
        }
        Inline::Nag { code, .. } => {
            let (glyph, name) =
                pdfcraft_chess::nag::by_code(*code).map_or_else(|| (format!("${code}"), String::new()), |n| (n.glyph.to_owned(), n.name.to_owned()));
            let title = if name.is_empty() { String::new() } else { format!(" title=\"{}\"", esc(&name)) };
            (format!("<span class=\"nag{review}\" data-nag=\"{code}\"{title}>{}</span>", esc(&glyph)), glyph)
        }
        Inline::Link { url, text, .. } => {
            let html = if safe_url(url) { format!("<a class=\"{}\" href=\"{}\">{}</a>", review.trim(), esc(url), esc(text)) } else { esc(text) };
            (html.replace(" class=\"\"", ""), text.clone())
        }
    }
}

/// Inlines joined with the spacing of [`super::plain_text`].
fn inlines_html(inlines: &[Inline], o: &HtmlOptions<'_>) -> String {
    let mut out = String::new();
    let mut last = String::new();
    for inline in inlines {
        let (html, text) = inline_html(inline, o);
        let glued = matches!(inline, Inline::Nag { .. }) || text.starts_with([')', ']', ',', '.', ';', ':', '!', '?']);
        if !out.is_empty() && !glued && !last.ends_with(['(', '[', ' ']) {
            out.push(' ');
        }
        out.push_str(&html);
        last = text;
    }
    out
}

fn diagram_html(d: &DiagramBlock, page: u32, o: &HtmlOptions<'_>) -> String {
    let review = if d.review.is_empty() && d.reading.as_ref().is_some_and(|r| r.accepted) { "" } else { " review" };
    let id = format!("p{}-d{}", u64::from(page) + 1, d.number + 1);
    let Some(reading) = &d.reading else {
        return format!(
            "<figure class=\"diagram unread review\" id=\"{id}\"><figcaption>Diagram {} (not read)</figcaption></figure>\n",
            d.number + 1
        );
    };
    let fen = esc(&reading.fen);
    let body = match (o.diagrams, pdfcraft_chess::fen::parse(&reading.fen)) {
        (DiagramMode::Svg, Ok(parsed)) => to_svg(&board(&parsed.placement, o.pieces, &o.style), &reading.fen),
        _ => format!("<code>{fen}</code>"),
    };
    let mut caption = String::new();
    if o.links
        && let Some(url) = d.lichess_url.as_deref().filter(|u| safe_url(u))
    {
        let _ = write!(caption, "<figcaption><a href=\"{}\">{}</a></figcaption>", esc(url), esc(o.link_text));
    }
    format!("<figure class=\"diagram{review}\" id=\"{id}\" data-fen=\"{fen}\">{body}{caption}</figure>\n")
}

/// The document as one HTML file.
pub fn write(doc: &ChessDoc, o: &HtmlOptions<'_>) -> String {
    let mut s = format!(
        "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<title>{}</title>\n<style>\
body{{max-width:46em;margin:2em auto;padding:0 1em;font-family:serif;line-height:1.5}}\
.move{{white-space:nowrap}}.review{{background:#fff3c4}}\
figure.diagram{{margin:1.2em auto;text-align:center}}figure.diagram svg{{max-width:100%;height:auto;width:20em}}\
figcaption{{font-family:sans-serif;font-size:.85em}}section.page+section.page{{border-top:1px solid #ccc;margin-top:2em}}\
</style>\n</head>\n<body>\n",
        esc(o.title)
    );
    for page in &doc.pages {
        let _ = writeln!(s, "<section class=\"page\" data-page=\"{}\">", u64::from(page.index) + 1);
        for block in &page.blocks {
            match block {
                Block::Heading { level, inlines, .. } => {
                    let l = (*level).clamp(1, 6);
                    let _ = writeln!(s, "<h{l}>{}</h{l}>", inlines_html(inlines, o));
                }
                Block::Paragraph { inlines, .. } => {
                    let _ = writeln!(s, "<p>{}</p>", inlines_html(inlines, o));
                }
                Block::Diagram(d) => s.push_str(&diagram_html(d, page.index, o)),
            }
        }
        s.push_str("</section>\n");
    }
    s.push_str("</body>\n</html>\n");
    s
}
