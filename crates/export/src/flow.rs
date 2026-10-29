//! A flowing document written as Word or RTF (plan A8 P9a): headings, paragraphs of runs and
//! figures, in order, with what a reader needs that a page layout does not say.
//!
//! Export a PDF ▸ Word (the [`super::docx`] writer) rebuilds pages from their boxes. A document
//! that already knows its structure, such as a recognised chess book, writes it here instead:
//!
//! - a [`Run`] says its font (a figurine or sign font the text needs), its language when it is
//!   not the document's, and whether spelling and grammar checks should skip it (`w:noProof`,
//!   `\noproof`: chess moves are not words);
//! - a [`Figure`] is a picture with its text alternative (`wp:docPr descr` in Word,
//!   `wzDescription` in RTF) and a caption paragraph under it, in Word's `Caption` style, the
//!   picture kept with it on one page (`keepNext`); Word also gets its SVG when it has one
//!   (`asvg:svgBlip`, the PNG kept as the fallback older readers show);
//! - a link is a real hyperlink, for `http`, `https` and `mailto` addresses only
//!   ([`super::link_target`]);
//! - the document's language is the default of every run (`w:lang`; `\deflang` and `\lang` in
//!   RTF, by the Windows language number [`rtf_lang`] knows), and RTF lists every font it names in
//!   its font table, so a reader can see which ones must be installed.
//!
//! Every text is escaped for its format and stripped of what XML does not allow.

use super::{Zip, emu, esc, link_target, png_size, rtf_text};

/// One run of text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    /// The font family it must be set in (a figurine or sign font), else the document's.
    pub font: Option<String>,
    /// Its language when it is not the document's, as a BCP 47 tag.
    pub lang: Option<String>,
    /// Spelling and grammar checks skip it.
    pub no_proof: bool,
    /// A hyperlink to this address.
    pub link: Option<String>,
}

impl Run {
    /// Plain text in the document's font and language.
    pub fn text(text: impl Into<String>) -> Run {
        Run { text: text.into(), ..Run::default() }
    }
}

/// A picture with its text alternative and caption.
#[derive(Clone, Debug, PartialEq)]
pub struct Figure {
    /// A PNG.
    pub png: Vec<u8>,
    /// The same picture as a standalone SVG, which Word draws at any size instead of the PNG.
    pub svg: Option<String>,
    /// Its side on the page, in points (it is square).
    pub side_pt: f64,
    /// What it shows, for a reader who cannot see it.
    pub alt: String,
    /// The caption under it; none when empty.
    pub caption: Vec<Run>,
}

/// One block of the document.
#[derive(Clone, Debug, PartialEq)]
pub enum FlowBlock {
    /// A heading, level 1 to 3 (deeper ones are set as 3).
    Heading {
        level: u8,
        runs: Vec<Run>,
    },
    Para(Vec<Run>),
    Figure(Figure),
}

/// A document to write.
#[derive(Clone, Debug, PartialEq)]
pub struct FlowDoc {
    pub title: String,
    /// The language of its text, as a BCP 47 tag (empty: not said).
    pub lang: String,
    pub blocks: Vec<FlowBlock>,
}

/// A language tag as the formats take it: letters, digits and `-`, at most 35 characters.
fn lang_tag(tag: &str) -> String {
    tag.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(35).collect()
}

/// A font family name as both formats can carry it: no control characters, nothing RTF or a
/// font table entry would read (`\`, `{`, `}`, `;`), at most 64 characters.
fn font_name(name: &str) -> String {
    name.chars().filter(|c| !c.is_control() && !matches!(c, '\\' | '{' | '}' | ';')).take(64).collect::<String>().trim().to_owned()
}

/// The fonts the document's runs name, each once, in the order they first appear.
pub fn fonts(doc: &FlowDoc) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let runs = doc.blocks.iter().flat_map(|b| match b {
        FlowBlock::Heading { runs, .. } | FlowBlock::Para(runs) => runs.as_slice(),
        FlowBlock::Figure(f) => f.caption.as_slice(),
    });
    for name in runs.filter_map(|r| r.font.as_deref()).map(font_name).filter(|n| !n.is_empty()) {
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

// ---------------------------------------------------------------- Word

/// The run's properties, in the order the schema wants them (rStyle, rFonts, noProof, lang).
fn run_props(run: &Run, link: bool) -> String {
    let mut p = String::new();
    if link {
        p.push_str("<w:rStyle w:val=\"Hyperlink\"/>");
    }
    if let Some(font) = run.font.as_deref().map(font_name).filter(|f| !f.is_empty()) {
        let f = esc(&font);
        p.push_str(&format!("<w:rFonts w:ascii=\"{f}\" w:hAnsi=\"{f}\" w:cs=\"{f}\"/>"));
    }
    if run.no_proof {
        p.push_str("<w:noProof/>");
    }
    if let Some(lang) = run.lang.as_deref().map(lang_tag).filter(|l| !l.is_empty()) {
        p.push_str(&format!("<w:lang w:val=\"{lang}\"/>"));
    }
    if p.is_empty() { p } else { format!("<w:rPr>{p}</w:rPr>") }
}

/// The runs of a paragraph, each link a `w:hyperlink` to a relationship added to `links`.
fn docx_runs(runs: &[Run], links: &mut Vec<String>) -> String {
    let mut out = String::new();
    for run in runs {
        // Tabs and line breaks become spaces: a run is one line of a paragraph.
        let text: String = run.text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
        let target = run.link.as_deref().and_then(link_target);
        let r = format!("<w:r>{}<w:t xml:space=\"preserve\">{}</w:t></w:r>", run_props(run, target.is_some()), esc(&text));
        match target {
            Some(url) => {
                links.push(url);
                out.push_str(&format!("<w:hyperlink r:id=\"rIdLink{}\" w:history=\"1\">{r}</w:hyperlink>", links.len()));
            }
            None => out.push_str(&r),
        }
    }
    out
}

/// The document as a `.docx` file.
pub fn docx(doc: &FlowDoc) -> Vec<u8> {
    let mut body = String::new();
    let mut links: Vec<String> = Vec::new();
    let mut media: Vec<&[u8]> = Vec::new();
    let mut svgs: Vec<(usize, &str)> = Vec::new();
    for block in &doc.blocks {
        match block {
            FlowBlock::Heading { level, runs } => {
                let level = (*level).clamp(1, 3);
                body.push_str(&format!("<w:p><w:pPr><w:pStyle w:val=\"Heading{level}\"/></w:pPr>{}</w:p>", docx_runs(runs, &mut links)));
            }
            FlowBlock::Para(runs) => body.push_str(&format!("<w:p>{}</w:p>", docx_runs(runs, &mut links))),
            FlowBlock::Figure(f) => {
                media.push(&f.png);
                let n = media.len();
                // The SVG as an extension of the picture: Word 2016 and later draw it, others the
                // PNG (plan A8 P9a).
                let svg_blip = match f.svg.as_deref() {
                    Some(svg) => {
                        svgs.push((n, svg));
                        format!("<a:extLst><a:ext uri=\"{SVG_EXT}\"><asvg:svgBlip xmlns:asvg=\"{SVG_NS}\" r:embed=\"rIdSvg{n}\"/></a:ext></a:extLst>")
                    }
                    None => String::new(),
                };
                let side = if f.side_pt.is_finite() && f.side_pt > 0.0 { f.side_pt.min(468.0) } else { 216.0 };
                let (cx, cy) = (emu(side), emu(side));
                let alt = esc(&f.alt.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>());
                body.push_str(&format!(
                    "<w:p><w:pPr><w:pStyle w:val=\"Figure\"/></w:pPr><w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\">\
<wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:docPr id=\"{n}\" name=\"Diagram {n}\" descr=\"{alt}\"/>\
<wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect=\"1\"/></wp:cNvGraphicFramePr>\
<a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
<pic:pic><pic:nvPicPr><pic:cNvPr id=\"{n}\" name=\"diagram{n}.png\" descr=\"{alt}\"/><pic:cNvPicPr/></pic:nvPicPr>\
<pic:blipFill><a:blip r:embed=\"rIdImg{n}\">{svg_blip}</a:blip><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
<pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic>\
</a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"
                ));
                if !f.caption.is_empty() {
                    body.push_str(&format!("<w:p><w:pPr><w:pStyle w:val=\"Caption\"/></w:pPr>{}</w:p>", docx_runs(&f.caption, &mut links)));
                }
            }
        }
    }
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><w:body>{body}\
<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/></w:sectPr>\
</w:body></w:document>"
    );
    let lang = lang_tag(&doc.lang);
    let lang = if lang.is_empty() { String::new() } else { format!("<w:lang w:val=\"{lang}\"/>") };
    let styles = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
<w:docDefaults><w:rPrDefault><w:rPr>{lang}</w:rPr></w:rPrDefault></w:docDefaults>\
<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:qFormat/><w:pPr><w:spacing w:after=\"120\"/></w:pPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Heading1\"><w:name w:val=\"heading 1\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before=\"240\"/><w:outlineLvl w:val=\"0\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"36\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Heading2\"><w:name w:val=\"heading 2\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before=\"200\"/><w:outlineLvl w:val=\"1\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"30\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Heading3\"><w:name w:val=\"heading 3\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before=\"160\"/><w:outlineLvl w:val=\"2\"/></w:pPr><w:rPr><w:b/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Figure\"><w:name w:val=\"Figure\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Caption\"/><w:pPr><w:keepNext/><w:spacing w:before=\"120\" w:after=\"60\"/><w:jc w:val=\"center\"/></w:pPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Caption\"><w:name w:val=\"caption\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:spacing w:after=\"200\"/><w:jc w:val=\"center\"/></w:pPr><w:rPr><w:i/><w:sz w:val=\"18\"/></w:rPr></w:style>\
<w:style w:type=\"character\" w:styleId=\"Hyperlink\"><w:name w:val=\"Hyperlink\"/><w:rPr><w:color w:val=\"0563C1\"/><w:u w:val=\"single\"/></w:rPr></w:style>\
</w:styles>"
    );
    let types = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Default Extension=\"png\" ContentType=\"image/png\"/><Default Extension=\"svg\" ContentType=\"image/svg+xml\"/>\
<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>\
<Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/>\
<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/></Types>";
    let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>\
<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/></Relationships>";
    let mut doc_rels = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rIdStyles\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>",
    );
    for n in 1..=media.len() {
        doc_rels.push_str(&format!(
            "<Relationship Id=\"rIdImg{n}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/diagram{n}.png\"/>"
        ));
    }
    for (n, _) in &svgs {
        doc_rels.push_str(&format!(
            "<Relationship Id=\"rIdSvg{n}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/diagram{n}.svg\"/>"
        ));
    }
    for (i, target) in links.iter().enumerate() {
        doc_rels.push_str(&format!(
            "<Relationship Id=\"rIdLink{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"{}\" TargetMode=\"External\"/>",
            i + 1,
            esc(target)
        ));
    }
    doc_rels.push_str("</Relationships>");
    let dc_lang = if doc.lang.trim().is_empty() { String::new() } else { format!("<dc:language>{}</dc:language>", lang_tag(&doc.lang)) };
    let core = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>{}</dc:title>{dc_lang}</cp:coreProperties>",
        esc(&doc.title)
    );
    let mut z = Zip::default();
    z.add("[Content_Types].xml", types.as_bytes(), true);
    z.add("_rels/.rels", rels.as_bytes(), true);
    z.add("docProps/core.xml", core.as_bytes(), true);
    z.add("word/document.xml", document.as_bytes(), true);
    z.add("word/styles.xml", styles.as_bytes(), true);
    z.add("word/_rels/document.xml.rels", doc_rels.as_bytes(), true);
    for (i, png) in media.iter().enumerate() {
        z.add(&format!("word/media/diagram{}.png", i + 1), png, false);
    }
    for (n, svg) in &svgs {
        z.add(&format!("word/media/diagram{n}.svg"), svg.as_bytes(), true);
    }
    z.finish()
}

/// The extension that carries a picture's SVG in Office Open XML (Office 2016), and its namespace.
const SVG_EXT: &str = "{96DAC541-7B7A-43D3-8B79-37D633B846F1}";
const SVG_NS: &str = "http://schemas.microsoft.com/office/drawing/2016/SVG/main";

// ---------------------------------------------------------------- RTF

/// The Windows language number RTF names a language by (`\langN`), for a BCP 47 tag: the 18
/// languages of chess notation and some others, a region where it changes the number (en-GB,
/// pt-PT, nn). `None` for a language not listed: the text is then left unmarked.
pub fn rtf_lang(tag: &str) -> Option<u16> {
    let tag = lang_tag(tag).to_ascii_lowercase();
    let mut parts = tag.split('-');
    let primary = parts.next().unwrap_or("");
    let region = parts.next().unwrap_or("");
    Some(match (primary, region) {
        ("en", "gb") => 2057,
        ("en", _) => 1033,
        ("pt", "pt") => 2070,
        ("pt", _) => 1046,
        ("de", _) => 1031,
        ("es", _) => 3082,
        ("fr", _) => 1036,
        ("it", _) => 1040,
        ("ru", _) => 1049,
        ("nl", _) => 1043,
        ("pl", _) => 1045,
        ("cs", _) => 1029,
        ("hu", _) => 1038,
        ("da", _) => 1030,
        ("sv", _) => 1053,
        ("nn", _) => 2068,
        ("no" | "nb", _) => 1044,
        ("fi", _) => 1035,
        ("ro", _) => 1048,
        ("is", _) => 1039,
        ("ca", _) => 1027,
        ("sk", _) => 1051,
        ("sl", _) => 1060,
        ("hr", _) => 1050,
        ("tr", _) => 1055,
        ("el", _) => 1032,
        ("uk", _) => 1058,
        ("bg", _) => 1026,
        ("lt", _) => 1063,
        ("lv", _) => 1062,
        ("et", _) => 1061,
        ("ja", _) => 1041,
        ("zh", _) => 2052,
        ("ko", _) => 1042,
        ("ar", _) => 1025,
        ("he", _) => 1037,
        ("vi", _) => 1066,
        _ => return None,
    })
}

/// The runs of a paragraph as RTF: each in its own group with its font and `\noproof`, a link as
/// a `HYPERLINK` field.
fn rtf_runs(runs: &[Run], fonts: &[String]) -> String {
    let mut out = String::new();
    for run in runs {
        let mut fmt = String::new();
        if let Some(i) = run.font.as_deref().map(font_name).and_then(|f| fonts.iter().position(|e| *e == f)) {
            fmt.push_str(&format!("\\f{}", i + 1));
        }
        if let Some(n) = run.lang.as_deref().and_then(rtf_lang) {
            fmt.push_str(&format!("\\lang{n}"));
        }
        if run.no_proof {
            fmt.push_str("\\noproof");
        }
        let text: String = run.text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
        let text = rtf_text(&text);
        let group = format!("{{{fmt} {text}}}");
        match run.link.as_deref().and_then(link_target) {
            Some(url) => out.push_str(&format!("{{\\field{{\\*\\fldinst{{HYPERLINK \"{url}\"}}}}{{\\fldrslt{{\\ul {group}}}}}}}")),
            None => out.push_str(&group),
        }
    }
    out
}

/// A PNG as an RTF picture, `side` points a side, its text alternative in the picture's
/// properties (`wzDescription`, which Word shows as the alt text); the bytes as hex.
fn rtf_figure(png: &[u8], side: f64, alt: &str) -> String {
    let twips = if side.is_finite() && side > 0.0 { (side.min(468.0) * 20.0).round() as i64 } else { 4320 };
    let (pw, ph) = png_size(png).map_or((twips / 20, twips / 20), |(x, y)| (i64::from(x), i64::from(y)));
    let alt: String = alt.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let mut s = format!(
        "{{\\pard\\qc\\keepn{{\\pict{{\\*\\picprop{{\\sp{{\\sn wzDescription}}{{\\sv {}}}}}}}\\pngblip\\picw{pw}\\pich{ph}\\picwgoal{twips}\\pichgoal{twips}\n",
        rtf_text(&alt)
    );
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for line in png.chunks(64) {
        for b in line {
            s.push(char::from(HEX[usize::from(b >> 4)]));
            s.push(char::from(HEX[usize::from(b & 15)]));
        }
        s.push('\n');
    }
    s.push_str("}\\par}\n");
    s
}

/// The document as Rich Text Format. The font table lists every font a run names after the body
/// font, so a reader sees which must be installed where the file is read.
pub fn rtf(doc: &FlowDoc) -> String {
    let fonts = fonts(doc);
    let mut s = String::from("{\\rtf1\\ansi\\deff0");
    // The document's language, the default of every run (`\plain` and each paragraph return to it).
    if let Some(n) = rtf_lang(&doc.lang) {
        s.push_str(&format!("\\deflang{n}"));
    }
    s.push_str("{\\fonttbl{\\f0\\froman Times New Roman;}");
    for (i, f) in fonts.iter().enumerate() {
        s.push_str(&format!("{{\\f{}\\fnil {};}}", i + 1, rtf_text(f)));
    }
    s.push_str("}\n");
    let title: String = doc.title.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    s.push_str(&format!("{{\\info{{\\title {}}}}}\n", rtf_text(&title)));
    for block in &doc.blocks {
        match block {
            FlowBlock::Heading { level, runs } => {
                let level = (*level).clamp(1, 3);
                let size = match level {
                    1 => 36,
                    2 => 30,
                    _ => 24,
                };
                s.push_str(&format!("{{\\pard\\keepn\\outlinelevel{}\\sb240\\sa120\\b\\fs{size} {}\\par}}\n", level - 1, rtf_runs(runs, &fonts)));
            }
            FlowBlock::Para(runs) => s.push_str(&format!("{{\\pard\\sa120\\fs22 {}\\par}}\n", rtf_runs(runs, &fonts))),
            FlowBlock::Figure(f) => {
                s.push_str(&rtf_figure(&f.png, f.side_pt, &f.alt));
                if !f.caption.is_empty() {
                    s.push_str(&format!("{{\\pard\\qc\\sa200\\i\\fs18 {}\\par}}\n", rtf_runs(&f.caption, &fonts)));
                }
            }
        }
    }
    s.push('}');
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::part;

    fn hostile() -> FlowDoc {
        let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x02\0\0\0\x03".to_vec();
        FlowDoc {
            title: "A <\"book\"> & {co}\\".into(),
            lang: "pt-BR\"><x".into(),
            blocks: vec![
                FlowBlock::Heading { level: 9, runs: vec![Run::text("Deep")] },
                FlowBlock::Heading { level: 0, runs: vec![Run::text("Top")] },
                FlowBlock::Para(vec![
                    Run::text("tab\there & <there>\u{1}"),
                    Run { font: Some("Sem}Fig;\\".into()), no_proof: true, ..Run::text("N{}\\") },
                    Run { link: Some("javascript:alert(1)".into()), ..Run::text("not a link") },
                    Run { link: Some("https://example.org/?a=1&b=\"2\"".into()), ..Run::text("a link") },
                    Run { lang: Some("de".into()), ..Run::text("Zug") },
                ]),
                FlowBlock::Figure(Figure {
                    png,
                    svg: Some("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\"/>".into()),
                    side_pt: f64::NAN,
                    alt: "Diagram \"1\" <a> & {b}\n".into(),
                    caption: vec![Run::text("cap")],
                }),
            ],
        }
    }

    /// Plan A8 P9a: whatever the text, the Word parts are well-formed XML, RTF groups balance, a
    /// link goes only to a web address, and heading levels stay 1 to 3.
    #[test]
    fn any_text_stays_text_in_word_and_rtf() {
        let doc = hostile();
        let word = docx(&doc);
        for name in
            ["[Content_Types].xml", "_rels/.rels", "docProps/core.xml", "word/document.xml", "word/styles.xml", "word/_rels/document.xml.rels"]
        {
            roxmltree::Document::parse(&part(&word, name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
        let xml = part(&word, "word/document.xml");
        assert!(xml.contains("Heading3") && xml.contains("Heading1") && !xml.contains("Heading9") && !xml.contains("Heading0"));
        assert_eq!(xml.matches("<w:hyperlink").count(), 1, "javascript: is no link");
        assert!(xml.contains("<w:rFonts w:ascii=\"SemFig\" w:hAnsi=\"SemFig\" w:cs=\"SemFig\"/><w:noProof/>"), "{xml}");
        assert!(xml.contains("<w:lang w:val=\"de\"/>"), "{xml}");
        assert!(xml.contains("descr=\"Diagram &quot;1&quot; &lt;a&gt; &amp; {b} \""), "{xml}");
        assert!(part(&word, "word/styles.xml").contains("<w:lang w:val=\"pt-BRx\"/>"), "only a tag's own characters");
        assert_eq!(fonts(&doc), ["SemFig"]);
        // The figure's SVG goes with its PNG (plan A8 P9a).
        assert!(xml.contains("<a:blip r:embed=\"rIdImg1\"><a:extLst><a:ext uri=\"{96DAC541-7B7A-43D3-8B79-37D633B846F1}\"><asvg:svgBlip xmlns:asvg=\"http://schemas.microsoft.com/office/drawing/2016/SVG/main\" r:embed=\"rIdSvg1\"/>"), "{xml}");
        assert!(part(&word, "word/_rels/document.xml.rels").contains(
            "Id=\"rIdSvg1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/diagram1.svg\""
        ));
        assert!(part(&word, "[Content_Types].xml").contains("Extension=\"svg\" ContentType=\"image/svg+xml\""));
        roxmltree::Document::parse(&part(&word, "word/media/diagram1.svg")).unwrap();

        let rich = rtf(&doc);
        // Every group closes: count the braces that are not escaped.
        let (mut depth, mut escaped) = (0i64, false);
        for c in rich.chars() {
            if escaped {
                escaped = false;
                continue;
            }
            match c {
                '\\' => escaped = true,
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
            assert!(depth >= 0, "{rich}");
        }
        assert_eq!(depth, 0, "{rich}");
        assert!(rich.contains(r"{\f1\fnil SemFig;}") && rich.contains(r"{\f1\noproof N\{\}\\}"), "{rich}");
        assert_eq!(rich.matches("HYPERLINK").count(), 1, "{rich}");
        // The document's language and a run's own, by their Windows numbers.
        assert!(rich.starts_with(r"{\rtf1\ansi\deff0\deflang1046{\fonttbl"), "{rich}");
        assert!(rich.contains(r"{\lang1031 Zug}"), "{rich}");
        assert!(rich.contains(r"\picwgoal4320\pichgoal4320"), "a side that is not a number is three inches: {rich}");
    }

    #[test]
    fn rtf_names_languages_by_their_windows_numbers() {
        assert_eq!(rtf_lang("pt-BR"), Some(1046));
        assert_eq!(rtf_lang("pt-PT"), Some(2070));
        assert_eq!(rtf_lang("EN-gb"), Some(2057));
        assert_eq!(rtf_lang("en"), Some(1033));
        assert_eq!(rtf_lang("nb"), Some(1044));
        assert_eq!(rtf_lang("nn-NO"), Some(2068));
        assert_eq!(rtf_lang("ca"), Some(1027));
        assert_eq!(rtf_lang(""), None);
        assert_eq!(rtf_lang("tlh"), None);
        let doc = FlowDoc {
            title: String::new(),
            lang: String::new(),
            blocks: vec![FlowBlock::Para(vec![Run { lang: Some("xx".into()), ..Run::text("a") }])],
        };
        let rich = rtf(&doc);
        assert!(!rich.contains(r"\lang") && !rich.contains(r"\deflang"), "no language said, none written: {rich}");
    }
}
