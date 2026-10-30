//! Piece sets from the user's own chess diagram fonts or piece images.
//!
//! PdfCraft ships no piece artwork (blueprint decision D4). Diagrams are drawn with a font the
//! user already has, such as Chess Merida, or with a folder of their own piece images
//! ([`images`]), read from their machine when needed and never copied into the repository or
//! the binary.
//!
//! A diagram font's light-square glyphs draw each piece as an outline with an open interior, and
//! the black piece's detail lines as holes. On a board either would show the square's colour, so
//! every piece is drawn in two layers: paper, the black piece's glyph filled white with its holes
//! closed (the whole silhouette), then the piece's own glyph in black on top. The
//! [`pdfcraft_chess::catalog`] layout of the font's family says which character is which piece.
//!
//! Where the pieces of **diagrams** come from (redrawn on the page, in the editor and in every
//! export), in order ([`find`]):
//! 1. `$PDFCRAFT_CHESS_PIECES`, the path of a chess diagram font or of a folder of piece images,
//!    or the id of a set PdfCraft ships ([`bundled`], `chessnut`…) when no such path exists;
//! 2. a chess diagram font (Chess Merida…) in the `fonts` folder beside the executable;
//! 3. Chess Merida among the fonts installed on this machine;
//! 4. a `pieces` folder of piece images beside the executable.
//!
//! The analysis **board** looks for the `pieces` folder first ([`find_for_board`]): a book's
//! diagrams in its diagram font, the board to play on in the user's own pictures.
//!
//! Without either, [`PieceSet::geometric`] draws plain shapes and the caller says so. The shipped
//! sets are used when named, and the user's choice in the preferences ([`choice`]) comes before
//! all of these but the variable.

use std::path::{Path, PathBuf};

pub mod bundled;
pub mod choice;
pub mod images;
pub mod import;
mod svg;

pub use images::{from_image_dir, is_piece_folder, missing_pieces};

use pdfcraft_chess::catalog::{FontKind, Layout, lookup_family, normalize_font_name};
use pdfcraft_chess::draw::{Layer, Paint, PieceGlyph, PieceSet, Rgb, Segment, Subpath};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::TableProvider;
use skrifa::raw::types::GlyphId;
use skrifa::string::StringId;
use skrifa::{FontRef, MetadataProvider};

/// Largest font file read.
const MAX_FONT_BYTES: usize = 32 << 20;
/// Most outline segments kept for one glyph.
const MAX_SEGMENTS: usize = 20_000;

/// Why a piece set could not be made.
#[derive(Debug, thiserror::Error)]
pub enum PieceError {
    #[error("cannot read {path}: {source}")]
    Read { path: PathBuf, source: std::io::Error },
    #[error("the font file is {0} bytes, more than the {MAX_FONT_BYTES} accepted")]
    TooLarge(usize),
    #[error("not a font PdfCraft can read: {0}")]
    Font(String),
    #[error("{0:?} is not a chess diagram font PdfCraft knows; its layout is not catalogued")]
    UnknownLayout(String),
    #[error("the font has no glyph for {0:?}")]
    MissingGlyph(char),
    #[error("the piece folder {dir} has no {name}")]
    MissingImage { dir: PathBuf, name: String },
    #[error("cannot use the piece image {path}: {reason}")]
    Image { path: PathBuf, reason: String },
    #[error("the shipped piece set {id} cannot read {name}: {reason}")]
    Bundled { id: &'static str, name: &'static str, reason: String },
    #[error("PdfCraft ships no piece set called {0:?}")]
    UnknownSet(String),
}

/// Where a piece set came from, to tell the user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PieceSource {
    /// A font file: its path and family name.
    Font { path: Option<PathBuf>, family: String },
    /// A folder of twelve piece images.
    Images { dir: PathBuf },
    /// A set PdfCraft ships ([`bundled::SETS`]), by its id.
    Bundled { id: &'static str },
    /// No font was found; plain shapes drawn in code.
    Geometric,
}

/// Make a piece set from a chess diagram font's bytes. `name_hint` (the file name, say) is used
/// when the font's own family name is not catalogued.
pub fn from_font_bytes(bytes: &[u8], name_hint: &str) -> Result<(PieceSet, String), PieceError> {
    if bytes.len() > MAX_FONT_BYTES {
        return Err(PieceError::TooLarge(bytes.len()));
    }
    let font = FontRef::new(bytes).map_err(|e| PieceError::Font(e.to_string()))?;
    let family_name = font.localized_strings(StringId::FAMILY_NAME).english_or_first().map(|s| s.to_string()).unwrap_or_default();
    let family = [family_name.as_str(), name_hint]
        .into_iter()
        .filter_map(lookup_family)
        .find(|f| f.kind == FontKind::Diagram && f.key != "unicode")
        .ok_or_else(|| PieceError::UnknownLayout(if family_name.is_empty() { name_hint.to_owned() } else { family_name.clone() }))?;
    let layout: &Layout = family.layout;
    let glyph_char = |set: &str, i: usize| set.chars().nth(i);

    let charmap = font.charmap();
    let outlines = font.outline_glyphs();
    // Symbol-cmap fonts map their characters at U+F000 + code.
    let lookup = |c: char| -> Result<GlyphId, PieceError> {
        charmap.map(c).or_else(|| char::from_u32(0xF000 + u32::from(c)).and_then(|s| charmap.map(s))).ok_or(PieceError::MissingGlyph(c))
    };
    let draw = |c: char| -> Result<Vec<Subpath>, PieceError> {
        let gid = lookup(c)?;
        let glyph = outlines.get(gid).ok_or(PieceError::MissingGlyph(c))?;
        let mut pen = Pen::default();
        glyph.draw(DrawSettings::unhinted(Size::unscaled(), LocationRef::default()), &mut pen).map_err(|e| PieceError::Font(e.to_string()))?;
        Ok(pen.finish())
    };

    // The cell a glyph is drawn in: the empty dark square's box when the font has one, else the
    // em square from the baseline up.
    let upem = f32::from(font.head().map(|h| h.units_per_em()).unwrap_or(1000).max(1));
    let cell = layout
        .empty_dark
        .chars()
        .next()
        .and_then(|c| draw(c).ok())
        .and_then(|p| pdfcraft_chess::draw::bounds(&p))
        .filter(|b| b[2] - b[0] > upem * 0.5 && b[3] - b[1] > upem * 0.5)
        .unwrap_or([0.0, 0.0, upem, upem]);

    let mut glyphs: [PieceGlyph; 12] = Default::default();
    for i in 0..6 {
        let white_c = glyph_char(layout.light_white, i).ok_or(PieceError::UnknownLayout(family.display_name.to_owned()))?;
        let black_c = glyph_char(layout.light_black, i).ok_or(PieceError::UnknownLayout(family.display_name.to_owned()))?;
        let (white, black) = font_pieces(normalize(&draw(white_c)?, cell), normalize(&draw(black_c)?, cell));
        if let Some(g) = glyphs.get_mut(i) {
            *g = white;
        }
        if let Some(g) = glyphs.get_mut(i + 6) {
            *g = black;
        }
    }
    let name = family.display_name.to_owned();
    Ok((PieceSet { glyphs, name: name.clone() }, name))
}

/// The white and the black piece of one kind from a diagram font's two light-square glyphs
/// (`white` the outline drawing, `black` the solid one). Each stands on paper: the black glyph
/// filled white with its holes closed ([`paper`]), so neither piece shows the square through its
/// open interior or its detail lines. On it go the glyph's own lines, in black.
fn font_pieces(white: Vec<Subpath>, black: Vec<Subpath>) -> (PieceGlyph, PieceGlyph) {
    let paper = Layer { subpaths: paper(&black), paint: Paint::Fill { color: Rgb::WHITE, even_odd: false } };
    let ink = |subpaths| Layer { subpaths, paint: Paint::Fill { color: Rgb::BLACK, even_odd: false } };
    (PieceGlyph { layers: vec![paper.clone(), ink(white)] }, PieceGlyph { layers: vec![paper, ink(black)] })
}

/// `paths` all turned the same way round, so that filled with the nonzero rule they cover every
/// contour's inside: a glyph's holes (drawn turning the other way) are closed. What is left is
/// its silhouette.
fn paper(paths: &[Subpath]) -> Vec<Subpath> {
    paths.iter().map(|p| if turn(p) < 0.0 { reversed(p) } else { p.clone() }).collect()
}

/// Twice the signed area a subpath encloses (shoelace, its curves sampled at quarters): which way
/// round it turns.
fn turn(p: &Subpath) -> f32 {
    let mut sum = 0.0_f32;
    let mut last = p.start;
    for seg in &p.segments {
        let (sx, sy) = last;
        // A line's end four times over: the repeats add nothing.
        let points = match *seg {
            Segment::Line(x, y) => [(x, y); 4],
            Segment::Cubic(ax, ay, bx, by, x, y) => [0.25_f32, 0.5, 0.75, 1.0].map(|t| {
                let u = 1.0 - t;
                let (w0, w1, w2, w3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                (w0 * sx + w1 * ax + w2 * bx + w3 * x, w0 * sy + w1 * ay + w2 * by + w3 * y)
            }),
        };
        for to in points {
            sum += last.0 * to.1 - to.0 * last.1;
            last = to;
        }
    }
    sum + last.0 * p.start.1 - p.start.0 * last.1
}

/// The same subpath drawn the other way round.
fn reversed(p: &Subpath) -> Subpath {
    // The point each segment starts from: the start, then each segment's end.
    let mut points = Vec::with_capacity(p.segments.len() + 1);
    points.push(p.start);
    points.extend(p.segments.iter().map(|s| match *s {
        Segment::Line(x, y) | Segment::Cubic(_, _, _, _, x, y) => (x, y),
    }));
    let start = points.last().copied().unwrap_or(p.start);
    let segments = p
        .segments
        .iter()
        .zip(&points)
        .rev()
        .map(|(seg, &(x, y))| match *seg {
            Segment::Line(..) => Segment::Line(x, y),
            Segment::Cubic(ax, ay, bx, by, _, _) => Segment::Cubic(bx, by, ax, ay, x, y),
        })
        .collect();
    Subpath { start, segments, closed: p.closed }
}

/// Read a font file and make a piece set from it.
pub fn from_font_file(path: &Path) -> Result<PieceSet, PieceError> {
    let size = std::fs::metadata(path).map_err(|source| PieceError::Read { path: path.to_path_buf(), source })?.len();
    let size = usize::try_from(size).unwrap_or(usize::MAX);
    if size > MAX_FONT_BYTES {
        return Err(PieceError::TooLarge(size));
    }
    let bytes = std::fs::read(path).map_err(|source| PieceError::Read { path: path.to_path_buf(), source })?;
    let hint = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
    from_font_bytes(&bytes, hint).map(|(set, _)| set)
}

/// The user's piece set: `$PDFCRAFT_CHESS_PIECES` (a font or a folder of images, or a shipped
/// set's id), then a `pieces` folder beside the executable, then an installed Chess Merida, else
/// the geometric set. Configured pieces that fail are reported, not silently replaced.
pub fn find() -> (PieceSet, PieceSource, Option<PieceError>) {
    if let Some(path) = std::env::var_os("PDFCRAFT_CHESS_PIECES").map(PathBuf::from) {
        // A path that exists wins over a shipped set of the same name.
        if !path.exists()
            && let Some(set) = path.to_str().and_then(bundled::by_id)
        {
            return from_bundled(set);
        }
        if path.is_dir() {
            return from_folder(path);
        }
        return match from_font_file(&path) {
            Ok(set) => {
                let family = set.name.clone();
                (set, PieceSource::Font { path: Some(path), family }, None)
            }
            Err(e) => (PieceSet::geometric(), PieceSource::Geometric, Some(e)),
        };
    }
    if let Some(found) = crate::text_font::beside_executable().and_then(|dir| diagram_font_in(&dir)) {
        return found;
    }
    for path in installed_fonts() {
        let stem = path.file_stem().and_then(|s| s.to_str()).map(normalize_font_name).unwrap_or_default();
        if stem.contains("merida")
            && let Ok(set) = from_font_file(&path)
        {
            let family = set.name.clone();
            return (set, PieceSource::Font { path: Some(path), family }, None);
        }
    }
    if let Some(dir) = beside_executable().filter(|d| is_piece_folder(d)) {
        return from_folder(dir);
    }
    (PieceSet::geometric(), PieceSource::Geometric, None)
}

/// The analysis board's pieces: the `pieces` folder beside the executable when there is one
/// (and `$PDFCRAFT_CHESS_PIECES` names nothing), else as diagrams ([`find`]).
pub fn find_for_board() -> (PieceSet, PieceSource, Option<PieceError>) {
    if std::env::var_os("PDFCRAFT_CHESS_PIECES").is_none()
        && let Some(dir) = beside_executable().filter(|d| is_piece_folder(d))
    {
        return from_folder(dir);
    }
    find()
}

/// The first chess diagram font in `dir`, by file name (other fonts there are for text).
fn diagram_font_in(dir: &Path) -> Option<(PieceSet, PieceSource, Option<PieceError>)> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .take(200)
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "ttf" | "otf")))
        .collect();
    files.sort();
    files.into_iter().find_map(|path| {
        let set = from_font_file(&path).ok()?;
        let family = set.name.clone();
        Some((set, PieceSource::Font { path: Some(path), family }, None))
    })
}

/// A shipped set's pieces; one that cannot be read (a broken build) is reported, and plain pieces
/// drawn instead.
pub fn from_bundled(set: &bundled::Bundled) -> (PieceSet, PieceSource, Option<PieceError>) {
    match set.load() {
        Ok(pieces) => (pieces, PieceSource::Bundled { id: set.id }, None),
        Err(e) => (PieceSet::geometric(), PieceSource::Geometric, Some(e)),
    }
}

fn from_folder(dir: PathBuf) -> (PieceSet, PieceSource, Option<PieceError>) {
    match from_image_dir(&dir) {
        Ok(set) => (set, PieceSource::Images { dir }, None),
        Err(e) => (PieceSet::geometric(), PieceSource::Geometric, Some(e)),
    }
}

/// The `pieces` folder beside the executable, where [`choice::candidates`] looks for folders of
/// piece images and [`import::import`] puts the sets it imports.
pub fn pieces_folder() -> Option<PathBuf> {
    beside_executable()
}

/// The `pieces` folder beside the executable, as the models are looked for there.
fn beside_executable() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join("pieces"))
}

/// Font files in the usual font folders of this machine (one level deep, capped).
fn installed_fonts() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    if let Some(w) = env("WINDIR") {
        dirs.push(w.join("Fonts"));
    }
    if let Some(l) = env("LOCALAPPDATA") {
        dirs.push(l.join("Microsoft").join("Windows").join("Fonts"));
    }
    if let Some(h) = env("HOME") {
        dirs.push(h.join(".local/share/fonts"));
        dirs.push(h.join(".fonts"));
        dirs.push(h.join("Library/Fonts"));
    }
    dirs.push(PathBuf::from("/usr/share/fonts"));
    dirs.push(PathBuf::from("/usr/local/share/fonts"));
    dirs.push(PathBuf::from("/Library/Fonts"));
    let mut out = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten().take(10_000) {
            let p = entry.path();
            let is_font = p.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "ttf" | "otf"));
            if is_font {
                out.push(p);
            }
        }
    }
    out
}

/// Map font units in `cell` (y up) to the unit square (y down).
fn normalize(paths: &[Subpath], cell: [f32; 4]) -> Vec<Subpath> {
    let [x0, y0, x1, y1] = cell;
    let (w, h) = ((x1 - x0).max(1e-6), (y1 - y0).max(1e-6));
    let t = |x: f32, y: f32| ((x - x0) / w, (y1 - y) / h);
    paths
        .iter()
        .map(|p| Subpath {
            start: t(p.start.0, p.start.1),
            segments: p
                .segments
                .iter()
                .map(|s| match *s {
                    Segment::Line(x, y) => {
                        let (x, y) = t(x, y);
                        Segment::Line(x, y)
                    }
                    Segment::Cubic(a, b, c, d, x, y) => {
                        let ((a, b), (c, d), (x, y)) = (t(a, b), t(c, d), t(x, y));
                        Segment::Cubic(a, b, c, d, x, y)
                    }
                })
                .collect(),
            closed: p.closed,
        })
        .collect()
}

/// Collects a glyph outline as subpaths; quadratic curves become cubics.
#[derive(Default)]
struct Pen {
    paths: Vec<Subpath>,
    current: Option<Subpath>,
    last: (f32, f32),
    segments: usize,
}

impl Pen {
    fn push(&mut self, seg: Segment, end: (f32, f32)) {
        if self.segments >= MAX_SEGMENTS {
            return;
        }
        self.segments += 1;
        let start = self.last;
        self.current.get_or_insert_with(|| Subpath { start, segments: Vec::new(), closed: false }).segments.push(seg);
        self.last = end;
    }

    fn finish(mut self) -> Vec<Subpath> {
        if let Some(p) = self.current.take() {
            self.paths.push(p);
        }
        self.paths
    }
}

impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        if let Some(p) = self.current.take() {
            self.paths.push(p);
        }
        self.current = Some(Subpath { start: (x, y), segments: Vec::new(), closed: false });
        self.last = (x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.push(Segment::Line(x, y), (x, y));
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        // The cubic with the same curve: control points two thirds of the way to the quad's.
        let (sx, sy) = self.last;
        let c1 = (sx + 2.0 / 3.0 * (cx0 - sx), sy + 2.0 / 3.0 * (cy0 - sy));
        let c2 = (x + 2.0 / 3.0 * (cx0 - x), y + 2.0 / 3.0 * (cy0 - y));
        self.push(Segment::Cubic(c1.0, c1.1, c2.0, c2.1, x, y), (x, y));
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.push(Segment::Cubic(cx0, cy0, cx1, cy1, x, y), (x, y));
    }

    fn close(&mut self) {
        if let Some(mut p) = self.current.take() {
            p.closed = true;
            self.paths.push(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quads_become_equivalent_cubics() {
        let mut pen = Pen::default();
        pen.move_to(0.0, 0.0);
        pen.quad_to(3.0, 3.0, 6.0, 0.0);
        pen.close();
        let paths = pen.finish();
        assert_eq!(paths.len(), 1);
        assert!(paths[0].closed);
        assert_eq!(paths[0].segments, vec![Segment::Cubic(2.0, 2.0, 4.0, 2.0, 6.0, 0.0)]);
    }

    #[test]
    fn the_cell_maps_to_the_unit_square_with_y_down() {
        let p = vec![Subpath { start: (0.0, 1000.0), segments: vec![Segment::Line(1000.0, 0.0)], closed: false }];
        let n = normalize(&p, [0.0, 0.0, 1000.0, 1000.0]);
        assert_eq!(n[0].start, (0.0, 0.0));
        assert_eq!(n[0].segments, vec![Segment::Line(1.0, 1.0)]);
    }

    /// A font's pieces stand on paper: where the white piece's outline is open and where the black
    /// piece's detail lines are holes, both pieces are white, not the square showing through.
    #[test]
    fn font_pieces_stand_on_paper_with_no_hole_left_open() {
        // A square turning one way, from (x0, y0) to (x1, y1); its top edge a curve.
        let square = |x0: f32, y0: f32, x1: f32, y1: f32, backwards: bool| {
            let p = Subpath {
                start: (x0, y0),
                segments: vec![Segment::Cubic(x0 + 0.1, y0, x1 - 0.1, y0, x1, y0), Segment::Line(x1, y1), Segment::Line(x0, y1)],
                closed: true,
            };
            if backwards { reversed(&p) } else { p }
        };
        // As a font draws them: the black piece solid with a hole, the white one a ring.
        let black = vec![square(0.2, 0.1, 0.8, 0.9, false), square(0.4, 0.4, 0.6, 0.6, true)];
        let white = vec![square(0.2, 0.1, 0.8, 0.9, false), square(0.25, 0.15, 0.75, 0.85, true)];
        let (w, b) = font_pieces(white, black);
        // In the black piece's hole, inside the white piece's outline.
        assert_eq!((color_at(&w, (0.5, 0.5)), color_at(&b, (0.5, 0.5))), (Some(Rgb::WHITE), Some(Rgb::WHITE)));
        // Inside the white outline, on the black piece's body.
        assert_eq!((color_at(&w, (0.3, 0.3)), color_at(&b, (0.3, 0.3))), (Some(Rgb::WHITE), Some(Rgb::BLACK)));
        // On the white piece's outline; outside both.
        assert_eq!(color_at(&w, (0.22, 0.5)), Some(Rgb::BLACK));
        assert_eq!((color_at(&w, (0.1, 0.5)), color_at(&b, (0.1, 0.5))), (None, None));
        // Turning a subpath round twice gives it back, and turns its area's sign.
        let s = square(0.2, 0.1, 0.8, 0.9, false);
        assert_eq!(reversed(&reversed(&s)), s);
        assert!(turn(&s) * turn(&reversed(&s)) < 0.0);
    }

    /// The colour the last layer painting `p` gives it, if any.
    fn color_at(g: &PieceGlyph, p: (f32, f32)) -> Option<Rgb> {
        g.layers.iter().rev().find_map(|l| match l.paint {
            Paint::Fill { color, even_odd: false } => (winding(&l.subpaths, p) != 0).then_some(color),
            Paint::Fill { color, even_odd: true } => (winding(&l.subpaths, p) % 2 != 0).then_some(color),
            Paint::Stroke { .. } => None,
        })
    }

    /// The winding number of `paths` around `p`, their curves cut into 32 lines each.
    fn winding(paths: &[Subpath], p: (f32, f32)) -> i32 {
        let mut n = 0;
        for path in paths {
            let mut points = vec![path.start];
            for seg in &path.segments {
                let (sx, sy) = *points.last().unwrap();
                match *seg {
                    Segment::Line(x, y) => points.push((x, y)),
                    Segment::Cubic(ax, ay, bx, by, x, y) => points.extend((1..=32).map(|i| {
                        let t = i as f32 / 32.0;
                        let u = 1.0 - t;
                        let (w0, w1, w2, w3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                        (w0 * sx + w1 * ax + w2 * bx + w3 * x, w0 * sy + w1 * ay + w2 * by + w3 * y)
                    })),
                }
            }
            points.push(path.start);
            for w in points.windows(2) {
                let ((x0, y0), (x1, y1)) = (w[0], w[1]);
                if (y0 <= p.1) != (y1 <= p.1) {
                    let x = x0 + (p.1 - y0) / (y1 - y0) * (x1 - x0);
                    if x > p.0 {
                        n += if y1 > y0 { 1 } else { -1 };
                    }
                }
            }
        }
        n
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(matches!(from_font_bytes(b"not a font", "ChessMerida"), Err(PieceError::Font(_))));
        assert!(matches!(from_font_bytes(&vec![0; MAX_FONT_BYTES + 1], "x"), Err(PieceError::TooLarge(_))));
    }

    #[test]
    fn pens_cap_their_segments() {
        let mut pen = Pen::default();
        pen.move_to(0.0, 0.0);
        for i in 0..(MAX_SEGMENTS + 50) {
            pen.line_to(i as f32, 0.0);
        }
        assert_eq!(pen.finish()[0].segments.len(), MAX_SEGMENTS);
    }

    #[test]
    fn a_fonts_folder_gives_its_diagram_font_and_skips_text_fonts() {
        let dir = std::env::temp_dir().join(format!("pdfcraft-diagram-font-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // A text font is no diagram font.
        std::fs::write(dir.join("Inter.ttf"), include_bytes!("../../../assets/fonts/Inter-Regular.ttf")).unwrap();
        assert!(diagram_font_in(&dir).is_none());
        // With the user's Chess Merida (PDFCRAFT_CHESS_TEST_PIECES), it is found among them.
        if let Some(merida) = std::env::var_os("PDFCRAFT_CHESS_TEST_PIECES") {
            std::fs::copy(&merida, dir.join("ChessMerida.ttf")).unwrap();
            let (set, source, error) = diagram_font_in(&dir).unwrap();
            assert!(error.is_none());
            assert!(matches!(source, PieceSource::Font { ref family, .. } if family == &set.name), "{source:?}");
            assert_eq!(set.glyphs[5].layers.len(), 2);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Builds a piece set from a real chess font when `PDFCRAFT_CHESS_TEST_PIECES` names one
    /// (Chess Merida, say). Skipped otherwise: no font is committed (decision D4).
    #[test]
    fn a_real_diagram_font_gives_twelve_pieces() {
        let Some(path) = std::env::var_os("PDFCRAFT_CHESS_TEST_PIECES") else { return };
        let set = from_font_file(Path::new(&path)).unwrap();
        for (i, g) in set.glyphs.iter().enumerate() {
            assert!(!g.layers.is_empty(), "glyph {i}");
            let b = pdfcraft_chess::draw::bounds(&g.layers[0].subpaths).unwrap();
            // Inside the cell, give or take the font's overshoot.
            assert!(b[0] > -0.1 && b[1] > -0.1 && b[2] < 1.1 && b[3] < 1.1, "glyph {i}: {b:?}");
        }
        // Every piece is two layers: paper, then its lines.
        assert_eq!(set.glyphs[5].layers.len(), 2);
        assert_eq!(set.glyphs[11].layers.len(), 2);
        // No point inside any contour of the black glyph (the silhouette, its holes included)
        // shows the square, on either piece: a grid over each cell finds none.
        for k in 0..6 {
            let contours = &set.glyphs[k + 6].layers.last().unwrap().subpaths;
            for i in [k, k + 6] {
                for p in (1..40).flat_map(|y| (1..40).map(move |x| (x as f32 / 40.0, y as f32 / 40.0))) {
                    if contours.iter().any(|c| winding(std::slice::from_ref(c), p) != 0) {
                        assert!(color_at(&set.glyphs[i], p).is_some(), "glyph {i} shows the square at {p:?}");
                    }
                }
            }
        }
        // For looking at: write a drawn position when asked.
        if let Some(out) = std::env::var_os("PDFCRAFT_CHESS_TEST_SVG_OUT") {
            let placement = pdfcraft_chess::fen::parse_placement("r1bqk2r/pp3ppp/2n2n2/2bpp3/4P3/2NB1N2/PPPP1PPP/R1BQK2R").unwrap();
            let style = pdfcraft_chess::draw::BoardStyle { coordinates: true, ..Default::default() };
            let drawing = pdfcraft_chess::draw::board(&placement, &set, &style);
            std::fs::write(&out, pdfcraft_chess::svg::to_svg(&drawing, "test")).unwrap();
        }
    }
}
