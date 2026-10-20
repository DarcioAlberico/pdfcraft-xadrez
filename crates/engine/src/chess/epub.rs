//! Chess ▸ Export ▸ EPUB (blueprint S15b): the recognised document as an EPUB 3 book.
//!
//! The package is the HTML export cut by page: one XHTML content document per PDF page
//! (`chessvision::doc::html::xhtml_page`, the same markup and inline SVG boards), a navigation
//! document listing the headings (or the pages, when there are none), the package document and
//! the container. `mimetype` comes first and is stored, as OCF requires. Nothing is fetched and
//! nothing runs: no script, no remote resource. No font is embedded: figurines use the reader's
//! own fonts (an embedded face waits for one with figurines in craft-fonts).

use pdfcraft_chessvision::doc::ChessDoc;
use pdfcraft_chessvision::doc::html::{HtmlOptions, epub_font_href, escape, toc, xhtml_page};
use pdfcraft_export::Zip;

/// The most pages a book holds (each is a file in the archive, whose count is 16-bit).
pub const MAX_PAGES: usize = 60_000;

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
    if let Some(font) = o.text_font {
        manifest.push_str(&format!("<item id=\"font\" href=\"{}\" media-type=\"{}\"/>\n", epub_font_href(font), font.format.mime()));
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
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"book-id\" xml:lang=\"{l}\">\n<metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n<dc:identifier id=\"book-id\">{id}</dc:identifier>\n<dc:title>{t}</dc:title>\n<dc:language>{l}</dc:language>\n<meta property=\"dcterms:modified\">{m}</meta>\n</metadata>\n<manifest>\n{manifest}</manifest>\n<spine>\n{spine}</spine>\n</package>\n",
            l = escape(lang),
            id = escape(&meta.identifier),
            t = escape(&meta.title),
            m = escape(&meta.modified),
        ),
    ));
    out.extend(pages);
    // The figurine font the pages' style loads (`html::epub_font_href`).
    if let Some(font) = o.text_font {
        out.push(EpubEntry { name: format!("OEBPS/{}", epub_font_href(font)), data: font.bytes.clone(), compress: true });
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
