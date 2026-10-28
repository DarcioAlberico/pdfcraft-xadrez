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
//! - each diagram is a `<figure class="diagram" data-fen="…">` (the FEN as read), in the
//!   document's one reading order; its SVG board is named for a screen reader by a `<title>`
//!   (`Diagrama 1, pág. 17: brancas jogam`) and a `<desc>` listing each side's pieces, both in the
//!   book's language and joined by `aria-labelledby`, and its `<figcaption>` is the PGN's
//!   comment for it (`Pág. 17 · diagrama 1 de 2`) with the lichess link (plan A8 P9b); a diagram
//!   that was not read is a `<figure class="diagram unread">` with no `data-fen`;
//! - each move is a `<span class="move" data-san="…">` (English SAN), with `data-fen` (the
//!   position after it) when it was replayed, and `data-status` when it was not valid;
//! - each annotation is a `<span class="nag" data-nag="n">` with the printed glyph;
//! - anything flagged for review carries the class `review`.
//!
//! Only `http` and `https` addresses become links. Every size is bounded by the document's own.

use crate::text_font::{self, FontRole, TextFont};
use std::fmt::Write as _;

use pdfcraft_chess::describe;
use pdfcraft_chess::draw::{BoardStyle, PieceSet, board};
use pdfcraft_chess::origin::PgnOrigin;
use pdfcraft_chess::san::{FigurineSet, Lang, San, Style};
use pdfcraft_chess::svg::to_svg_described;

use super::{Block, ChessDoc, ChessPage, DiagramBlock, Inline, MoveInline, MoveStatus};

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
    /// The user's figurine font ([`crate::text_font`]): HTML embeds it, an EPUB carries it as
    /// [`epub_font_href`]. A Unicode one is used for the chess symbols, one that draws pieces on
    /// letters (SemFig) for the piece letters of the moves (with [`Figurines::Unicode`]).
    pub text_font: Option<&'a TextFont>,
    /// The user's sign font (ISChess): the annotation glyphs it draws are written in it.
    pub sign_font: Option<&'a TextFont>,
    /// The book's language for `<html lang>`, as a BCP 47 tag (empty: not said).
    pub lang: &'a str,
    /// The number printed on each page, by page index (empty, or an empty label: the page's own
    /// number from 1): diagrams are said with it.
    pub page_labels: &'a [String],
    /// Highlight what the reading flagged for review (the `review` class's background). A book
    /// for readers leaves it off; the class stays on the elements either way.
    pub review_marks: bool,
}

/// The number printed on page `index` (0-based), when the document gives one.
fn printed_label<'o>(o: &HtmlOptions<'o>, index: u32) -> Option<&'o str> {
    usize::try_from(index).ok().and_then(|i| o.page_labels.get(i)).map(|l| l.trim()).filter(|l| !l.is_empty())
}

/// Page `index` (0-based) as the book numbers it: its printed number, else its own from 1.
pub fn page_label(o: &HtmlOptions<'_>, index: u32) -> String {
    printed_label(o, index).map_or_else(|| (u64::from(index) + 1).to_string(), str::to_owned)
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

/// A move's text, and its markup inside the move's span. With a figurine font that draws pieces
/// on letters, the move is written with English letters and each piece letter (K Q R B N, never
/// a file or castling's O) is a `fig` span in that font.
fn move_markup(m: &MoveInline, o: &HtmlOptions<'_>) -> (String, String) {
    let letters = o.figurines == Figurines::Unicode && o.text_font.is_some_and(|f| f.role == FontRole::Letters);
    if letters && let Ok(san) = San::read(&m.san, Lang::En) {
        let number = m.number.map(|n| n.text()).unwrap_or_default();
        let body = san.write(Lang::En, Style::Letters);
        let mut html = esc(&number);
        for c in body.chars() {
            if matches!(c, 'K' | 'Q' | 'R' | 'B' | 'N') {
                let _ = write!(html, "<span class=\"fig\">{c}</span>");
            } else {
                html.push_str(&esc(&c.to_string()));
            }
        }
        return (format!("{number}{body}"), html);
    }
    let shown = move_text(m, o.figurines);
    let html = esc(&shown);
    (shown, html)
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
        MoveStatus::Caption => "caption",
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
            let (shown, body) = move_markup(m, o);
            let mut attrs = format!(" data-san=\"{}\"", esc(&m.san));
            if let Some(fen) = &m.fen {
                let _ = write!(attrs, " data-fen=\"{}\"", esc(fen));
            }
            if m.status != MoveStatus::Valid {
                let _ = write!(attrs, " data-status=\"{}\"", status_word(m.status));
            }
            (format!("<span class=\"move{review}\"{attrs}>{body}</span>"), shown)
        }
        Inline::Nag { code, .. } => {
            let (glyph, name) = (pdfcraft_chess::nag::shown(*code), pdfcraft_chess::nag::by_code(*code).map_or("", |n| n.name));
            let title = if name.is_empty() { String::new() } else { format!(" title=\"{}\"", esc(name)) };
            // With the sign font, the glyph is its letter (the title still names it).
            if o.sign_font.is_some()
                && let Some(letter) = text_font::ischess_letter(*code)
            {
                return (format!("<span class=\"nag sign{review}\" data-nag=\"{code}\"{title}>{letter}</span>"), glyph);
            }
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

/// The font a [`Span`] must be set in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanFont {
    /// The user's figurine font ([`HtmlOptions::text_font`]).
    Figurines,
    /// The user's sign font, ISChess ([`HtmlOptions::sign_font`]).
    Signs,
}

/// A piece of a paragraph as a document writer sets it (Word and RTF, plan A8 P9a): prose, or
/// chess (a move or an annotation, which spelling checks should skip), set in a font of its own
/// when it needs one, or a link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub chess: bool,
    pub font: Option<SpanFont>,
    pub link: Option<String>,
}

impl Span {
    fn prose(text: impl Into<String>) -> Span {
        Span { text: text.into(), chess: false, font: None, link: None }
    }

    fn chess(text: impl Into<String>, font: Option<SpanFont>) -> Span {
        Span { text: text.into(), chess: true, font, link: None }
    }
}

/// `spans` with `span` after them, joined to the last one when they read the same way.
fn push_span(spans: &mut Vec<Span>, span: Span) {
    if span.text.is_empty() {
        return;
    }
    match spans.last_mut() {
        Some(last) if last.chess == span.chess && last.font == span.font && last.link == span.link => last.text.push_str(&span.text),
        _ => spans.push(span),
    }
}

/// `inlines` as [`Span`]s, spaced as the HTML export spaces them, moves as `figurines` says:
/// with a figurine font that draws pieces on letters (SemFig, `text_font` of role
/// [`FontRole::Letters`]) and Unicode figurines, each piece letter is set in it, as the HTML's
/// `fig` spans; with a Unicode figurine font, whole moves are; with the sign font, an annotation
/// is its letter in it.
pub fn spans(inlines: &[Inline], figurines: Figurines, text_font: Option<FontRole>, sign_font: bool) -> Vec<Span> {
    let mut out = Vec::new();
    let mut last = String::new();
    for inline in inlines {
        let mut mine = Vec::new();
        let text = match inline {
            Inline::Text { text, .. } => {
                mine.push(Span::prose(text.clone()));
                text.clone()
            }
            Inline::Move(m) => {
                let letters = figurines == Figurines::Unicode && text_font == Some(FontRole::Letters);
                match San::read(&m.san, Lang::En) {
                    Ok(san) if letters => {
                        let number = m.number.map(|n| n.text()).unwrap_or_default();
                        let body = san.write(Lang::En, Style::Letters);
                        push_span(&mut mine, Span::chess(number.clone(), None));
                        for c in body.chars() {
                            let font = matches!(c, 'K' | 'Q' | 'R' | 'B' | 'N').then_some(SpanFont::Figurines);
                            push_span(&mut mine, Span::chess(c.to_string(), font));
                        }
                        format!("{number}{body}")
                    }
                    _ => {
                        let shown = move_text(m, figurines);
                        let unicode = figurines == Figurines::Unicode && text_font == Some(FontRole::Unicode);
                        mine.push(Span::chess(shown.clone(), unicode.then_some(SpanFont::Figurines)));
                        shown
                    }
                }
            }
            Inline::Nag { code, .. } => {
                let glyph = pdfcraft_chess::nag::shown(*code);
                match text_font::ischess_letter(*code).filter(|_| sign_font) {
                    Some(letter) => mine.push(Span::chess(letter.to_string(), Some(SpanFont::Signs))),
                    None => mine.push(Span::chess(glyph.clone(), None)),
                }
                glyph
            }
            Inline::Link { url, text, .. } => {
                let link = safe_url(url).then(|| url.clone());
                mine.push(Span { text: text.clone(), chess: false, font: None, link });
                text.clone()
            }
        };
        let glued = matches!(inline, Inline::Nag { .. }) || text.starts_with([')', ']', ',', '.', ';', ':', '!', '?']);
        if !out.is_empty() && !glued && !last.ends_with(['(', '[', ' ']) {
            push_span(&mut out, Span::prose(" "));
        }
        for span in mine {
            push_span(&mut out, span);
        }
        last = text;
    }
    out
}

/// Inlines as plain text, spaced as [`super::plain_text`], with moves printed as `figurines`
/// says: the text the Word and RTF exports set.
pub fn plain_text_with(inlines: &[Inline], figurines: Figurines) -> String {
    let mut out = String::new();
    for inline in inlines {
        let text = match inline {
            Inline::Move(m) => move_text(m, figurines),
            other => other.text(),
        };
        let glued = matches!(inline, Inline::Nag { .. }) || text.starts_with([')', ']', ',', '.', ';', ':', '!', '?']);
        if !out.is_empty() && !glued && !out.ends_with(['(', '[', ' ']) {
            out.push(' ');
        }
        out.push_str(&text);
    }
    out
}

fn diagram_html(d: &DiagramBlock, page: &ChessPage, o: &HtmlOptions<'_>) -> String {
    let review = if d.review.is_empty() && d.reading.as_ref().is_some_and(|r| r.accepted) { "" } else { " review" };
    let number = d.number.saturating_add(1);
    let id = format!("p{}-d{number}", u64::from(page.index) + 1);
    // Said in the book's language, with the number printed on the page (plan A8 P9b).
    let lang = Lang::from_tag(&page.lang).unwrap_or(Lang::En);
    let printed = printed_label(o, page.index);
    let label = page_label(o, page.index);
    let Some(reading) = &d.reading else {
        return format!(
            "<figure class=\"diagram unread review\" id=\"{id}\"><figcaption>{}</figcaption></figure>\n",
            esc(&describe::not_read(number, &label, lang))
        );
    };
    let fen = esc(&reading.fen);
    let body = match (o.diagrams, pdfcraft_chess::fen::parse(&reading.fen)) {
        (DiagramMode::Svg, Ok(parsed)) => {
            let title = describe::title(number, &label, parsed.side_to_move, lang);
            to_svg_described(&board(&parsed.placement, o.pieces, &o.style), &id, &title, &describe::pieces(&parsed.placement, lang))
        }
        _ => format!("<code>{fen}</code>"),
    };
    // The caption is the PGN's comment for the diagram, then its link.
    let of = page.blocks.iter().filter(|b| matches!(b, Block::Diagram(_))).count();
    let origin = PgnOrigin {
        page_index: page.index,
        page_label: printed.map(str::to_owned),
        diagram: Some(number),
        of: Some(of),
        lang: Some(lang),
        ..PgnOrigin::default()
    };
    let mut caption = format!("<figcaption>{}", esc(&origin.comment()));
    if o.links
        && let Some(url) = d.lichess_url.as_deref().filter(|u| safe_url(u))
    {
        let _ = write!(caption, " · <a href=\"{}\">{}</a>", esc(url), esc(o.link_text));
    }
    caption.push_str("</figcaption>");
    format!("<figure class=\"diagram{review}\" id=\"{id}\" data-fen=\"{fen}\">{body}{caption}</figure>\n")
}

/// The highlight of what was flagged for review ([`HtmlOptions::review_marks`]).
const REVIEW_RULE: &str = ".review{background:#fff3c4}";

/// The style both outputs share.
const STYLE: &str = "body{max-width:46em;margin:2em auto;padding:0 1em;font-family:serif;line-height:1.5}\
.move{white-space:nowrap}.review{background:#fff3c4}\
figure.diagram{margin:1.2em auto;text-align:center}figure.diagram svg{max-width:100%;height:auto;width:20em}\
figcaption{font-family:sans-serif;font-size:.85em}section.page+section.page{border-top:1px solid #ccc;margin-top:2em}";

/// Where an EPUB carries a font, relative to its pages: `fonts/figurines.*` or `fonts/signs.*`.
pub fn epub_font_href(font: &TextFont) -> String {
    let name = if font.role == FontRole::Signs { "signs" } else { "figurines" };
    format!("fonts/{name}.{}", font.format.extension())
}

/// The style, with the user's fonts loaded from their `src`. A Unicode figurine font goes first
/// in each font family (its `unicode-range` keeps it to the chess symbols, so the prose keeps the
/// body font); a figurine font by letters and the sign font are used only by the `fig` and `sign`
/// spans.
fn style(figurines: Option<(&TextFont, &str)>, signs: Option<(&TextFont, &str)>, review_marks: bool) -> String {
    let mut faces = String::new();
    let mut css = STYLE.to_owned();
    if !review_marks {
        css = css.replace(REVIEW_RULE, "");
    }
    if let Some((f, src)) = figurines {
        faces.push_str(&text_font::font_face(f, src));
        if f.role == FontRole::Unicode {
            css = css.replace("font-family:", &format!("font-family:\"{}\",", text_font::FAMILY));
        } else {
            let _ = write!(css, ".fig{{font-family:\"{}\"}}", text_font::FAMILY);
        }
    }
    if let Some((f, src)) = signs {
        faces.push_str(&text_font::font_face(f, src));
        let _ = write!(css, ".sign{{font-family:\"{}\"}}", text_font::SIGN_FAMILY);
    }
    format!("{faces}{css}")
}

/// The anchor of heading `k` (0-based, in order) of page `index`.
pub fn heading_id(index: u32, k: usize) -> String {
    format!("p{}-h{}", u64::from(index) + 1, k + 1)
}

/// One page as a `<section>`; with `pagebreak`, it starts with the EPUB page break that names
/// the page as the book numbers it (`id="pagebreak"`, the target of the package's page list).
fn page_section(page: &ChessPage, o: &HtmlOptions<'_>, pagebreak: bool) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "<section class=\"page\" data-page=\"{}\">", u64::from(page.index) + 1);
    if pagebreak {
        let label = esc(&page_label(o, page.index));
        let _ = writeln!(s, "<span epub:type=\"pagebreak\" role=\"doc-pagebreak\" id=\"pagebreak\" aria-label=\"{label}\" title=\"{label}\"></span>");
    }
    let mut k = 0;
    for block in &page.blocks {
        match block {
            Block::Heading { level, inlines, .. } => {
                let l = (*level).clamp(1, 6);
                let _ = writeln!(s, "<h{l} id=\"{}\">{}</h{l}>", heading_id(page.index, k), inlines_html(inlines, o));
                k += 1;
            }
            Block::Paragraph { inlines, .. } => {
                let _ = writeln!(s, "<p>{}</p>", inlines_html(inlines, o));
            }
            Block::Diagram(d) => s.push_str(&diagram_html(d, page, o)),
        }
    }
    s.push_str("</section>\n");
    s
}

/// The document as one HTML file.
pub fn write(doc: &ChessDoc, o: &HtmlOptions<'_>) -> String {
    // The file stays self-contained: the fonts go in as data URLs.
    let (fig_src, sign_src) = (o.text_font.map(text_font::data_url), o.sign_font.map(text_font::data_url));
    let lang: String = o.lang.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(35).collect();
    let html = if lang.is_empty() { "<html>".to_owned() } else { format!("<html lang=\"{lang}\">") };
    let mut s = format!(
        "<!DOCTYPE html>\n{html}\n<head>\n<meta charset=\"utf-8\">\n<title>{}</title>\n<style>{}</style>\n</head>\n<body>\n",
        esc(o.title),
        style(o.text_font.zip(fig_src.as_deref()), o.sign_font.zip(sign_src.as_deref()), o.review_marks),
    );
    for page in &doc.pages {
        s.push_str(&page_section(page, o, false));
    }
    s.push_str("</body>\n</html>\n");
    s
}

/// Where an EPUB's pages find their one stylesheet ([`epub_css`]), relative to them.
pub const EPUB_CSS: &str = "styles.css";

/// The stylesheet an EPUB's pages share, its fonts loaded from [`epub_font_href`] (plan A8 P9b:
/// one file in the package, not a copy in every page).
pub fn epub_css(o: &HtmlOptions<'_>) -> String {
    let (fig, sign) = (o.text_font.map(epub_font_href), o.sign_font.map(epub_font_href));
    style(o.text_font.zip(fig.as_deref()), o.sign_font.zip(sign.as_deref()), o.review_marks)
}

/// One page as a standalone XHTML document (an EPUB 3 content document): well-formed XML, the
/// same markup as [`write`] inside, starting with the page break that names the page, and the
/// package's stylesheet ([`EPUB_CSS`]). `lang` is the book's language tag.
pub fn xhtml_page(page: &ChessPage, o: &HtmlOptions<'_>, lang: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" lang=\"{l}\" xml:lang=\"{l}\">\n<head>\n<meta charset=\"utf-8\"/>\n<title>{t}</title>\n<link rel=\"stylesheet\" type=\"text/css\" href=\"{EPUB_CSS}\"/>\n</head>\n<body>\n{body}</body>\n</html>\n",
        l = esc(lang),
        t = esc(o.title),
        body = page_section(page, o, true),
    )
}

/// A heading for a table of contents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TocEntry {
    /// The page's index in the document, from 0.
    pub page: u32,
    pub level: u8,
    pub text: String,
    /// Its anchor ([`heading_id`]).
    pub id: String,
}

/// The document's headings, in order.
pub fn toc(doc: &ChessDoc) -> Vec<TocEntry> {
    let mut out = Vec::new();
    for page in &doc.pages {
        let mut k = 0;
        for block in &page.blocks {
            if let Block::Heading { level, inlines, .. } = block {
                out.push(TocEntry { page: page.index, level: (*level).clamp(1, 6), text: super::plain_text(inlines), id: heading_id(page.index, k) });
                k += 1;
            }
        }
    }
    out
}

/// Escape text for HTML and XML (the writer's own escaping, for other parts of a package).
pub fn escape(s: &str) -> String {
    esc(s)
}
