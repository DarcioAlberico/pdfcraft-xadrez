//! Chess ▸ Scan diagrams: find the chess diagrams on pages and read them into positions
//! (blueprint S9).
//!
//! The vision is `pdfcraft-chessvision`'s; this module adapts the document to it and keeps the
//! results. Per page:
//!
//! 1. The inputs are adapted ([`Session::chess_scan_job`]): the images the page draws
//!    (`edit::PageImage` → `ImageBox`), its filled rectangles (`edit::paths` → `FilledBox`), and,
//!    when the job runs, its text in chess diagram fonts (`render::TextGlyph` → `FontGlyph`) and
//!    the page rendered to gray at [`ChessScanSettings::dpi`].
//! 2. Every detector runs, then `merge` joins what they found into one diagram each and `order`
//!    puts the diagrams in reading order, the one ordering the screen, exports, links and tools
//!    share. "Diagram 3" is the third of [`ChessPage::diagrams`] everywhere.
//! 3. `board::read` reads them in that order. A diagram font is read exactly; the others need the
//!    user's piece model and are `Unreadable::NoModel` without it. When a diagram's source offers
//!    more detail than the detection raster (an embedded image scanned finer), the page is
//!    rendered again for reading, up to [`ChessScanSettings::max_read_dpi`]. The resolution a
//!    board was read at is stored with its reading.
//!
//! Reading is slow with a model, so it is split as OCR is ([`crate::ocr`]): the job captures what
//! it needs, [`ChessScanJob::run`] works anywhere (a worker thread on the desktop, inline on the
//! web) and [`Session::store_chess_scan`] keeps the result. Results are cached per document, page
//! and scan parameters, the document's edit generation included: an entry whose parameters
//! differ is dropped and the page scanned again, never adapted.
//!
//! Scanning is read-only: nothing here changes a document. Replacing a diagram is [`replace`]
//! (S10); the recognised document is [`document`] (S14b); adding lichess links is [`links`]
//! (S11).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use pdfcraft_chess::recognize::side::SideSource;
use pdfcraft_chessvision::board::{self, BoardReading, ReadPosition, SquareClassifier};
use pdfcraft_chessvision::coords::{PageGeometry, PageRect, PixelRect, Rotation, ViewRect, view_to_pixels};
use pdfcraft_chessvision::detect::embedded::{self, ImageBox};
use pdfcraft_chessvision::detect::font::{self, FontGlyph};
use pdfcraft_chessvision::detect::vector::{self, FilledBox};
use pdfcraft_chessvision::detect::{DiagramCandidate, contour};
use pdfcraft_chessvision::merge::{self, Merged};
use pdfcraft_chessvision::order::reading_order;
use pdfcraft_chessvision::raster::GrayImage;
use pdfcraft_chessvision::{chess_text, side_text};
use pdfcraft_edit::paths::{PathColor, PathItem, PathSegment, Subpath};
use pdfcraft_render::{PageInfo, PageRenderer, RenderConfig, RenderRequest, RequestKind};

pub use document::ChessDocument;
pub use keys::DocKeys;
pub use links::{LinkPlan, LinkSettings, LinkTarget};
pub use pdfcraft_chessvision::models::{ChessModels, ModelState};
pub use pdfcraft_chessvision::order::ReadingOrder;
pub use replace::{DiagramReplacement, ReplacementPlan};
pub use {pdfcraft_chess, pdfcraft_chessvision};

use crate::{DocId, Session};

/// The lowest and highest resolution a page is rendered at for scanning.
pub const MIN_DPI: f32 = 72.0;
pub const MAX_DPI: f32 = 600.0;
/// Most diagrams kept on one page: as many as the reader reads with a model on one page.
pub const MAX_DIAGRAMS_PER_PAGE: usize = board::reading::consts::MAX_MODEL_BOARDS_PER_PAGE;
/// Most pixels of one scan raster. The renderer allows 64 MP, but the contour detector shrinks
/// anything above 12 MP anyway; this keeps a huge page's gray copy and image crops affordable.
pub const MAX_SCAN_PIXELS: f32 = 40.0e6;
/// Most pixels of image crops handed to the embedded-image detector on one page. Images past
/// this budget go without pixels (they are not trimmed), and the page says so.
pub const MAX_CROP_PIXELS: usize = 24_000_000;
/// Pages kept in the scan cache across a session; the oldest go first.
pub const MAX_CACHED_PAGES: usize = 512;

/// Chess ▸ Scan diagrams settings.
#[derive(Clone, Debug, PartialEq)]
pub struct ChessScanSettings {
    /// Resolution pages are rendered at for the contour detector, in dots per inch. The default
    /// is the prototype's (`streamlit_demo.py`: 220), at which the contour detector was measured
    /// (recall 0.9826 on the field set; `chessvision` README).
    pub dpi: f32,
    /// Highest resolution a page is rendered at again for reading, when a diagram's source has
    /// finer detail than [`Self::dpi`] (an embedded image's own resolution, which the prototype
    /// read its crops at). Never below `dpi`; the page is also capped at [`MAX_SCAN_PIXELS`].
    pub max_read_dpi: f32,
    /// Reading order of the diagrams on a page.
    pub order: ReadingOrder,
    /// Most diagrams kept per page, in reading order; the page says when there were more.
    pub max_diagrams: usize,
    /// Also read diagram fonts whose layout was not checked against the font.
    pub allow_unverified_fonts: bool,
    /// The folder of the piece model (`chess-pieces.onnx` and `.json`). `None` uses the models
    /// installed from files ([`install_models`], the web app) or looks where
    /// [`ChessModels::find`] looks (`$PDFCRAFT_MODELS`, then beside the executable). The app
    /// passes the folder chosen in it, through [`ChessModels::locate`].
    pub models_dir: Option<PathBuf>,
}

impl Default for ChessScanSettings {
    fn default() -> Self {
        ChessScanSettings {
            dpi: 220.0,
            max_read_dpi: MAX_DPI,
            order: ReadingOrder::Column,
            max_diagrams: MAX_DIAGRAMS_PER_PAGE,
            allow_unverified_fonts: false,
            models_dir: None,
        }
    }
}

impl ChessScanSettings {
    /// The settings with every number in range: DPIs finite and within [`MIN_DPI`]..=[`MAX_DPI`]
    /// (the reading one at least the detection one), and 1..=[`MAX_DIAGRAMS_PER_PAGE`] diagrams.
    pub fn sanitized(&self) -> ChessScanSettings {
        let dpi = if self.dpi.is_finite() { self.dpi.clamp(MIN_DPI, MAX_DPI) } else { ChessScanSettings::default().dpi };
        let max_read_dpi = if self.max_read_dpi.is_finite() { self.max_read_dpi.clamp(dpi, MAX_DPI) } else { dpi };
        ChessScanSettings { dpi, max_read_dpi, max_diagrams: self.max_diagrams.clamp(1, MAX_DIAGRAMS_PER_PAGE), ..self.clone() }
    }
}

/// What scanning found on one page.
#[derive(Clone, Debug, PartialEq)]
pub struct ChessPage {
    /// The page, 0-based.
    pub page: usize,
    /// The page's geometry (crop box, `/Rotate`, `/UserUnit`): what maps the diagrams' user-space
    /// boxes to the displayed page.
    pub geometry: Option<PageGeometry>,
    /// The resolution the page was rendered at for detection (after the renderer's size caps).
    pub dpi: f32,
    /// The diagrams, in reading order, each with every detector that saw it.
    pub diagrams: Vec<Merged>,
    /// One reading per diagram, in the same order (`BoardReading::index` is the diagram's
    /// index). `None` when the page was only scanned for diagrams, not read.
    pub readings: Option<Vec<BoardReading>>,
    /// The page had more diagrams than [`ChessScanSettings::max_diagrams`]; the first ones in
    /// reading order were kept.
    pub truncated: bool,
    /// Why the page could not be scanned, when it could not. Diagrams found without the page
    /// raster (fonts, vectors, images) are still listed, but nothing is read.
    pub error: Option<String>,
    /// What was skipped or approximated on the way (an unreadable content stream, a limit hit,
    /// images left untrimmed), so a partial result is never silent.
    pub notes: Vec<String>,
}

/// Which piece model a reading used, for the cache: a reading made with one model is not the
/// reading another would make.
#[derive(Clone, Debug, PartialEq)]
enum ModelKey {
    /// No piece model: only diagram fonts were read.
    Missing,
    /// The model loaded from this folder.
    Folder(PathBuf),
    /// A classifier the caller supplied ([`ChessScanJob::run_with`]); not cached.
    Supplied,
}

impl ModelKey {
    /// The piece model `models` reads with.
    fn of(models: &ChessModels) -> ModelKey {
        match (models.pieces.loaded(), &models.folder) {
            (Some(_), Some(folder)) => ModelKey::Folder(folder.clone()),
            (Some(_), None) => ModelKey::Supplied,
            (None, _) => ModelKey::Missing,
        }
    }
}

/// Everything a page's scan result depends on.
#[derive(Clone, Debug, PartialEq)]
struct ScanParams {
    /// The document's edit generation and size: any edit, undo or redo makes a new result.
    generation: u64,
    bytes: usize,
    settings: ChessScanSettings,
    /// `None`: detection only.
    model: Option<ModelKey>,
}

impl ScanParams {
    /// Whether a result made with `self` answers a request made with `wanted`: the same document
    /// state and detection settings, and, when `wanted` reads, the same model and reading
    /// resolution. (The model folder setting itself is not compared: the model it found is.)
    fn answers(&self, wanted: &ScanParams) -> bool {
        let (a, b) = (&self.settings, &wanted.settings);
        let same_detection = self.generation == wanted.generation
            && self.bytes == wanted.bytes
            && a.dpi == b.dpi
            && a.order == b.order
            && a.max_diagrams == b.max_diagrams
            && a.allow_unverified_fonts == b.allow_unverified_fonts;
        same_detection
            && match &wanted.model {
                None => true,
                Some(m) => self.model.as_ref() == Some(m) && *m != ModelKey::Supplied && a.max_read_dpi == b.max_read_dpi,
            }
    }
}

struct CacheEntry {
    doc: DocId,
    page: usize,
    params: ScanParams,
    result: Arc<ChessPage>,
}

/// Scan results kept in the session, keyed by document, page and scan parameters.
#[derive(Default)]
pub(crate) struct ChessCache {
    entries: Vec<CacheEntry>,
}

impl ChessCache {
    /// The result for `page` of `doc` that `wanted` would produce. An entry for that page made
    /// with other parameters is dropped.
    fn get(&mut self, doc: DocId, page: usize, wanted: &ScanParams) -> Option<Arc<ChessPage>> {
        let at = self.entries.iter().position(|e| e.doc == doc && e.page == page)?;
        let entry = self.entries.get(at)?;
        if entry.params.answers(wanted) {
            return Some(entry.result.clone());
        }
        self.entries.remove(at);
        None
    }

    fn put(&mut self, doc: DocId, page: usize, params: ScanParams, result: Arc<ChessPage>) {
        if params.model == Some(ModelKey::Supplied) {
            return;
        }
        self.entries.retain(|e| !(e.doc == doc && e.page == page));
        if self.entries.len() >= MAX_CACHED_PAGES {
            self.entries.remove(0);
        }
        self.entries.push(CacheEntry { doc, page, params, result });
    }

    /// Forget every result for `doc` (it was closed).
    pub(crate) fn forget(&mut self, doc: DocId) {
        self.entries.retain(|e| e.doc != doc);
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Model folders kept loaded at most; the oldest goes first.
const MAX_MODEL_FOLDERS: usize = 8;

/// The models loaded so far, by folder (`None`: found as [`ChessModels::find`] finds them), and
/// the ones installed from files for the session (the web app). Generic so its rules can be
/// tested without real models.
struct ModelCache<T> {
    installed: Option<Arc<T>>,
    loaded: Vec<(Option<PathBuf>, Arc<T>)>,
}

impl<T> ModelCache<T> {
    const fn new() -> Self {
        ModelCache { installed: None, loaded: Vec::new() }
    }

    /// The models for `dir`: the installed ones when `dir` is `None` and some are, else the ones
    /// loaded before, else `load`ed now and kept when `keep` says they are worth keeping.
    fn get(&mut self, dir: Option<&Path>, load: impl FnOnce(Option<&Path>) -> T, keep: impl Fn(&T) -> bool) -> Arc<T> {
        if dir.is_none()
            && let Some(m) = &self.installed
        {
            return m.clone();
        }
        if let Some((_, m)) = self.loaded.iter().find(|(k, _)| k.as_deref() == dir) {
            return m.clone();
        }
        let found = Arc::new(load(dir));
        if keep(&found) {
            if self.loaded.len() >= MAX_MODEL_FOLDERS {
                self.loaded.remove(0);
            }
            self.loaded.push((dir.map(Path::to_path_buf), found.clone()));
        }
        found
    }

    /// [`Self::get`], reading `dir` again (its files may have changed).
    fn reload(&mut self, dir: Option<&Path>, load: impl FnOnce(Option<&Path>) -> T, keep: impl Fn(&T) -> bool) -> Arc<T> {
        self.loaded.retain(|(k, _)| k.as_deref() != dir);
        self.get(dir, load, keep)
    }

    /// Use `models` wherever no folder is given, or stop (`None`).
    fn install(&mut self, models: Option<T>) -> Option<Arc<T>> {
        self.installed = models.map(Arc::new);
        self.installed.clone()
    }
}

/// The session's models. A job holds its own `Arc`, so models reloaded or installed while it
/// runs replace these for the next job and never change under it.
static MODELS: Mutex<ModelCache<ChessModels>> = Mutex::new(ModelCache::new());

fn model_cache() -> std::sync::MutexGuard<'static, ModelCache<ChessModels>> {
    // The cache is a list of shared models: a panic elsewhere while it was held leaves it whole.
    MODELS.lock().unwrap_or_else(|e| e.into_inner())
}

fn load_models(dir: Option<&Path>) -> ChessModels {
    match dir {
        Some(d) => ChessModels::load_from(d),
        None => ChessModels::find(),
    }
}

/// Only a loaded piece model is kept: a missing or broken one is looked for again next time, so
/// a model installed while the app runs is picked up.
fn worth_keeping(models: &ChessModels) -> bool {
    models.pieces.loaded().is_some()
}

/// The user's chess models for `dir` (or found as [`ChessModels::find`] finds them), loaded
/// once and shared. Without `dir`, models installed from files ([`install_models`]) come first.
/// The app passes the folder [`ChessModels::locate`] finds from the one chosen in it.
pub fn models(dir: Option<&Path>) -> Arc<ChessModels> {
    model_cache().get(dir, load_models, worth_keeping)
}

/// [`models`], read from `dir` again: after the user chose (or chose again) the models folder,
/// whose files may have changed. Readings already running keep the models they started with.
pub fn reload_models(dir: Option<&Path>) -> Arc<ChessModels> {
    model_cache().reload(dir, load_models, worth_keeping)
}

/// Use `models` (built from files' bytes: [`ChessModels::from_files`]) wherever no folder is
/// given, for the rest of the session; `None` stops. The web app's way to have models, where no
/// folder can be looked in. Returns the models now installed.
pub fn install_models(models: Option<ChessModels>) -> Option<Arc<ChessModels>> {
    model_cache().install(models)
}

/// The state of a model, as a word and a reason.
pub fn model_state<T>(state: &ModelState<T>) -> (&'static str, Option<&str>) {
    match state {
        ModelState::Loaded(_) => ("loaded", None),
        ModelState::Missing => ("missing", None),
        ModelState::Invalid(why) => ("invalid", Some(why.as_str())),
    }
}

/// The lichess analysis URL of a reading, only when its position is legal with the side to move
/// it was given and the decoder did not have to change a square to make it so. An illegal
/// reading gets no link, nor does a repaired one until a person confirms it: either would show a
/// position the book may not have (an empty board "repaired" with two kings, say).
pub fn lichess_url(position: &ReadPosition) -> Option<String> {
    if !position.legality.allows(position.side.color) || position.repaired() {
        return None;
    }
    let parsed = pdfcraft_chess::fen::parse(&position.fen).ok()?;
    let from = if position.orientation.is_some_and(|v| v.black_point_of_view) {
        pdfcraft_chess::placement::Color::Black
    } else {
        pdfcraft_chess::placement::Color::White
    };
    Some(pdfcraft_chess::lichess::analysis_url(&parsed, from))
}

/// One page's inputs taken from the document's object graph when the job is made.
struct PageInputs {
    page: usize,
    info: Option<PageInfo>,
    /// The images the page draws: box in user space and pixel size.
    images: Vec<(PageRect, u32, u32)>,
    boxes: Vec<FilledBox>,
    notes: Vec<String>,
}

/// Everything scanning needs from a document, detached from the session.
pub struct ChessScanJob {
    doc: DocId,
    bytes: Arc<Vec<u8>>,
    password: Option<String>,
    inputs: Vec<PageInputs>,
    params: ScanParams,
    /// Read the diagrams, or only find them.
    read: bool,
    pub settings: ChessScanSettings,
}

/// What [`ChessScanJob::run`] found.
pub struct ChessScan {
    pub doc: DocId,
    params: ScanParams,
    pub pages: Vec<Arc<ChessPage>>,
}

impl ChessScanJob {
    /// The pages (0-based) this job scans.
    pub fn pages(&self) -> Vec<usize> {
        self.inputs.iter().map(|i| i.page).collect()
    }

    /// Scan the pages, reading with the user's piece model when `models` has one.
    /// `progress(done, total)` is called before each page; returning `false` stops (the pages
    /// scanned so far are returned).
    pub fn run(self, models: &ChessModels, progress: impl FnMut(usize, usize) -> bool) -> ChessScan {
        let model = models.pieces.loaded().map(|m| m as &dyn SquareClassifier);
        self.run_inner(model, ModelKey::of(models), progress)
    }

    /// [`Self::run`] with a classifier the caller supplies: a model loaded from bytes (the web
    /// has no model folder), or a stand-in in tests. Its results are not cached.
    pub fn run_with(self, model: Option<&dyn SquareClassifier>, progress: impl FnMut(usize, usize) -> bool) -> ChessScan {
        let key = if model.is_some() { ModelKey::Supplied } else { ModelKey::Missing };
        self.run_inner(model, key, progress)
    }

    fn run_inner(mut self, model: Option<&dyn SquareClassifier>, key: ModelKey, mut progress: impl FnMut(usize, usize) -> bool) -> ChessScan {
        if self.read {
            self.params.model = Some(key);
        }
        let config = RenderConfig { password: self.password.as_deref().map(Arc::from), ..Default::default() };
        let mut renderer = PageRenderer::new(self.bytes.clone(), config);
        let settings = self.settings.sanitized();
        let total = self.inputs.len();
        let mut pages = Vec::with_capacity(total);
        for (done, input) in self.inputs.iter().enumerate() {
            if !progress(done, total) {
                break;
            }
            // A last-resort guard (AGENTS.md §4): the detectors and the reader return values for
            // every input, but this is untrusted-document code, and one bad page must not take
            // the scan down.
            let scanned = catch_unwind(AssertUnwindSafe(|| scan_page(&mut renderer, input, &settings, self.read, model)));
            let page = scanned.unwrap_or_else(|_| {
                // The renderer's state is unknown after a panic: start a fresh one.
                renderer =
                    PageRenderer::new(self.bytes.clone(), RenderConfig { password: self.password.as_deref().map(Arc::from), ..Default::default() });
                failed_page(input, "scanning the page failed unexpectedly")
            });
            pages.push(Arc::new(page));
        }
        progress(total, total);
        ChessScan { doc: self.doc, params: self.params, pages }
    }
}

fn failed_page(input: &PageInputs, why: &str) -> ChessPage {
    ChessPage {
        page: input.page,
        geometry: None,
        dpi: 0.0,
        diagrams: Vec::new(),
        readings: None,
        truncated: false,
        error: Some(why.into()),
        notes: input.notes.clone(),
    }
}

/// The page's geometry from what the renderer reports: crop box, rotation, and `/UserUnit` as the
/// ratio of the displayed size to the crop box.
pub fn geometry(info: &PageInfo) -> Option<PageGeometry> {
    let [x0, y0, x1, y1] = info.crop;
    let crop = PageRect::new(x0, y0, x1, y1);
    let rotation = Rotation::from_degrees(i64::from(info.rotation));
    let geometry = PageGeometry::new(crop, rotation)?;
    let across = if rotation.swaps_axes() { info.height } else { info.width };
    let unit = across / crop.width();
    // A unit that is not a finite positive number is the renderer's rounding on a degenerate box:
    // keep 1, the default.
    Some(geometry.with_user_unit(unit).unwrap_or(geometry))
}

/// A whole page rendered to gray at about `dpi`, and the resolution it really has (the renderer
/// caps a page's size).
fn render_gray(renderer: &mut PageRenderer, page: usize, info: &PageInfo, dpi: f32) -> Result<(GrayImage, f32), String> {
    let (w, h) = (info.width.max(1.0), info.height.max(1.0));
    let mut scale = dpi / 72.0;
    let pixels = w * h * scale * scale;
    if !(pixels.is_finite() && pixels <= MAX_SCAN_PIXELS) {
        scale = (MAX_SCAN_PIXELS / (w * h)).sqrt();
    }
    if !(scale.is_finite() && scale > 0.0) {
        return Err("the page has no size to render".into());
    }
    let shot = renderer.render(RenderRequest { page, kind: RequestKind::Pixels, scale, ..Default::default() });
    if let Some(e) = shot.error {
        return Err(e);
    }
    let gray = to_gray(&shot.rgba, shot.width, shot.height).ok_or("the page raster is malformed")?;
    let real = 72.0 * shot.width as f32 / w;
    if !(real.is_finite() && real > 0.0) {
        return Err("the page raster has no resolution".into());
    }
    Ok((gray, real))
}

/// Premultiplied RGBA over white, to 8-bit gray (Rec. 601 luma, as the prototype's
/// `cv2.cvtColor(..., COLOR_RGB2GRAY)`).
fn to_gray(rgba: &[u8], width: u32, height: u32) -> Option<GrayImage> {
    let n = usize::try_from(u64::from(width) * u64::from(height)).ok()?;
    let mut data = Vec::with_capacity(n.min(rgba.len() / 4));
    for &[r, g, b, a] in rgba.as_chunks::<4>().0.iter().take(n) {
        let white = 255 - u32::from(a);
        let over = |c: u8| (u32::from(c) + white).min(255);
        data.push(((299 * over(r) + 587 * over(g) + 114 * over(b) + 500) / 1000).min(255) as u8);
    }
    GrayImage::new(width, height, data).ok()
}

/// The `rect` pixels of `image`, clamped to it. `None` when nothing is left.
fn crop(image: &GrayImage, rect: PixelRect) -> Option<GrayImage> {
    let x0 = rect.x0.min(image.width());
    let x1 = rect.x1.min(image.width());
    let y0 = rect.y0.min(image.height());
    let y1 = rect.y1.min(image.height());
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let (from, to) = (usize::try_from(x0).ok()?, usize::try_from(x1).ok()?);
    let mut data = Vec::with_capacity(usize::try_from(u64::from(x1 - x0) * u64::from(y1 - y0)).ok()?);
    for y in y0..y1 {
        data.extend_from_slice(image.row(y)?.get(from..to)?);
    }
    GrayImage::new(x1 - x0, y1 - y0, data).ok()
}

/// The page's images as the embedded detector takes them, each with its region of the page
/// raster in gray (within [`MAX_CROP_PIXELS`]), so a framed or captioned picture can be trimmed.
fn image_boxes(
    images: &[(PageRect, u32, u32)],
    geometry: &PageGeometry,
    raster: Option<(&GrayImage, f32)>,
    notes: &mut Vec<String>,
) -> Vec<ImageBox> {
    let crop_box = geometry.crop_box();
    let page_area = crop_box.width() * crop_box.height();
    let mut budget = MAX_CROP_PIXELS;
    let mut uncropped = 0_usize;
    let out: Vec<ImageBox> = images
        .iter()
        .take(embedded::consts::MAX_IMAGES)
        .map(|&(bbox_user, pixel_width, pixel_height)| {
            // A background scan covering most of the page is dropped by the detector anyway:
            // cutting its pixels would only spend the budget.
            let background = bbox_user.width() * bbox_user.height() > page_area * embedded::consts::MAX_PAGE_COVERAGE;
            let gray = match raster {
                Some((page, dpi)) if !background => {
                    let pixels = geometry.user_to_view(bbox_user).and_then(|v| view_to_pixels(v, dpi));
                    let size = pixels.map_or(0, |p| usize::try_from(u64::from(p.width()) * u64::from(p.height())).unwrap_or(usize::MAX));
                    if size <= budget {
                        budget -= size;
                        pixels.and_then(|p| crop(page, p))
                    } else {
                        uncropped += 1;
                        None
                    }
                }
                _ => None,
            };
            ImageBox { bbox_user, pixel_width, pixel_height, gray }
        })
        .collect();
    if images.len() > embedded::consts::MAX_IMAGES {
        notes.push(format!("only the first {} of {} images were looked at", embedded::consts::MAX_IMAGES, images.len()));
    }
    if uncropped > 0 {
        notes.push(format!("{uncropped} image(s) were not trimmed: the page's image pixels exceed the scan budget"));
    }
    out
}

/// A fill colour as a gray level, 0 black to 1 white, when it is a device colour.
fn gray_of(color: &PathColor) -> Option<f32> {
    let unit = |v: f64| v.clamp(0.0, 1.0);
    let g = match color {
        PathColor::Gray(g) => unit(*g),
        PathColor::Rgb([r, g, b]) => 0.299 * unit(*r) + 0.587 * unit(*g) + 0.114 * unit(*b),
        PathColor::Cmyk([c, m, y, k]) => {
            let ink = |v: f64| (1.0 - unit(v)) * (1.0 - unit(*k));
            0.299 * ink(*c) + 0.587 * ink(*m) + 0.114 * ink(*y)
        }
        PathColor::Other(_) => return None,
    };
    let g = g as f32;
    g.is_finite().then_some(g)
}

/// A subpath's box when it is an axis-aligned rectangle (`re`, or four straight sides): every
/// point a corner of its box, all four corners present.
fn rectangle(path: &Subpath) -> Option<PageRect> {
    if !(3..=4).contains(&path.segments.len()) {
        return None;
    }
    let mut points = vec![path.start];
    for s in &path.segments {
        match s {
            PathSegment::Line(p) => points.push(*p),
            PathSegment::Cubic(..) => return None,
        }
    }
    if !points.iter().flatten().all(|v| v.is_finite()) {
        return None;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for [x, y] in &points {
        (x0, y0, x1, y1) = (x0.min(*x), y0.min(*y), x1.max(*x), y1.max(*y));
    }
    let tolerance = 1e-3 * (x1 - x0).abs().max((y1 - y0).abs()).max(1e-6);
    let near = |a: f64, b: f64| (a - b).abs() <= tolerance;
    let mut corners = [false; 4];
    for [x, y] in &points {
        let i = match ((near(*x, x0), near(*x, x1)), (near(*y, y0), near(*y, y1))) {
            ((true, _), (true, _)) => 0,
            ((_, true), (true, _)) => 1,
            ((_, true), (_, true)) => 2,
            ((true, _), (_, true)) => 3,
            _ => return None,
        };
        if let Some(c) = corners.get_mut(i) {
            *c = true;
        }
    }
    let rect = PageRect::new(x0 as f32, y0 as f32, x1 as f32, y1 as f32);
    (corners.iter().all(|c| *c)
        && rect.width() > 0.0
        && rect.height() > 0.0
        && rect.x0.is_finite()
        && rect.x1.is_finite()
        && rect.y0.is_finite()
        && rect.y1.is_finite())
    .then_some(rect)
}

/// The filled rectangles among a page's paths, as the vector detector takes them.
fn filled_boxes(items: &[PathItem]) -> Vec<FilledBox> {
    let mut out = Vec::new();
    for item in items {
        let Some(fill) = &item.fill else { continue };
        let gray = gray_of(fill);
        for path in &item.subpaths {
            if out.len() >= vector::MAX_BOXES {
                return out;
            }
            if let Some(bbox_user) = rectangle(path) {
                out.push(FilledBox { bbox_user, gray });
            }
        }
    }
    out
}

/// The page's glyphs in chess diagram fonts, as the font detector takes them. Other text is left
/// out here, so a page of prose costs one font lookup per font, not per glyph.
fn font_glyphs(text: &pdfcraft_render::PageText) -> Vec<FontGlyph> {
    use pdfcraft_chess::catalog::{FontKind, lookup_family};
    let mut known: Vec<(Arc<str>, bool)> = Vec::new();
    let mut out = Vec::new();
    for g in &text.glyphs {
        if out.len() >= font::MAX_GLYPHS {
            break;
        }
        let Some(name) = &g.font else { continue };
        let diagram = match known.iter().find(|(n, _)| n == name) {
            Some((_, d)) => *d,
            None => {
                let d = lookup_family(name).is_some_and(|f| f.kind == FontKind::Diagram);
                known.push((name.clone(), d));
                d
            }
        };
        if !diagram {
            continue;
        }
        let [x0, y0, x1, y1] = g.rect;
        out.push(FontGlyph {
            font: name.to_string(),
            // The catalogues are keyed by one-byte codes.
            code: g.code.filter(|c| *c <= 0xFF),
            text: g.text.clone(),
            rect: ViewRect::new(x0, y0, x1, y1),
        });
    }
    out
}

/// Detect, merge, order and (when `read`) read one page.
fn scan_page(
    renderer: &mut PageRenderer,
    input: &PageInputs,
    settings: &ChessScanSettings,
    read: bool,
    model: Option<&dyn SquareClassifier>,
) -> ChessPage {
    let mut notes = input.notes.clone();
    let Some(info) = &input.info else { return failed_page(input, "the page could not be read") };
    let Some(geometry) = geometry(info) else { return failed_page(input, "the page has no visible area") };

    let raster = render_gray(renderer, input.page, info, settings.dpi);
    let text = renderer.render(RenderRequest { page: input.page, kind: RequestKind::Text, scale: 1.0, ..Default::default() });
    if let Some(e) = &text.error {
        notes.push(format!("the page's text could not be read: {e}"));
    }
    let glyphs = text.text.as_deref().map(font_glyphs).unwrap_or_default();

    let options = font::Options { allow_unverified: settings.allow_unverified_fonts };
    let mut found: Vec<DiagramCandidate> = font::candidates(&glyphs, &geometry, options);
    // On a turned page (`/Rotate`), a diagram typeset in user space shows turned, its ranks
    // running down the display. Look for it as typeset too: the glyphs in the unturned page's
    // frame. `merge` joins a diagram found both ways.
    if geometry.rotation() != Rotation::R0
        && let Some(upright) = PageGeometry::new(geometry.crop_box(), Rotation::R0).and_then(|g| g.with_user_unit(geometry.user_unit()))
    {
        let typeset: Vec<FontGlyph> = glyphs
            .iter()
            .filter_map(|g| {
                let rect = upright.user_to_view(geometry.view_to_user(g.rect)?)?;
                Some(FontGlyph { rect, ..g.clone() })
            })
            .collect();
        found.extend(font::candidates(&typeset, &upright, options));
    }
    found.extend(vector::candidates(&input.boxes));
    let page_raster = raster.as_ref().ok().map(|(g, d)| (g, *d));
    found.extend(embedded::candidates_from_images(&image_boxes(&input.images, &geometry, page_raster, &mut notes), &geometry));
    if let Some((gray, dpi)) = page_raster {
        found.extend(contour::candidates(gray, &geometry, dpi));
    }

    let merged = merge::merge(found);
    let flat: Vec<DiagramCandidate> = merged.iter().map(|m| m.candidate.clone()).collect();
    let mut diagrams: Vec<Merged> = reading_order(&flat, &geometry, settings.order).into_iter().filter_map(|i| merged.get(i).cloned()).collect();
    let truncated = diagrams.len() > settings.max_diagrams;
    diagrams.truncate(settings.max_diagrams);

    let (dpi, error, mut readings) = match raster {
        Err(e) => (0.0, Some(e), None),
        Ok((gray, dpi)) => {
            let readings = read.then(|| {
                let candidates: Vec<DiagramCandidate> = diagrams.iter().map(|m| m.candidate.clone()).collect();
                read_page(renderer, input.page, info, &geometry, (&gray, dpi), &candidates, settings, model, &mut notes)
            });
            (dpi, None, readings)
        }
    };
    if let (Some(readings), Some(text)) = (readings.as_mut(), text.text.as_deref()) {
        sides_from_move_numbers(text, &geometry, &diagrams, readings);
    }
    ChessPage { page: input.page, geometry: Some(geometry), dpi, diagrams, readings, truncated, error, notes }
}

/// The side to move (and move number) from the bold move number under each diagram, in a
/// born-digital page's text (`side_text`). The position still overrules it.
fn sides_from_move_numbers(text: &pdfcraft_render::PageText, geometry: &PageGeometry, diagrams: &[Merged], readings: &mut [BoardReading]) {
    if !readings.iter().any(|r| r.position().is_some()) {
        return;
    }
    // A box that does not map to the page matches no text.
    let nowhere = ViewRect { x0: f32::NAN, y0: f32::NAN, x1: f32::NAN, y1: f32::NAN };
    let boxes: Vec<ViewRect> = diagrams.iter().map(|d| geometry.user_to_view(d.candidate.bbox_user).unwrap_or(nowhere)).collect();
    let chess = chess_text::from_page_text(text, None);
    let hints = side_text::hints(text, &chess, &boxes);
    for reading in readings {
        if let Some(hint) = hints.get(reading.index).copied().flatten()
            && let Ok(position) = &mut reading.outcome
        {
            position.declare_side(hint.color, SideSource::MoveNumber, u32::from(hint.number));
        }
    }
}

/// Read the candidates, on the detection raster, or on a finer one when a candidate's source has
/// more detail and there is a model to use it.
#[allow(clippy::too_many_arguments)]
fn read_page(
    renderer: &mut PageRenderer,
    page: usize,
    info: &PageInfo,
    geometry: &PageGeometry,
    (gray, dpi): (&GrayImage, f32),
    candidates: &[DiagramCandidate],
    settings: &ChessScanSettings,
    model: Option<&dyn SquareClassifier>,
    notes: &mut Vec<String>,
) -> Vec<BoardReading> {
    // `board::read` cuts a box-only candidate (an embedded image) from the page raster at the
    // raster's resolution, while the prototype read such a crop at the image's own resolution.
    // So the page is read at least at the finest `dpi_hint` of its candidates (capped by
    // `max_read_dpi` and `MAX_SCAN_PIXELS`), and a cap that keeps it lower is noted, never
    // silent. Detection stays at the cheaper `settings.dpi`.
    let finest = candidates.iter().filter(|c| c.placement.is_none()).map(|c| c.dpi_hint).filter(|d| d.is_finite()).fold(0.0_f32, f32::max);
    let wanted = finest.min(settings.max_read_dpi);
    if finest > settings.max_read_dpi * 1.05 && model.is_some() {
        notes.push(format!("a diagram offers {finest:.0} dpi; read at most at {:.0} dpi (max_read_dpi)", settings.max_read_dpi));
    }
    // Within 5 % is the same raster: rendering again would cost a page and add nothing.
    if model.is_some() && wanted > dpi * 1.05 {
        match render_gray(renderer, page, info, wanted) {
            Ok((finer, finer_dpi)) => {
                if finer_dpi < wanted * 0.95 {
                    notes.push(format!("the page is too large to render at {wanted:.0} dpi; diagrams were read at {finer_dpi:.0} dpi"));
                }
                return board::read(&finer, geometry, finer_dpi, candidates, model);
            }
            Err(e) => notes.push(format!("the page could not be rendered at {wanted:.0} dpi for reading ({e}); read at {dpi:.0} dpi")),
        }
    }
    board::read(gray, geometry, dpi, candidates, model)
}

impl Session {
    /// Capture what scanning `pages` (0-based; empty = all) of document `id` needs: the pages'
    /// images and filled rectangles are taken from its object graph now. `read`: read the
    /// diagrams too, not only find them. `None` when there is no such document.
    pub fn chess_scan_job(&self, id: DocId, pages: &[usize], settings: &ChessScanSettings, read: bool) -> Option<ChessScanJob> {
        let doc = self.get(id)?;
        let all = doc.info.pages.len();
        let pages: Vec<usize> = if pages.is_empty() { (0..all).collect() } else { pages.iter().copied().filter(|p| *p < all).collect() };
        let settings = settings.sanitized();
        let inputs = pages
            .iter()
            .map(|&page| {
                let mut notes = Vec::new();
                let images = doc
                    .page_images(page)
                    .iter()
                    .filter(|i| !i.is_form && i.width > 0 && i.height > 0)
                    .filter_map(|i| {
                        let [x0, y0, x1, y1] = i.rect.map(|v| v as f32);
                        let rect = PageRect::new(x0, y0, x1, y1);
                        [rect.x0, rect.y0, rect.x1, rect.y1].iter().all(|v| v.is_finite()).then_some((rect, i.width, i.height))
                    })
                    .collect();
                let boxes = match doc.editor.as_ref().map(|e| pdfcraft_edit::paths::page_paths(&e.cos, page)) {
                    Some(Ok(inventory)) => {
                        if inventory.truncated {
                            notes.push("the page draws more paths than are looked at; the rest were skipped".into());
                        }
                        if inventory.unreadable > 0 {
                            notes.push(format!("{} content stream(s) could not be decoded and were skipped", inventory.unreadable));
                        }
                        filled_boxes(&inventory.items)
                    }
                    Some(Err(e)) => {
                        notes.push(format!("the page's paths could not be read: {e}"));
                        Vec::new()
                    }
                    None => {
                        notes.push("the document can't be read for editing, so its images and paths were not looked at".into());
                        Vec::new()
                    }
                };
                PageInputs { page, info: doc.info.pages.get(page).cloned(), images, boxes, notes }
            })
            .collect();
        let params = ScanParams { generation: doc.generation, bytes: doc.bytes.len(), settings: settings.clone(), model: None };
        Some(ChessScanJob { doc: id, bytes: doc.bytes.clone(), password: doc.password.clone(), inputs, params, read, settings })
    }

    /// Keep what [`ChessScanJob::run`] found, unless the document changed since the job was
    /// made (then the result describes a state that is gone, and is dropped). Returns whether it
    /// was kept.
    pub fn store_chess_scan(&mut self, scan: &ChessScan) -> bool {
        let Some(doc) = self.get(scan.doc) else { return false };
        if doc.generation != scan.params.generation || doc.bytes.len() != scan.params.bytes {
            return false;
        }
        if scan.params.model == Some(ModelKey::Supplied) {
            return false;
        }
        for page in &scan.pages {
            self.chess.put(scan.doc, page.page, scan.params.clone(), page.clone());
        }
        true
    }

    /// Scan `pages` (0-based; empty = all) of document `id`, from the cache where it still
    /// answers and scanning the rest now (the CLI and agents use this). `read`: read the
    /// diagrams with the user's models, not only find them. Fails when the document does not
    /// exist or its permissions do not allow copying its content.
    pub fn chess_scan(&mut self, id: DocId, pages: &[usize], settings: &ChessScanSettings, read: bool) -> Result<Vec<Arc<ChessPage>>, String> {
        let doc = self.get(id).ok_or("no such document")?;
        doc.check_copying().map_err(|e| e.to_string())?;
        let all = doc.info.pages.len();
        if let Some(p) = pages.iter().find(|p| **p >= all) {
            return Err(format!("page {} is out of range: the document has {all} pages", p + 1));
        }
        let pages: Vec<usize> = if pages.is_empty() { (0..all).collect() } else { pages.to_vec() };
        let settings = settings.sanitized();
        let models = read.then(|| models(settings.models_dir.as_deref()));
        let model = models.as_deref().map(ModelKey::of);
        let wanted = ScanParams { generation: doc.generation, bytes: doc.bytes.len(), settings: settings.clone(), model };
        let mut found: Vec<Option<Arc<ChessPage>>> = pages.iter().map(|&p| self.chess.get(id, p, &wanted)).collect();
        let missing: Vec<usize> = pages.iter().zip(&found).filter(|(_, f)| f.is_none()).map(|(p, _)| *p).collect();
        if !missing.is_empty() {
            let job = self.chess_scan_job(id, &missing, &settings, read).ok_or("no such document")?;
            let scan = match &models {
                Some(m) => job.run(m, |_, _| true),
                None => job.run_with(None, |_, _| true),
            };
            self.store_chess_scan(&scan);
            for page in scan.pages {
                if let Some(slot) = pages.iter().position(|p| *p == page.page).and_then(|i| found.get_mut(i)) {
                    *slot = Some(page);
                }
            }
        }
        Ok(found.into_iter().flatten().collect())
    }

    /// Chess ▸ Scan diagrams on one page: its diagrams in reading order, read with the user's
    /// models. See [`Self::chess_scan`].
    pub fn chess_page(&mut self, id: DocId, page: usize, settings: &ChessScanSettings) -> Result<Arc<ChessPage>, String> {
        self.chess_scan(id, &[page], settings, true)?.into_iter().next().ok_or_else(|| "the page could not be scanned".into())
    }

    /// The state document `id` is in now, to compare with a scan's [`ChessScan::stamp`]: a
    /// screen showing a scan's boxes drops them once the two differ. `None` when there is no
    /// such document.
    pub fn chess_stamp(&self, id: DocId) -> Option<ScanStamp> {
        self.get(id).map(|doc| ScanStamp { generation: doc.generation, bytes: doc.bytes.len() })
    }
}

/// The document state a scan describes (its edit generation and size). Every edit, undo or
/// redo makes a new one, after which the scan's boxes and positions may no longer be on the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanStamp {
    generation: u64,
    bytes: usize,
}

impl ChessScan {
    /// The document state this scan was made from: see [`Session::chess_stamp`].
    pub fn stamp(&self) -> ScanStamp {
        ScanStamp { generation: self.params.generation, bytes: self.params.bytes }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub mod analysis;
pub mod capture;
pub mod collection;
pub mod corrections;
pub mod document;
pub mod epub;
pub mod export;
pub mod keys;
pub mod links;
pub mod office;
pub mod replace;
pub mod samples;
pub mod span;
pub mod text_ocr;

#[cfg(test)]
mod tests;
