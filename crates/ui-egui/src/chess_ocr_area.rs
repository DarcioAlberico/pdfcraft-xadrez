//! Chess ▸ PGN toolbar ▸ OCR area (T9): on a scanned page, a rectangle drawn on the page is read
//! with chess OCR and captured like a selection (PGN toolbar spec §4.3.1, §4.6).
//!
//! **The switch.** OCR area (O, `chess.tool.ocr_area`) is a switch on the capture tools' source:
//! while it is on and a capture tool (Capture or Comment) is armed, a drag on the page draws a
//! rectangle (the Marquee Zoom gesture, `zoom_snap::page_input`) instead of selecting text.
//! Switching it on with nothing armed arms Capture (automatic). It **switches on by itself**
//! when a capture tool is armed on a page whose text layer is poor or missing: the Text tab's
//! reading of that page when it has one, else the engine's check of the layer the view loaded
//! (`Session::chess_layer_quality`, the Text tab's verdict, another program's invisible OCR over
//! a scan included). It is decided once per page shown (and edit), and switches off by itself
//! on a page with a good layer only when it was the page that switched it on; the user's own
//! choice stands until the page changes. The capture chip says "OCR area" while it is on.
//!
//! **Reading.** Letting go reads the area off the interface thread (inline on the web and in
//! tests): `engine::chess::text_ocr::OcrAreaJob`, with the user's `chess-glyphs` model, the
//! general OCR for prose when installed, and the document's book cipher as the Text tab learned
//! it. The chip shows "Reading the area…" meanwhile, and Esc drops the read (the work finishes in
//! the background and its result is thrown away). Reading is extraction: a document that does
//! not allow copying refuses it with the usual notice. Without the model the notice says where to
//! put it.
//!
//! **Capturing.** What was read goes through the same capture as a selection (`chess_capture`):
//! planned, then applied or previewed beside the area. A scan has no weight, so its moves are
//! placed by their numbers only (`CaptureText::Read`). When the cipher or the repair changed or
//! doubted a word, the preview is shown even for a plan that fits; there a move that did not play
//! is corrected with the glyphs' top readings, kept with the request (T10, `chess_correct`).

use std::sync::{Arc, Mutex, MutexGuard};

use egui::{Key, RichText};
use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::document::{OcrMode, OcrOptions};
use pdfcraft_engine::chess::pdfcraft_chess::san::Lang;
use pdfcraft_engine::chess::pdfcraft_chessvision::chess_text::TextQuality;
use pdfcraft_engine::chess::pdfcraft_chessvision::coords::{PageGeometry, ViewRect};
use pdfcraft_engine::chess::text_ocr::{AreaError, AreaRead, MIN_AREA_SIDE};

use crate::chess_capture::{Request, Text};
use crate::chess_tools::{ChessKey, Kind, PgnTool, Tool};
use crate::chess_ui::ChessState;
use crate::theme::Tokens;
use crate::{PdfCraftApp, icons};

/// What an area reader is asked: the area of page `page` (0-based), in user and view space, and
/// the language of the piece letters.
#[derive(Clone, Debug, PartialEq)]
pub struct AreaQuery {
    pub page: usize,
    pub rect_user: [f32; 4],
    pub rect_view: [f32; 4],
    pub lang: Lang,
}

/// Reads areas in place of the engine's chess OCR: tests, or a glyph model loaded another way.
pub type AreaReader = Arc<dyn Fn(&AreaQuery) -> Result<AreaRead, AreaError> + Send + Sync>;

/// The OCR area's state, kept in `CaptureState`.
#[derive(Default)]
pub struct AreaState {
    /// On: with a capture tool armed, a drag on the page draws an area to read.
    pub on: bool,
    /// It was switched on by the page's text layer, not by the user.
    pub auto: bool,
    /// The page the switch was last decided for: document, page and edit generation.
    decided: Option<(DocId, usize, u64)>,
    /// The area being read.
    job: Option<AreaJob>,
    /// Reads areas in place of the engine (see [`PdfCraftApp::set_chess_area_reader`]).
    reader: Option<AreaReader>,
}

/// The read's result, once there is one.
type Slot = Arc<Mutex<Option<Result<AreaRead, AreaError>>>>;

/// An area being read, and what to capture it as.
struct AreaJob {
    doc: DocId,
    page: usize,
    geometry: Option<PageGeometry>,
    /// The document's edit generation when the area was drawn.
    generation: u64,
    mode: pdfcraft_engine::chess::capture::CaptureMode,
    replace: bool,
    lang: Lang,
    /// The document whose games it goes into.
    games: Option<DocId>,
    result: Slot,
}

/// The slot, even after a worker panicked while holding it (it holds plain data).
fn lock(m: &Mutex<Option<Result<AreaRead, AreaError>>>) -> MutexGuard<'_, Option<Result<AreaRead, AreaError>>> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Fills the slot when the worker ends without a result, so the interface never waits for a
/// worker that is gone.
struct Finish(Slot);

impl Drop for Finish {
    fn drop(&mut self) {
        let mut slot = lock(&self.0);
        if slot.is_none() {
            *slot = Some(Err(AreaError::Read("the reading stopped unexpectedly".into())));
        }
    }
}

/// OCR area (O): the switch on the capture tools' source.
pub(crate) const OCR_AREA: Tool = Tool {
    id: "ocr-area",
    icon: "scan-text",
    label: |_| tl!("OCR area").to_owned(),
    keys: &[ChessKey::plain(Key::O)],
    command: Some("chess.tool.ocr_area"),
    kind: Kind::Switch(toggle),
    enabled: |b| b.illegal.is_none(),
    visible: |_| true,
    heads: None,
    on_move: None,
    on: Some(|chess| chess.capture.area.on),
};

/// The switch, pressed: the user's choice from now on. On, it arms Capture when no capture tool
/// is armed.
fn toggle(chess: &mut ChessState) {
    let area = &mut chess.capture.area;
    area.on = !area.on;
    area.auto = false;
    if area.on && crate::chess_capture::armed(chess).is_none() {
        crate::chess_capture::arm(chess, PgnTool::Capture(pdfcraft_engine::chess::capture::CaptureMode::Auto));
    }
}

/// Whether a drag on the page draws an area to read: the switch is on and a capture tool armed.
pub(crate) fn drawing(chess: &ChessState) -> bool {
    chess.capture.area.on && crate::chess_capture::armed(chess).is_some()
}

/// Whether an area is being read.
pub(crate) fn reading(chess: &ChessState) -> bool {
    chess.capture.area.job.is_some()
}

/// Esc: drop the area being read. Whether there was one.
pub(crate) fn cancel(chess: &mut ChessState) -> bool {
    chess.capture.area.job.take().is_some()
}

/// The capture chip's part: "OCR area" while the switch is on (a click turns it off), with a
/// spinner while an area is read.
pub(crate) fn chip(ui: &mut egui::Ui, t: &Tokens, chess: &mut ChessState) {
    if !chess.capture.area.on {
        return;
    }
    ui.label(RichText::new("·").color(t.text_faint));
    if reading(chess) {
        ui.spinner();
    } else {
        ui.add(icons::image("scan-text", 14.0, t.accent));
    }
    let r = ui.selectable_label(true, tl!("OCR area"));
    let tip = if chess.capture.area.auto {
        tl!("This page's text layer is missing or can't be trusted, so a drag draws an area to read with chess OCR. Click to select text instead.")
    } else {
        tl!("A drag draws an area to read with chess OCR. Click to select text instead.")
    };
    if r.on_hover_text(tip).clicked() {
        toggle(chess);
    }
}

/// What an area error tells the user.
fn error_text(e: &AreaError) -> String {
    match e {
        AreaError::NoModel => {
            tl!("Chess OCR needs the chess-glyphs model. Use Chess models… in the chess panel to give the app chess-glyphs.onnx.").to_owned()
        }
        AreaError::NotPermitted => crate::copy_refused().to_string(),
        AreaError::TooLarge => tl!("The area is too large to read at once: draw a smaller one.").to_owned(),
        other => crate::i18n::fmt(tl!("The area couldn't be read: {e}"), &[("e", &other.to_string())]),
    }
}

impl PdfCraftApp {
    /// Read areas with `reader` in place of the engine's chess OCR (tests; a glyph model loaded
    /// another way). `None` goes back to the engine.
    pub fn set_chess_area_reader(&mut self, reader: Option<impl Fn(&AreaQuery) -> Result<AreaRead, AreaError> + Send + Sync + 'static>) {
        self.chess.capture.area.reader = reader.map(|r| Arc::new(r) as AreaReader);
    }

    /// Whether an area is being read with chess OCR (tests and automation).
    pub fn chess_area_reading(&self) -> bool {
        reading(&self.chess)
    }

    /// Read area `view_rect` (view space) of page `page` in view `vi` with chess OCR, then capture
    /// it as the armed tool says. A click (no area) does nothing; a document that does not allow
    /// copying refuses it with the usual notice.
    pub(crate) fn capture_area(&mut self, vi: usize, page: usize, view_rect: [f32; 4]) {
        let Some((mode, replace)) = crate::chess_capture::armed(&self.chess) else { return };
        let Some(id) = self.views.get(vi).map(|v| v.id) else { return };
        let [x0, y0, x1, y1] = view_rect;
        if !(x1 - x0 >= MIN_AREA_SIDE && y1 - y0 >= MIN_AREA_SIDE) {
            return;
        }
        if !self.may_copy(id) {
            return;
        }
        let Some(doc) = self.session.get(id) else { return };
        let generation = doc.edit_generation();
        let geometry = doc.info.pages.get(page).and_then(pdfcraft_engine::chess::geometry);
        let Some(user) = geometry.and_then(|g| g.view_to_user(ViewRect::new(x0, y0, x1, y1))) else { return };
        let job = match self.session.chess_ocr_area_job(id, page, user) {
            Ok(job) => job,
            Err(e) => {
                self.notify(error_text(&e));
                return;
            }
        };
        self.sync_chess_docs();
        let lang = crate::chess_capture::document_lang(&self.chess, Some(id));
        let games = self.chess.games.current_doc();
        let book = self.chess.cipher.as_ref().filter(|(d, _)| *d == id).map(|(_, c)| c.clone());
        let query = AreaQuery { page, rect_user: [user.x0, user.y0, user.x1, user.y1], rect_view: job.rect_view(), lang };
        let reader = self.chess.capture.area.reader.clone();
        let models_dir = self.chess.models.models_dir();
        let result: Slot = Arc::default();
        let slot = result.clone();
        let run = move || {
            let _finish = Finish(slot.clone());
            let read = match reader {
                Some(reader) => reader(&query),
                None => job.run(&OcrOptions { mode: OcrMode::Off, glyphs: None, general: true, book: book.as_ref() }, lang, models_dir.as_deref()),
            };
            *lock(&slot) = Some(read);
        };
        self.chess.capture.area.job = Some(AreaJob { doc: id, page, geometry, generation, mode, replace, lang, games, result });
        #[cfg(not(target_arch = "wasm32"))]
        if self.run_inline {
            run();
        } else if let Err(e) = std::thread::Builder::new().name("pdfcraft-ocr-area".into()).spawn(run) {
            self.chess.capture.area.job = None;
            self.notify(e.to_string());
            return;
        }
        #[cfg(target_arch = "wasm32")]
        run();
        self.poll_chess_area();
    }

    /// Every frame: decide the switch for the page shown, and pick up an area read.
    pub(crate) fn poll_chess_area(&mut self) {
        self.decide_chess_area();
        let Some(job) = self.chess.capture.area.job.as_ref() else { return };
        let Some(done) = lock(&job.result).take() else {
            if let Some(ctx) = &self.ctx {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            return;
        };
        let Some(job) = self.chess.capture.area.job.take() else { return };
        // The page read is not the page shown any more.
        if self.session.get(job.doc).map(pdfcraft_engine::Document::edit_generation) != Some(job.generation) {
            self.notify(tl!("The area read was dropped: the document was edited.").to_owned());
            return;
        }
        match done {
            Err(e) => self.notify(error_text(&e)),
            Ok(read) if read.text.glyphs.iter().all(|g| g.text.trim().is_empty()) => {
                self.notify(tl!("Chess OCR read nothing in the area.").to_owned());
            }
            Ok(read) => {
                let read = Arc::new(read);
                let text = Text::Page {
                    page: job.page,
                    glyphs: Arc::new(read.text.glyphs.clone()),
                    geometry: job.geometry,
                    rects: vec![read.rect_view],
                    read: Some(read),
                };
                self.start_capture(Request {
                    mode: job.mode,
                    replace: job.replace,
                    text,
                    doc: job.games,
                    lang: job.lang,
                    generation: Some(job.generation),
                });
            }
        }
    }

    /// With a capture tool armed, switch the OCR area on for a page shown whose text layer is
    /// poor or missing, and back off on a page with a good one when a page switched it on. Once
    /// per page (and edit), when its layer is known.
    fn decide_chess_area(&mut self) {
        if crate::chess_capture::armed(&self.chess).is_none() {
            return;
        }
        let Some((vi, id)) = self.active_ids() else { return };
        let Some(page) = self.views.get(vi).map(|v| v.current) else { return };
        let Some(generation) = self.session.get(id).map(pdfcraft_engine::Document::edit_generation) else { return };
        let key = (id, page, generation);
        if self.chess.capture.area.decided == Some(key) {
            return;
        }
        let Some(poor) = self.layer_poor(vi, id, page) else { return };
        let area = &mut self.chess.capture.area;
        area.decided = Some(key);
        if poor {
            if !area.on {
                area.on = true;
                area.auto = true;
            }
        } else if area.auto {
            area.on = false;
            area.auto = false;
        }
    }

    /// Whether page `page`'s text layer is poor or missing: the Text tab's reading of it when it
    /// is current, else the layer view `vi` loaded, checked by the engine. `None` while neither
    /// is there.
    fn layer_poor(&self, vi: usize, id: DocId, page: usize) -> Option<bool> {
        let stamp = self.session.chess_stamp(id);
        if let Some(t) = self.chess.text.as_ref().filter(|t| t.doc == id && t.page == page && Some(t.stamp) == stamp) {
            return Some(t.ocr || t.needs_ocr.is_some());
        }
        let text = self.views.get(vi)?.texts.get(&page)?.clone();
        let quality = self.session.chess_layer_quality(id, page, &text)?;
        Some(quality != TextQuality::Good)
    }
}
