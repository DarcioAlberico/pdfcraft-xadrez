//! Chess ▸ Export ▸ EPUB (blueprint S15b): the recognised document as an EPUB 3 book.
//!
//! The package is the HTML export cut by page: one XHTML content document per PDF page
//! (`chessvision::doc::html::xhtml_page`, the same markup and inline SVG boards), a navigation
//! document listing the headings (or the pages, when there are none), the package document and
//! the container. `mimetype` comes first and is stored, as OCF requires. Nothing is fetched and
//! nothing runs: no script, no remote resource. No font is embedded: figurines use the reader's
//! own fonts (an embedded face waits for one with figurines in craft-fonts).
//!
//! The cover is the book's first page drawn by the page renderer ([`cover_png`]), at most
//! [`MAX_COVER_PX`] on its long side: the manifest item has `properties="cover-image"` (EPUB 3),
//! and `<meta name="cover"/>` names it for EPUB 2 readers.

use pdfcraft_chessvision::doc::ChessDoc;
use pdfcraft_chessvision::doc::html::{HtmlOptions, epub_font_href, escape, toc, xhtml_page};
use pdfcraft_export::Zip;
use pdfcraft_render::{PageRenderer, RenderRequest};

use crate::export::{ExportSource, encode_png};

/// The most pages a book holds (each is a file in the archive, whose count is 16-bit).
pub const MAX_PAGES: usize = 60_000;
/// The longest side of the cover image, in pixels.
pub const MAX_COVER_PX: u32 = 1600;
/// The cover's file in the package, and its manifest id.
const COVER_FILE: &str = "cover.png";
const COVER_ID: &str = "cover-image";

/// What the package says about the book.
#[derive(Clone, Debug, PartialEq)]
pub struct EpubMeta {
    pub title: String,
    /// BCP 47 language tag of the text.
    pub lang: String,
    /// A stable identifier (`dc:identifier`): see [`identifier`].
    pub identifier: String,
    /// `dcterms:modified`, `CCYY-MM-DDThh:mm:ssZ`: see [`modified`].
    pub modified: String,
    /// The cover, a PNG ([`cover_png`]); `None` (or empty) writes a book without one.
    pub cover: Option<Vec<u8>>,
}

/// One file of the package.
#[derive(Clone, Debug, PartialEq)]
pub struct EpubEntry {
    pub name: String,
    pub data: Vec<u8>,
    /// Deflate it (everything but `mimetype`).
    pub compress: bool,
}

/// A stable identifier for a book made from these PDF bytes: the same file gives the same id,
/// so a reader recognises a re-export as the same book. FNV-1a, not a security hash.
pub fn identifier(pdf: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in pdf {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("urn:pdfcraft:chess:{h:016x}")
}

/// `dcterms:modified` from a PDF date (`D:YYYYMMDDHHmmSS…`, as `pdfcraft_cos::pdf_date` makes,
/// taken as UTC), or the epoch when it does not parse.
pub fn modified(pdf_date: &str) -> String {
    let digits: String = pdf_date.trim_start_matches("D:").chars().take_while(char::is_ascii_digit).take(14).collect();
    if digits.len() < 14 {
        return "1970-01-01T00:00:00Z".into();
    }
    // 14 ASCII digits: every slice below is on a char boundary.
    let part = |a: usize, b: usize| digits.get(a..b).unwrap_or("00");
    format!("{}-{}-{}T{}:{}:{}Z", part(0, 4), part(4, 6), part(6, 8), part(8, 10), part(10, 12), part(12, 14))
}

/// `dcterms:modified` for a moment in seconds since the Unix epoch (UTC).
pub fn modified_at(secs: i64) -> String {
    modified(&pdfcraft_cos::pdf_date(secs))
}

/// Page `page` (0-based) of `src` as a PNG whose long side is [`MAX_COVER_PX`] pixels (fewer when
/// the renderer's size limits ask): the book's cover.
pub fn cover_png(src: ExportSource, page: usize) -> Result<Vec<u8>, String> {
    let &(w, h) = src.sizes.get(page).ok_or_else(|| format!("page {} does not exist", page.saturating_add(1)))?;
    let long = w.max(h);
    if !(long.is_finite() && long > 0.0) {
        return Err(format!("page {} has no size to draw", page.saturating_add(1)));
    }
    // Half a pixel short, so the renderer's rounding up stays within the limit.
    let scale = (MAX_COVER_PX as f32 - 0.5) / long;
    let mut renderer = PageRenderer::new(src.bytes, src.config);
    let shot = renderer.render(RenderRequest { page, scale, ..Default::default() });
    if let Some(e) = shot.error {
        return Err(format!("page {}: {e}", page.saturating_add(1)));
    }
    if shot.width.max(shot.height) > MAX_COVER_PX || shot.width == 0 || shot.height == 0 {
        return Err(format!("page {} drew at {}×{} pixels", page.saturating_add(1), shot.width, shot.height));
    }
    encode_png(shot.width, shot.height, &shot.rgba)
}

fn page_file(n: usize) -> String {
    format!("page-{:05}.xhtml", n + 1)
}

/// The package's files, in archive order. Fails for a document of more than [`MAX_PAGES`] pages.
pub fn entries(doc: &ChessDoc, o: &HtmlOptions<'_>, meta: &EpubMeta) -> Result<Vec<EpubEntry>, String> {
    if doc.pages.len() > MAX_PAGES {
        return Err(format!("the document has {} pages; a book holds at most {MAX_PAGES}", doc.pages.len()));
    }
    let lang = if meta.lang.trim().is_empty() { "en" } else { meta.lang.trim() };
    let entry = |name: &str, data: String| EpubEntry { name: name.into(), data: data.into_bytes(), compress: true };
    let mut out = vec![EpubEntry { name: "mimetype".into(), data: b"application/epub+zip".to_vec(), compress: false }];
    out.push(entry(
        "META-INF/container.xml",
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\n<rootfiles><rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles>\n</container>\n".into(),
    ));

    let mut manifest = String::from("<item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n");
    let mut spine = String::new();
    let mut pages = Vec::with_capacity(doc.pages.len());
    for (n, page) in doc.pages.iter().enumerate() {
        let xhtml = xhtml_page(page, o, lang);
        // EPUB 3 asks content documents with inline SVG to say so.
        let svg = if xhtml.contains("<svg") { " properties=\"svg\"" } else { "" };
        manifest.push_str(&format!("<item id=\"p{}\" href=\"{}\" media-type=\"application/xhtml+xml\"{svg}/>\n", n + 1, page_file(n)));
        spine.push_str(&format!("<itemref idref=\"p{}\"/>\n", n + 1));
        pages.push(entry(&format!("OEBPS/{}", page_file(n)), xhtml));
    }
    for (id, font) in [("font", o.text_font), ("signs", o.sign_font)] {
        if let Some(font) = font {
            manifest.push_str(&format!("<item id=\"{id}\" href=\"{}\" media-type=\"{}\"/>\n", epub_font_href(font), font.format.mime()));
        }
    }
    let cover = meta.cover.as_deref().filter(|png| !png.is_empty());
    let mut cover_meta = "";
    if cover.is_some() {
        manifest.push_str(&format!("<item id=\"{COVER_ID}\" href=\"{COVER_FILE}\" media-type=\"image/png\" properties=\"cover-image\"/>\n"));
        cover_meta = "<meta name=\"cover\" content=\"cover-image\"/>\n";
    }

    // The table of contents: the headings, flat (each links to its anchor), else the pages.
    let headings = toc(doc);
    let mut items = String::new();
    if headings.is_empty() {
        for (n, page) in doc.pages.iter().enumerate() {
            items.push_str(&format!("<li><a href=\"{}\">Page {}</a></li>\n", page_file(n), u64::from(page.index) + 1));
        }
    } else {
        for h in &headings {
            let file = doc.pages.iter().position(|p| p.index == h.page).map(page_file).unwrap_or_default();
            let text = if h.text.trim().is_empty() { format!("Page {}", u64::from(h.page) + 1) } else { h.text.clone() };
            items.push_str(&format!("<li><a href=\"{file}#{}\">{}</a></li>\n", escape(&h.id), escape(&text)));
        }
    }
    if items.is_empty() {
        // A navigation document needs at least one entry: an empty book links to its own start.
        items.push_str("<li><a href=\"nav.xhtml\">Contents</a></li>\n");
    }
    out.push(entry(
        "OEBPS/nav.xhtml",
        format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" lang=\"{l}\" xml:lang=\"{l}\">\n<head>\n<meta charset=\"utf-8\"/>\n<title>{t}</title>\n</head>\n<body>\n<nav epub:type=\"toc\" id=\"toc\">\n<h1>{t}</h1>\n<ol>\n{items}</ol>\n</nav>\n</body>\n</html>\n",
            l = escape(lang),
            t = escape(&meta.title),
        ),
    ));
    out.push(entry(
        "OEBPS/content.opf",
        format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"book-id\" xml:lang=\"{l}\">\n<metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n<dc:identifier id=\"book-id\">{id}</dc:identifier>\n<dc:title>{t}</dc:title>\n<dc:language>{l}</dc:language>\n<meta property=\"dcterms:modified\">{m}</meta>\n{cover_meta}</metadata>\n<manifest>\n{manifest}</manifest>\n<spine>\n{spine}</spine>\n</package>\n",
            l = escape(lang),
            id = escape(&meta.identifier),
            t = escape(&meta.title),
            m = escape(&meta.modified),
        ),
    ));
    out.extend(pages);
    // The fonts the pages' style loads (`html::epub_font_href`).
    for font in [o.text_font, o.sign_font].into_iter().flatten() {
        out.push(EpubEntry { name: format!("OEBPS/{}", epub_font_href(font)), data: font.bytes.clone(), compress: true });
    }
    // A PNG is compressed already.
    if let Some(png) = cover {
        out.push(EpubEntry { name: format!("OEBPS/{COVER_FILE}"), data: png.to_vec(), compress: false });
    }
    Ok(out)
}

/// The book as an `.epub` file.
pub fn epub(doc: &ChessDoc, o: &HtmlOptions<'_>, meta: &EpubMeta) -> Result<Vec<u8>, String> {
    let mut zip = Zip::default();
    for e in entries(doc, o, meta)? {
        zip.add(&e.name, &e.data, e.compress);
    }
    Ok(zip.finish())
}
