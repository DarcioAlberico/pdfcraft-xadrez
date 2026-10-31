//! Importing piece sets into the user's `pieces` folder (plan HC3).
//!
//! A set comes from a folder of twelve piece images (copied as they are) or from a saved web
//! page that carries sets inside it: `data:image/svg+xml` values keyed `wK`…`bP` (as JSON or
//! a script's object), plain or base64. The page is searched as text, with no HTML parser and
//! no script run; a page that only fetches its pieces from the web has none to import.
//!
//! Every piece is read through the same SVG or PNG reader as the user's folders before anything
//! is written, so a file that points elsewhere or draws nothing is refused. What is imported has
//! no licence PdfCraft knows: each folder gets an `ORIGIN.txt` saying so, and the user is told
//! it is for personal use. Nothing already there is replaced unless asked.
//!
//! Chess fonts are imported too ([`import_fonts`]), into the `fonts` folder beside the program
//! rather than the pieces folder: a diagram font there is a piece set, and a figurine or sign
//! font writes the moves of the exports ([`crate::text_font`]). A font carries its own licence in
//! its name table; PdfCraft copies it as it is and ships none.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use base64::Engine as _;

use super::images::{self, PIECE_STEMS};
use super::svg::{self, MAX_SVG_BYTES};

/// Largest page read.
const MAX_PAGE_BYTES: usize = 64 << 20;
/// Most sets taken from one page.
const MAX_SETS: usize = 64;
/// Most bytes decoded from one page, all pieces together.
const MAX_DECODED_BYTES: usize = 64 << 20;
/// How far before a set's first piece its name is looked for.
const NAME_WINDOW: usize = 256;
/// Longest folder name written.
const MAX_NAME_CHARS: usize = 40;
/// The note written into every imported folder.
pub const ORIGIN_FILE: &str = "ORIGIN.txt";
/// Most font files looked at in one folder.
const MAX_FONT_FILES: usize = 256;

/// Why pieces could not be imported.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("cannot read {path}: {source}")]
    Read { path: PathBuf, source: std::io::Error },
    #[error("cannot write {path}: {source}")]
    Write { path: PathBuf, source: std::io::Error },
    #[error("the page is {0} bytes, more than the {MAX_PAGE_BYTES} accepted")]
    TooLarge(usize),
    #[error("{0} holds no piece set PdfCraft can import (twelve pieces named wK to bP)")]
    NoSets(PathBuf),
    #[error("the folder {dir} lacks {missing}")]
    Incomplete { dir: PathBuf, missing: String },
    #[error("the set {set} cannot be used: {reason}")]
    Invalid { set: String, reason: String },
    #[error("already there: {}", names(.0))]
    Exists(Vec<PathBuf>),
    #[error("{0} is the folder it would be imported into")]
    Same(PathBuf),
    #[error("{0} holds no chess font PdfCraft can use (a diagram font it knows, or a figurine or sign font)")]
    NoFonts(PathBuf),
}

fn names(dirs: &[PathBuf]) -> String {
    dirs.iter().filter_map(|d| d.file_name()).map(|n| n.to_string_lossy().into_owned()).collect::<Vec<_>>().join(", ")
}

/// A set found in a page: its name and its twelve SVG files by stem (`wK`…).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageSet {
    pub name: String,
    pub pieces: BTreeMap<&'static str, Vec<u8>>,
}

/// The sets a page carries, and how many groups of pieces fell short of twelve.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Found {
    pub sets: Vec<PageSet>,
    pub incomplete: usize,
}

/// A set to write: its folder name and its files (name and bytes).
type SetFiles = (String, Vec<(String, Vec<u8>)>);

/// A set written to the pieces folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Imported {
    pub name: String,
    pub dir: PathBuf,
}

/// What an imported font is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontUse {
    /// A chess diagram font: its pieces are a set to choose.
    Pieces,
    /// A figurine or sign font: the exports write moves with it.
    Text,
}

/// A chess font copied into the fonts folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportedFont {
    /// Its family, as the piece picker names it.
    pub name: String,
    /// Where it is now.
    pub path: PathBuf,
    pub kind: FontUse,
}

/// Whether `path` is for [`import_fonts`] rather than [`import`]: a font file, or a folder with
/// font files and no piece images.
pub fn holds_fonts(path: &Path) -> bool {
    if path.is_dir() {
        return !images::is_piece_folder(path) && !font_files(path).is_empty();
    }
    is_font_file(path)
}

/// Copy the chess fonts among `from` (font files, or folders of them) into `into`, the fonts
/// folder beside the program: the diagram fonts PdfCraft can draw pieces with
/// ([`super::from_font_bytes`], their layout checked), and the figurine and sign fonts the
/// exports write moves with ([`crate::text_font`]). Other fonts are passed over, and so is a
/// second font of a family already taken (DiagPDF keeps a patched copy beside some). Nothing is
/// written unless there is one at least, and a file of the same name already in `into` is
/// replaced only when `replace` is set ([`ImportError::Exists`] names them otherwise); an
/// identical one is left as it is. One font at a time is held in memory.
pub fn import_fonts(from: &[PathBuf], into: &Path, replace: bool) -> Result<Vec<ImportedFont>, ImportError> {
    let mut files = Vec::new();
    for path in from {
        if path.is_dir() {
            if same_place(path, into) {
                return Err(ImportError::Same(path.clone()));
            }
            files.extend(font_files(path));
        } else {
            files.push(path.clone());
        }
    }
    // Each font taken, with the file it comes from.
    let mut plan: Vec<(ImportedFont, PathBuf)> = Vec::new();
    for path in files {
        let Some((name, kind)) = chess_font(&path)? else { continue };
        if plan.iter().any(|(f, _)| f.kind == kind && f.name == name) {
            continue;
        }
        let Some(file) = path.file_name() else { continue };
        plan.push((ImportedFont { name, path: into.join(file), kind }, path));
    }
    if plan.is_empty() {
        return Err(ImportError::NoFonts(from.first().cloned().unwrap_or_default()));
    }
    // A file there already with other bytes waits for Replace; one with the same bytes stays.
    let mut copies = Vec::new();
    let mut taken = Vec::new();
    for (font, source) in &plan {
        if !font.path.exists() {
            copies.push((source, &font.path));
        } else if !same_bytes(source, &font.path)? {
            taken.push(font.path.clone());
            copies.push((source, &font.path));
        }
    }
    if !taken.is_empty() && !replace {
        return Err(ImportError::Exists(taken));
    }
    if !copies.is_empty() {
        std::fs::create_dir_all(into).map_err(|source| ImportError::Write { path: into.to_path_buf(), source })?;
    }
    for (source, target) in copies {
        std::fs::copy(source, target).map_err(|e| ImportError::Write { path: target.clone(), source: e })?;
    }
    Ok(plan.into_iter().map(|(font, _)| font).collect())
}

/// Whether two files hold the same bytes; a file there that cannot be read is taken as other.
fn same_bytes(source: &Path, there: &Path) -> Result<bool, ImportError> {
    let size = |p: &Path| std::fs::metadata(p).map(|m| m.len()).ok();
    if size(source) != size(there) {
        return Ok(false);
    }
    let ours = std::fs::read(source).map_err(|e| ImportError::Read { path: source.to_path_buf(), source: e })?;
    Ok(std::fs::read(there).is_ok_and(|theirs| theirs == ours))
}

/// The font files directly in `dir`, by name, at most [`MAX_FONT_FILES`].
fn font_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut found: Vec<PathBuf> = entries.flatten().take(10_000).map(|e| e.path()).filter(|p| is_font_file(p)).collect();
    found.sort();
    found.truncate(MAX_FONT_FILES);
    found
}

fn is_font_file(path: &Path) -> bool {
    path.is_file() && path.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "ttf" | "otf"))
}

/// The chess font in `path`: its family and what it is for; `None` for any other font, or a
/// file that is no font. Only a file that cannot be read at all is an error.
fn chess_font(path: &Path) -> Result<Option<(String, FontUse)>, ImportError> {
    let size = std::fs::metadata(path).map_err(|source| ImportError::Read { path: path.to_path_buf(), source })?.len();
    if usize::try_from(size).unwrap_or(usize::MAX) > super::MAX_FONT_BYTES {
        return Ok(None);
    }
    let bytes = std::fs::read(path).map_err(|source| ImportError::Read { path: path.to_path_buf(), source })?;
    let hint = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
    if let Ok(font) = super::read_font(&bytes, hint) {
        // A layout only presumed would be imported and then never offered.
        return Ok(font.offered().then_some((font.set.name, FontUse::Pieces)));
    }
    // A figurine or sign font PdfCraft knows; a font with the Unicode chess symbols is any text
    // font, not a chess one.
    match crate::text_font::from_bytes(bytes, hint) {
        Ok(font) if font.role != crate::text_font::FontRole::Unicode => {
            let name = if font.family.is_empty() { hint.to_owned() } else { font.family };
            Ok(Some((name, FontUse::Text)))
        }
        _ => Ok(None),
    }
}

/// Import the sets in `from` (a folder of piece images or a saved web page) into `into`, one
/// folder each. Nothing is written unless every set reads, and a folder already in `into` is
/// replaced only when `replace` is set ([`ImportError::Exists`] names them otherwise).
pub fn import(from: &Path, into: &Path, replace: bool) -> Result<Vec<Imported>, ImportError> {
    let plan = if from.is_dir() { folder_set(from)? } else { page_sets(from)? };
    let targets: Vec<PathBuf> = plan.iter().map(|(name, _)| into.join(name)).collect();
    if from.is_dir() && targets.iter().any(|t| same_place(t, from)) {
        return Err(ImportError::Same(from.to_path_buf()));
    }
    let taken: Vec<PathBuf> = targets.iter().filter(|t| t.exists()).cloned().collect();
    if !taken.is_empty() && !replace {
        return Err(ImportError::Exists(taken));
    }
    let origin = from.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut out = Vec::new();
    for ((name, files), dir) in plan.into_iter().zip(targets) {
        write_set(&dir, &files, &origin)?;
        out.push(Imported { name, dir });
    }
    Ok(out)
}

/// The sets of a page read from `path`, as [`sets_in_page`] finds them, each checked; or why none.
fn page_sets(path: &Path) -> Result<Vec<SetFiles>, ImportError> {
    let size = std::fs::metadata(path).map_err(|source| ImportError::Read { path: path.to_path_buf(), source })?.len();
    let size = usize::try_from(size).unwrap_or(usize::MAX);
    if size > MAX_PAGE_BYTES {
        return Err(ImportError::TooLarge(size));
    }
    let bytes = std::fs::read(path).map_err(|source| ImportError::Read { path: path.to_path_buf(), source })?;
    let found = sets_in_page(&bytes)?;
    if found.sets.is_empty() {
        return Err(ImportError::NoSets(path.to_path_buf()));
    }
    let mut out = Vec::new();
    for set in found.sets {
        for (stem, svg) in &set.pieces {
            svg::glyph_from_svg(svg).map_err(|reason| ImportError::Invalid { set: set.name.clone(), reason: format!("{stem}: {reason}") })?;
        }
        let files = set.pieces.into_iter().map(|(stem, svg)| (format!("{stem}.svg"), svg)).collect();
        out.push((set.name, files));
    }
    Ok(out)
}

/// A folder's twelve images, read as a set first so a folder that cannot be drawn is refused.
fn folder_set(dir: &Path) -> Result<Vec<SetFiles>, ImportError> {
    let missing = images::missing_pieces(dir);
    if !missing.is_empty() {
        return Err(ImportError::Incomplete { dir: dir.to_path_buf(), missing: missing.join(", ") });
    }
    let name = folder_name(dir.file_name().map(|n| n.to_string_lossy().into_owned()).as_deref().unwrap_or_default(), 1);
    images::from_image_dir(dir).map_err(|e| ImportError::Invalid { set: name.clone(), reason: e.to_string() })?;
    let mut files = Vec::new();
    for path in images::piece_files(dir) {
        let bytes = std::fs::read(&path).map_err(|source| ImportError::Read { path: path.clone(), source })?;
        let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        files.push((file, bytes));
    }
    Ok(vec![(name, files)])
}

/// Write one set's files into `dir`, after taking away the piece images already there (an SVG
/// left behind would be read before a PNG written now).
fn write_set(dir: &Path, files: &[(String, Vec<u8>)], origin: &str) -> Result<(), ImportError> {
    let write_err = |path: &Path| {
        let path = path.to_path_buf();
        move |source| ImportError::Write { path, source }
    };
    std::fs::create_dir_all(dir).map_err(write_err(dir))?;
    for old in images::piece_files(dir) {
        std::fs::remove_file(&old).map_err(write_err(&old))?;
    }
    for (file, bytes) in files {
        let path = dir.join(file);
        std::fs::write(&path, bytes).map_err(write_err(&path))?;
    }
    let note = format!(
        "These pieces were imported by PdfCraft from {origin}.\n\
         Their licence is not known: use them for personal use only, and do not share them\n\
         unless their author allows it.\n"
    );
    let path = dir.join(ORIGIN_FILE);
    std::fs::write(&path, note).map_err(write_err(&path))
}

/// Whether `a` and `b` are one folder.
fn same_place(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// The sets of twelve SVG pieces a web page carries, in the order they appear. A group of pieces
/// that does not reach twelve before a key repeats is counted in [`Found::incomplete`].
pub fn sets_in_page(bytes: &[u8]) -> Result<Found, ImportError> {
    if bytes.len() > MAX_PAGE_BYTES {
        return Err(ImportError::TooLarge(bytes.len()));
    }
    let text = String::from_utf8_lossy(bytes);
    let mut found = Found::default();
    let mut current: BTreeMap<&'static str, Vec<u8>> = BTreeMap::new();
    let mut name: Option<String> = None;
    let mut decoded = 0usize;
    let mut used: Vec<String> = Vec::new();
    let finish = |current: &mut BTreeMap<&'static str, Vec<u8>>, name: &mut Option<String>, found: &mut Found, used: &mut Vec<String>| {
        if current.len() == PIECE_STEMS.len() {
            let base = name.take().unwrap_or_default();
            let mut n = 1;
            let mut unique = folder_name(&base, found.sets.len() + 1);
            while used.contains(&unique) {
                n += 1;
                unique = format!("{}-{n}", folder_name(&base, found.sets.len() + 1));
            }
            used.push(unique.clone());
            found.sets.push(PageSet { name: unique, pieces: std::mem::take(current) });
        } else if !current.is_empty() {
            found.incomplete = found.incomplete.saturating_add(1);
        }
        current.clear();
        *name = None;
    };
    for (key_start, stem, value) in keyed_svgs(&text) {
        if found.sets.len() >= MAX_SETS {
            break;
        }
        if current.contains_key(stem) {
            finish(&mut current, &mut name, &mut found, &mut used);
        }
        let Some(svg) = decode_data_url(value) else { continue };
        decoded = decoded.saturating_add(svg.len());
        if decoded > MAX_DECODED_BYTES {
            break;
        }
        if current.is_empty() {
            name = set_name(&text, key_start);
        }
        current.insert(stem, svg);
        if current.len() == PIECE_STEMS.len() {
            finish(&mut current, &mut name, &mut found, &mut used);
        }
    }
    finish(&mut current, &mut name, &mut found, &mut used);
    Ok(found)
}

/// Every `data:image/svg+xml` string value whose key is a piece stem: where the key starts, the
/// stem and the value as written (escapes still in it). Linear in the page: each quote is looked
/// for from where the last search ended, and a key only in the few bytes before its value.
fn keyed_svgs(text: &str) -> Vec<(usize, &'static str, &str)> {
    const MARK: &str = "data:image/svg+xml";
    let lower = text.to_ascii_lowercase();
    let bytes = text.as_bytes();
    let mut quotes = [Quotes::new(bytes, b'"'), Quotes::new(bytes, b'\'')];
    let mut out = Vec::new();
    let mut after = 0;
    for (start, _) in lower.match_indices(MARK) {
        if start < after {
            continue; // inside the value just taken
        }
        let Some(open) = start.checked_sub(1) else { continue };
        let Some(cursor) = quotes.iter_mut().find(|q| bytes.get(open) == Some(&q.quote)) else { continue };
        let Some(end) = cursor.after(start) else { continue };
        let (Some(value), Some((key_start, stem))) = (text.get(start..end), key_before(text, open)) else { continue };
        if end - start > MAX_VALUE_BYTES {
            continue;
        }
        after = end;
        out.push((key_start, stem, value));
    }
    out
}

/// Longest value looked at: an SVG of [`MAX_SVG_BYTES`] percent-encoded throughout.
const MAX_VALUE_BYTES: usize = MAX_SVG_BYTES * 3 + 64;
/// How far before a value its key is looked for.
const KEY_WINDOW: usize = 64;

/// The unescaped `quote`s of a text, found in order: each search starts where the last ended,
/// so the searches of one page cost one pass over it.
struct Quotes<'a> {
    bytes: &'a [u8],
    quote: u8,
    /// The last quote found, and where the search stopped.
    found: Option<usize>,
    scanned: usize,
}

impl<'a> Quotes<'a> {
    fn new(bytes: &'a [u8], quote: u8) -> Quotes<'a> {
        Quotes { bytes, quote, found: None, scanned: 0 }
    }

    /// The first unescaped quote at or after `from`; `from` never goes back between calls.
    fn after(&mut self, from: usize) -> Option<usize> {
        if let Some(at) = self.found.filter(|&at| at >= from) {
            return Some(at);
        }
        let mut i = from.max(self.scanned);
        while let Some(&b) = self.bytes.get(i) {
            if b == self.quote && !escaped(self.bytes, i) {
                (self.found, self.scanned) = (Some(i), i + 1);
                return Some(i);
            }
            i += 1;
        }
        (self.found, self.scanned) = (None, i);
        None
    }
}

/// Whether the byte at `i` follows an odd run of backslashes.
fn escaped(bytes: &[u8], i: usize) -> bool {
    let run = bytes.get(..i).map_or(0, |head| head.iter().rev().take_while(|&&b| b == b'\\').count());
    run % 2 == 1
}

/// The piece stem keying the value whose opening quote is at `quote_at`: `"wK": "…"`, `wK:'…'`
/// or `wK = "…"`, with the key's start; looked for in the [`KEY_WINDOW`] bytes before it.
fn key_before(text: &str, quote_at: usize) -> Option<(usize, &'static str)> {
    let from = quote_at.saturating_sub(KEY_WINDOW);
    let from = (from..=quote_at).find(|&i| text.is_char_boundary(i))?;
    let window = text.get(from..quote_at)?.trim_end();
    let window = window.strip_suffix(':').or_else(|| window.strip_suffix('='))?.trim_end();
    let (head, key) = match window.chars().last()? {
        q @ ('"' | '\'') => {
            let inner = window.get(..window.len() - 1)?;
            let open = inner.rfind(q)?;
            (inner.get(..open)?, inner.get(open + 1..)?)
        }
        _ => {
            let start = window.char_indices().rev().find(|(_, c)| !c.is_ascii_alphanumeric()).map_or(0, |(i, c)| i + c.len_utf8());
            (window.get(..start)?, window.get(start..)?)
        }
    };
    let stem = PIECE_STEMS.iter().copied().find(|s| s.eq_ignore_ascii_case(key))?;
    Some((from + head.len(), stem))
}

/// The name a set's object is keyed by in the page (`"fresca": {"wK": …`), looked for just
/// before its first piece's key.
fn set_name(text: &str, key_start: usize) -> Option<String> {
    let from = key_start.saturating_sub(NAME_WINDOW);
    let from = (from..key_start).find(|&i| text.is_char_boundary(i))?;
    let head = text.get(from..key_start)?.trim_end();
    let head = head.strip_suffix('{')?.trim_end().strip_suffix(':')?.trim_end();
    let q = head.chars().last().filter(|c| *c == '"' || *c == '\'')?;
    let inner = head.get(..head.len() - 1)?;
    let open = inner.rfind(q)?;
    inner.get(open + 1..).map(str::to_owned).filter(|n| !n.is_empty())
}

/// A `data:image/svg+xml` URL's bytes: base64 or percent-encoded, after the string escapes a JSON
/// or script value may carry (`\"`, `\/`, `\u003c`).
fn decode_data_url(value: &str) -> Option<Vec<u8>> {
    let value = unescape(value);
    let (meta, payload) = value.split_once(',')?;
    let bytes = if meta.to_ascii_lowercase().ends_with(";base64") {
        let clean: String = payload.chars().filter(|c| !c.is_ascii_whitespace()).collect();
        base64::engine::general_purpose::STANDARD.decode(clean.as_bytes()).ok()?
    } else {
        percent_decode(payload)
    };
    (bytes.len() <= MAX_SVG_BYTES).then_some(bytes)
}

/// A string value's backslash escapes resolved.
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                if let Some(c) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    out.push(c);
                }
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

fn percent_decode(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        let hex = bytes.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
        match (b, hex) {
            (b'%', Some(v)) => {
                out.push(v);
                i += 3;
            }
            _ => {
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

/// A folder name from a set's name: letters, digits, `-` and `_`, lower case and at most
/// [`MAX_NAME_CHARS`]; `imported-<n>` when nothing is left.
fn folder_name(name: &str, n: usize) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if out.chars().count() >= MAX_NAME_CHARS {
            break;
        }
        if c.is_alphanumeric() || c == '-' || c == '_' {
            out.extend(c.to_lowercase());
        } else if (c == ' ' || c == '.') && !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_owned();
    if out.is_empty() { format!("imported-{n}") } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A square piece of `shade`, as an SVG.
    fn piece(shade: &str) -> String {
        format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><path d="M2 2 H8 V8 H2 Z" fill="{shade}"/></svg>"#)
    }

    /// A page with two sets as a script's JSON (one base64, one percent-encoded), and a group of
    /// three pieces that is not a set.
    fn page() -> String {
        let b64 = |s: &str| format!("data:image/svg+xml;base64,{}", base64::engine::general_purpose::STANDARD.encode(s));
        let pct = |s: &str| format!("data:image/svg+xml;charset=utf-8,{}", s.replace('<', "%3C").replace('>', "%3E").replace('"', "\\\""));
        let set = |f: &dyn Fn(&str) -> String, shade: &str| {
            PIECE_STEMS.iter().map(|k| format!("\"{k}\": \"{}\"", f(&piece(shade)))).collect::<Vec<_>>().join(", ")
        };
        let few = PIECE_STEMS.iter().take(3).map(|k| format!("{k}: '{}'", b64(&piece("red")))).collect::<Vec<_>>().join(", ");
        format!(
            "<html><body><script type=\"application/json\" id=\"sets\">{{\"Squares One\": {{{}}}, \"two\": {{{}}}}}</script>\
             <script>var x = {{{few}}};</script></body></html>",
            set(&b64, "black"),
            set(&pct, "gray")
        )
    }

    fn scratch(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pdfcraft-piece-import-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A folder like DiagPDF's `Fonts`: board fonts (one with a patched copy beside it), a
    /// figurine font, a text font and a stray file. The diagram fonts and the figurine font go
    /// into the fonts folder, each family once; a second import changes nothing, and a different
    /// file of the same name waits for Replace.
    #[test]
    fn a_folder_of_chess_fonts_goes_into_the_fonts_folder() {
        use super::super::test_font::font;
        let dir = scratch("fonts");
        let (from, into) = (dir.join("Fonts"), dir.join("app").join("fonts"));
        std::fs::create_dir_all(&from).unwrap();
        let leipzig = font("Chess Leipzig DG", "phbrqkojntwl+ ", true, 0);
        std::fs::write(from.join("LeipzigDG.ttf"), &leipzig).unwrap();
        std::fs::write(from.join("LeipzigDG_patch.ttf"), &leipzig).unwrap();
        std::fs::write(from.join("Chess Cases.ttf"), font("Chess Cases", "pnbrqkomvtwl+ ", true, 0x0002)).unwrap();
        std::fs::write(from.join("HastingsFigurine.TTF"), font("HastingsFigurine", "KQRBN", false, 0)).unwrap();
        std::fs::write(from.join("Inter.ttf"), include_bytes!("../../../../assets/fonts/Inter-Regular.ttf")).unwrap();
        // A layout only presumed would never be offered: not imported either.
        std::fs::write(from.join("LinaresDiagram.ttf"), font("LinaresDiagram", "PNLRQKpnlrqk+ ", false, 0)).unwrap();
        std::fs::write(from.join("notes.txt"), "x").unwrap();
        assert!(holds_fonts(&from) && holds_fonts(&from.join("LeipzigDG.ttf")));
        assert!(!holds_fonts(&from.join("notes.txt")) && !holds_fonts(&dir.join("nothing")));
        let imported = import_fonts(std::slice::from_ref(&from), &into, false).unwrap();
        let got: Vec<(&str, FontUse, PathBuf)> = imported.iter().map(|f| (f.name.as_str(), f.kind, f.path.clone())).collect();
        assert_eq!(
            got,
            [
                ("Chess Cases", FontUse::Pieces, into.join("Chess Cases.ttf")),
                ("HastingsFigurine", FontUse::Text, into.join("HastingsFigurine.TTF")),
                ("Chess Leipzig DG", FontUse::Pieces, into.join("LeipzigDG.ttf")),
            ]
        );
        let mut there: Vec<String> = std::fs::read_dir(&into).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        there.sort();
        assert_eq!(there, ["Chess Cases.ttf", "HastingsFigurine.TTF", "LeipzigDG.ttf"]);
        assert_eq!(std::fs::read(into.join("LeipzigDG.ttf")).unwrap(), leipzig);
        // The same fonts again: already there as they are.
        assert_eq!(import_fonts(std::slice::from_ref(&from), &into, false).unwrap().len(), 3);
        // A different font of the same file name waits for Replace.
        let other = font("Chess Leipzig DG", "phbrqkojntwl+ *", true, 0);
        std::fs::write(from.join("LeipzigDG.ttf"), &other).unwrap();
        let file = from.join("LeipzigDG.ttf");
        assert!(matches!(import_fonts(std::slice::from_ref(&file), &into, false), Err(ImportError::Exists(t)) if t == [into.join("LeipzigDG.ttf")]));
        assert_eq!(std::fs::read(into.join("LeipzigDG.ttf")).unwrap(), leipzig);
        import_fonts(&[file], &into, true).unwrap();
        assert_eq!(std::fs::read(into.join("LeipzigDG.ttf")).unwrap(), other);
        // The fonts folder itself, and a folder with no chess font.
        assert!(matches!(import_fonts(std::slice::from_ref(&into), &into, false), Err(ImportError::Same(_))));
        let plain = dir.join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        std::fs::write(plain.join("Inter.ttf"), include_bytes!("../../../../assets/fonts/Inter-Regular.ttf")).unwrap();
        assert!(matches!(import_fonts(std::slice::from_ref(&plain), &dir.join("empty"), false), Err(ImportError::NoFonts(p)) if p == plain));
        assert!(!dir.join("empty").exists(), "nothing written");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_page_s_embedded_sets_are_found_by_their_piece_keys() {
        let found = sets_in_page(page().as_bytes()).unwrap();
        let names: Vec<&str> = found.sets.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["squares-one", "two"]);
        assert_eq!(found.incomplete, 1);
        for set in &found.sets {
            assert_eq!(set.pieces.len(), 12);
            for svg in set.pieces.values() {
                svg::glyph_from_svg(svg).unwrap();
            }
        }
        assert_eq!(found.sets[1].pieces["bP"], piece("gray").into_bytes());
    }

    #[test]
    fn a_page_is_imported_as_folders_the_pieces_reader_takes() {
        let dir = scratch("page");
        let from = dir.join("course.html");
        std::fs::write(&from, page()).unwrap();
        let into = dir.join("pieces");
        let imported = import(&from, &into, false).unwrap();
        assert_eq!(imported.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["squares-one", "two"]);
        for set in &imported {
            assert!(images::missing_pieces(&set.dir).is_empty());
            images::from_image_dir(&set.dir).unwrap();
            let note = std::fs::read_to_string(set.dir.join(ORIGIN_FILE)).unwrap();
            assert!(note.contains("course.html") && note.contains("personal use"), "{note}");
        }
        // A second import refuses to replace them unless asked.
        assert!(matches!(import(&from, &into, false), Err(ImportError::Exists(ref d)) if d.len() == 2));
        import(&from, &into, true).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_is_copied_and_a_replaced_one_keeps_no_stale_picture() {
        let dir = scratch("folder");
        let from = dir.join("My Wood");
        std::fs::create_dir_all(&from).unwrap();
        for k in PIECE_STEMS {
            std::fs::write(from.join(format!("{k}.svg")), piece("black")).unwrap();
        }
        std::fs::write(from.join("readme.txt"), "not a piece").unwrap();
        let into = dir.join("pieces");
        let target = into.join("my-wood");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("wK.svg"), piece("red")).unwrap();
        std::fs::write(target.join("wQ.png"), b"stale").unwrap();
        assert!(matches!(import(&from, &into, false), Err(ImportError::Exists(_))));
        let imported = import(&from, &into, true).unwrap();
        assert_eq!(imported, [Imported { name: "my-wood".into(), dir: target.clone() }]);
        assert_eq!(std::fs::read_to_string(target.join("wK.svg")).unwrap(), piece("black"));
        assert!(!target.join("wQ.png").exists() && !target.join("readme.txt").exists());
        // Importing a folder onto itself is refused.
        assert!(matches!(import(&target, &into, true), Err(ImportError::Same(_))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nothing_is_written_when_a_piece_cannot_be_read() {
        let dir = scratch("refused");
        // One set's king points at a file on disk: the reader takes nothing from outside, so it
        // draws nothing and the set is refused.
        let outside = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="10" height="10"><image href="C:/Windows/win.ini" width="10" height="10"/></svg>"#;
        let b64 = |s: &str| base64::engine::general_purpose::STANDARD.encode(s);
        let entries: Vec<String> = PIECE_STEMS
            .iter()
            .map(|k| format!("\"{k}\":\"data:image/svg+xml;base64,{}\"", b64(&if *k == "wK" { outside.to_owned() } else { piece("black") })))
            .collect();
        let from = dir.join("bad.html");
        std::fs::write(&from, format!("{{\"bad\":{{{}}}}}", entries.join(","))).unwrap();
        let into = dir.join("pieces");
        let err = import(&from, &into, false).unwrap_err();
        assert!(matches!(err, ImportError::Invalid { ref set, ref reason } if set == "bad" && reason.starts_with("wK")), "{err}");
        assert!(!into.exists());
        // An incomplete folder is refused with what it lacks.
        let half = dir.join("half");
        std::fs::create_dir_all(&half).unwrap();
        std::fs::write(half.join("wK.svg"), piece("black")).unwrap();
        assert!(matches!(import(&half, &into, false), Err(ImportError::Incomplete { .. })));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn any_bytes_are_searched_without_a_panic() {
        let mut seed = 0x2545_f491_u32;
        let alphabet = b"\"':={},\\%data:image/svg+xml;base64,wKbP <>u00";
        for len in [0usize, 1, 7, 64, 4096] {
            for _ in 0..200 {
                let bytes: Vec<u8> = (0..len)
                    .map(|_| {
                        seed ^= seed << 13;
                        seed ^= seed >> 17;
                        seed ^= seed << 5;
                        alphabet.get(seed as usize % alphabet.len()).copied().unwrap_or(b' ')
                    })
                    .collect();
                let _ = sets_in_page(&bytes);
            }
        }
        // Escapes and keys cut short at the end of the text.
        for text in ["\"wK\":\"data:image/svg+xml;base64,\\", "wK='data:image/svg+xml,%3", "\"é\":{\"wK\":\"data:image/svg+xml,\\u00"] {
            let _ = sets_in_page(text.as_bytes());
        }
    }

    #[test]
    fn a_hostile_page_is_searched_in_one_pass() {
        // Values whose quotes never close (each one after it is escaped): every search would run
        // to the end of the page if the quotes were looked for from every value again.
        let page = "x\\'data:image/svg+xml,".repeat(200_000);
        let started = std::time::Instant::now();
        let found = sets_in_page(page.as_bytes()).unwrap();
        assert!(found.sets.is_empty());
        assert!(started.elapsed() < std::time::Duration::from_secs(5), "{:?} for {} bytes", started.elapsed(), page.len());
    }

    #[test]
    fn names_become_safe_folder_names() {
        assert_eq!(folder_name("Squares One", 1), "squares-one");
        assert_eq!(folder_name("../../etc", 2), "etc");
        assert_eq!(folder_name("..", 3), "imported-3");
        assert_eq!(folder_name("C:\\Windows", 4), "cwindows");
        assert_eq!(folder_name(&"x".repeat(100), 5).len(), MAX_NAME_CHARS);
    }
}
