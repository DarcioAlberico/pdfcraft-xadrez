//! Chess ▸ Scan diagrams as tools (blueprint S9): `chess_detect`, `chess_read`, `chess_status`;
//! and Chess ▸ Replace diagram (S10): `chess_replace_diagram`.
//!
//! The scanning tools are read-only: they find and read diagrams, and never change a document.
//! They report the document's edit `generation`, which `chess_replace_diagram` takes back so a
//! diagram numbered before a later edit is never the one replaced. Pages and
//! diagram numbers are 1-based; diagram `n` is the `n`-th in the engine's reading order, the same
//! one the interface and exports use. Boxes are given twice: `rect` in points from the top-left
//! of the displayed page (the tools' convention) and `bbox_user` in PDF user space (where links
//! and replacements go).

use std::sync::Arc;

use pdfcraft_engine::Edit;
use pdfcraft_engine::chess::document::{OcrMode, OcrOptions};
use pdfcraft_engine::chess::export::{ChessExportOptions, ChessFormat};
use pdfcraft_engine::chess::links::Side;
use pdfcraft_engine::chess::pdfcraft_chess::draw::{BoardStyle, PieceSet, Rgb};
use pdfcraft_engine::chess::pdfcraft_chess::fen;
use pdfcraft_engine::chess::pdfcraft_chess::rules::Legality;
use pdfcraft_engine::chess::pdfcraft_chess::san::Lang;
use pdfcraft_engine::chess::pdfcraft_chessvision::board::{BoardReading, ReadPosition, ReadingSource, Unreadable};
use pdfcraft_engine::chess::pdfcraft_chessvision::cipher::BookCipher;
use pdfcraft_engine::chess::pdfcraft_chessvision::coords::{PageGeometry, PageRect};
use pdfcraft_engine::chess::pdfcraft_chessvision::detect::Source;
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::{Block, Inline, MoveInline, MoveStatus};
use pdfcraft_engine::chess::pdfcraft_chessvision::merge::Merged;
use pdfcraft_engine::chess::pdfcraft_chessvision::pieces::{self, PieceSource};
use pdfcraft_engine::chess::{self, ChessPage, ChessScanSettings, DiagramReplacement, LinkSettings, ModelState, ReadingOrder};
use serde_json::{Value, json};

/// Where to save the book cipher: the path as given, and resolved in the root.
type CipherOut = (String, std::path::PathBuf);

/// `ocr`: when chess OCR reads a page instead of its text layer (default auto: when the layer is
/// poor or missing, or another program's invisible OCR over a scan). The general OCR reads the
/// prose when its models are installed.
fn ocr_options(a: &Args) -> Result<OcrOptions<'static>> {
    let mode = match a.opt_str("ocr")? {
        None | Some("auto") => OcrMode::Auto,
        Some("off") => OcrMode::Off,
        Some("all") => OcrMode::All,
        Some(_) => return Err(Args::wrong("ocr", "\"auto\", \"off\" or \"all\"")),
    };
    Ok(OcrOptions { mode, glyphs: None, general: true, book: None })
}

use crate::{Args, Automation, Result, ToolError, comments, failed};

/// What `pieces` takes, for the error that refuses anything else.
fn pieces_expected() -> String {
    let ids: Vec<&str> = pieces::bundled::SETS.iter().map(|s| s.id).collect();
    format!("\"installed\", \"geometric\" or a set PdfCraft ships ({})", ids.join(", "))
}

/// Two decimals: enough for points, and stable in test output.
fn round(v: f32) -> f64 {
    (f64::from(v) * 100.0).round() / 100.0
}

fn user_rect(r: PageRect) -> Value {
    json!([round(r.x0), round(r.y0), round(r.x1), round(r.y1)])
}

/// A user-space box in points from the top-left of the displayed page.
fn view_rect(geometry: Option<&PageGeometry>, r: PageRect) -> Value {
    match geometry.and_then(|g| g.user_to_view(r)) {
        Some(v) => json!([round(v.x0), round(v.y0), round(v.x1), round(v.y1)]),
        None => Value::Null,
    }
}

fn source_name(s: Source) -> &'static str {
    match s {
        Source::Embedded => "image",
        Source::Contour => "contour",
        Source::Font => "font",
        Source::Vector => "vector",
    }
}

fn diagram(index: usize, d: &Merged, geometry: Option<&PageGeometry>) -> Value {
    let c = &d.candidate;
    let corners = c.corners.map(|cs| {
        cs.iter()
            .map(|&(x, y)| match geometry.and_then(|g| g.user_to_view_point(x as f32, y as f32)) {
                Some([vx, vy]) => json!([round(vx), round(vy)]),
                None => Value::Null,
            })
            .collect::<Vec<_>>()
    });
    json!({
        "index": index + 1,
        "rect": view_rect(geometry, c.bbox_user),
        "bbox_user": user_rect(c.bbox_user),
        "source": source_name(c.source),
        "sources": d.sources.iter().map(|s| source_name(*s)).collect::<Vec<_>>(),
        "score": round(c.score),
        "dpi_hint": round(c.dpi_hint),
        "corners": corners,
        "position_in_text": c.placement.is_some(),
    })
}

fn unreadable_code(u: &Unreadable) -> &'static str {
    match u {
        Unreadable::NoModel => "no_model",
        Unreadable::BadDpi => "bad_dpi",
        Unreadable::BadGeometry => "bad_geometry",
        Unreadable::OffPage => "off_page",
        Unreadable::DegenerateQuad => "degenerate_quad",
        Unreadable::TooMany => "too_many",
        Unreadable::Model(_) => "model_failed",
        Unreadable::Probabilities(_) => "bad_model_output",
    }
}

fn position(p: &ReadPosition) -> Value {
    let (legality, violations) = match &p.legality {
        Legality::Legal => ("legal".to_string(), Vec::new()),
        Legality::OnlyWithSideToMove { side, other } => {
            (format!("legal_only_with_{}_to_move", side.fen_char()), other.iter().map(|v| format!("{v:?}")).collect())
        }
        Legality::Illegal(v) => ("illegal".to_string(), v.iter().map(|v| format!("{v:?}")).collect()),
    };
    let piece = |p: Option<pdfcraft_engine::chess::pdfcraft_chess::placement::Piece>| p.map(|p| p.fen_char().to_string());
    json!({
        "fen": p.fen,
        "legal": p.legality.allows(p.side.color),
        "legality": legality,
        "violations": violations,
        "side_to_move": p.side.color.fen_char().to_string(),
        "side_source": format!("{:?}", p.side.source),
        "side_assumed": p.side.is_assumed(),
        "orientation": p.orientation.map(|v| json!({
            "upright": v.upright,
            "rule": format!("{:?}", v.rule),
            "ambiguous": v.ambiguous,
            "black_point_of_view": v.black_point_of_view,
            "black_view_from_pieces": v.black_view_from_pieces,
        })),
        "min_confidence": round(p.min_confidence),
        "accepted": p.accepted,
        "uncertain": p.uncertain.iter().map(|u| json!({
            "square": u.square.to_string(),
            "confidence": round(u.confidence),
            "alternative": piece(u.alternative),
            "alternative_confidence": round(u.alternative_confidence),
        })).collect::<Vec<_>>(),
        "repaired": p.repaired(),
        "repairs": p.repairs.iter().map(|c| json!({ "square": c.square.to_string(), "from": piece(c.from), "to": piece(c.to) })).collect::<Vec<_>>(),
        "lichess_url": chess::lichess_url(p),
    })
}

fn reading(r: &BoardReading, geometry: Option<&PageGeometry>) -> Value {
    let mut out = json!({
        "index": r.index + 1,
        "rect": view_rect(geometry, r.bbox_user),
        "bbox_user": user_rect(r.bbox_user),
        "read_by": match r.source { ReadingSource::Font => "font", ReadingSource::Model => "model" },
        "dpi": round(r.dpi),
    });
    let extra = match &r.outcome {
        Ok(p) => position(p),
        Err(u) => json!({ "unreadable": unreadable_code(u), "reason": u.to_string() }),
    };
    if let (Some(o), Value::Object(e)) = (out.as_object_mut(), extra) {
        o.extend(e);
    }
    out
}

fn page(p: &ChessPage) -> Value {
    let g = p.geometry.as_ref();
    let mut out = json!({
        "page": p.page + 1,
        "dpi": round(p.dpi),
        "diagrams": p.diagrams.iter().enumerate().map(|(i, d)| diagram(i, d, g)).collect::<Vec<_>>(),
        "truncated": p.truncated,
        "error": p.error,
        "notes": p.notes,
    });
    if let (Some(o), Some(readings)) = (out.as_object_mut(), &p.readings) {
        o.insert("readings".into(), readings.iter().map(|r| reading(r, g)).collect());
    }
    out
}

fn page_rect(r: [f64; 4]) -> PageRect {
    let [x0, y0, x1, y1] = r.map(|v| v as f32);
    PageRect::new(x0, y0, x1, y1)
}

/// `light`, `dark`, `border`, `border_color`, `coordinates` and `flipped`, over the default style.
fn board_style(a: &Args) -> Result<BoardStyle> {
    let mut style = BoardStyle::default();
    let colour = |key: &str| -> Result<Option<Rgb>> {
        Ok(a.opt_str(key)?.map(comments::parse_color).transpose()?.map(|c| {
            let [r, g, b] = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
            Rgb(r, g, b)
        }))
    };
    if let Some(c) = colour("light")? {
        style.light = c;
    }
    if let Some(c) = colour("dark")? {
        style.dark = c;
    }
    if let Some(c) = colour("border_color")? {
        style.border_color = c;
    }
    if let Some(b) = a.opt_num("border")? {
        if !(0.0..=1.0).contains(&b) {
            return Err(Args::wrong("border", "from 0 to 1 (a share of a square)"));
        }
        style.border = b as f32;
    }
    if let Some(c) = a.opt_bool("coordinates")? {
        style.coordinates = c;
    }
    if let Some(f) = a.opt_bool("flipped")? {
        style.flipped = f;
    }
    Ok(style)
}

fn model_json<T>(state: &ModelState<T>) -> Value {
    let (word, reason) = chess::model_state(state);
    json!({ "state": word, "reason": reason })
}

impl Automation {
    /// The scan settings from the arguments. Out-of-range numbers are refused, not clamped, so
    /// the agent learns what is allowed.
    fn chess_settings(&self, a: &Args) -> Result<ChessScanSettings> {
        let mut s = ChessScanSettings::default();
        if let Some(dpi) = a.opt_num("dpi")? {
            if !(f64::from(chess::MIN_DPI)..=f64::from(chess::MAX_DPI)).contains(&dpi) {
                return Err(Args::wrong("dpi", &format!("between {} and {}", chess::MIN_DPI, chess::MAX_DPI)));
            }
            s.dpi = dpi as f32;
        }
        if let Some(dpi) = a.opt_num("max_read_dpi")? {
            if !(f64::from(s.dpi)..=f64::from(chess::MAX_DPI)).contains(&dpi) {
                return Err(Args::wrong("max_read_dpi", &format!("between dpi ({}) and {}", s.dpi, chess::MAX_DPI)));
            }
            s.max_read_dpi = dpi as f32;
        }
        if let Some(order) = a.opt_str("order")? {
            s.order = match order {
                "column" => ReadingOrder::Column,
                "row" => ReadingOrder::Row,
                _ => return Err(Args::wrong("order", "\"column\" or \"row\"")),
            };
        }
        if let Some(n) = a.opt_int("max_per_page")? {
            s.max_diagrams = usize::try_from(n)
                .ok()
                .filter(|n| (1..=chess::MAX_DIAGRAMS_PER_PAGE).contains(n))
                .ok_or_else(|| Args::wrong("max_per_page", &format!("from 1 to {}", chess::MAX_DIAGRAMS_PER_PAGE)))?;
        }
        if let Some(b) = a.opt_bool("allow_unverified_fonts")? {
            s.allow_unverified_fonts = b;
        }
        s.models_dir = self.models_dir(a)?;
        Ok(s)
    }

    /// `models_dir`, confined to the root like every other path.
    pub(crate) fn models_dir(&self, a: &Args) -> Result<Option<std::path::PathBuf>> {
        let Some(dir) = a.opt_str("models_dir")? else { return Ok(None) };
        let path = self.resolve(dir, false)?;
        if !path.is_dir() {
            return Err(failed(format!("models_dir {dir} is not a folder")));
        }
        Ok(Some(path))
    }

    fn chess_pages(&mut self, a: &Args, settings: &ChessScanSettings, read: bool) -> Result<Vec<std::sync::Arc<ChessPage>>> {
        let pages = if a.opt_ints("pages")?.is_some() { self.pages(a, "pages")? } else { Vec::new() };
        let id = self.doc(a)?.id;
        self.session.chess_scan(id, &pages, settings, read).map_err(failed)
    }

    pub(crate) fn chess_detect(&mut self, a: &Args) -> Result<Value> {
        let settings = self.chess_settings(a)?;
        let pages = self.chess_pages(a, &settings, false)?;
        let mut detected: Vec<Value> = pages.iter().map(|p| page(p)).collect();
        // A cached read answers a detection: leave its readings out, as asked.
        for p in &mut detected {
            if let Some(o) = p.as_object_mut() {
                o.remove("readings");
            }
        }
        let generation = self.doc(a)?.edit_generation();
        Ok(json!({ "diagrams": pages.iter().map(|p| p.diagrams.len()).sum::<usize>(), "generation": generation, "pages": detected }))
    }

    pub(crate) fn chess_read(&mut self, a: &Args) -> Result<Value> {
        let settings = self.chess_settings(a)?;
        let models = chess::models(settings.models_dir.as_deref());
        let pages = self.chess_pages(a, &settings, true)?;
        let readings = || pages.iter().flat_map(|p| p.readings.iter().flatten());
        let mut model = model_json(&models.pieces);
        if let Some(o) = model.as_object_mut() {
            o.insert("folder".into(), models.folder.as_ref().map(|f| f.to_string_lossy().into_owned()).into());
        }
        let generation = self.doc(a)?.edit_generation();
        Ok(json!({
            "model": model,
            "generation": generation,
            "diagrams": pages.iter().map(|p| p.diagrams.len()).sum::<usize>(),
            "read": readings().filter(|r| r.position().is_some()).count(),
            "accepted": readings().filter(|r| r.accepted()).count(),
            "pages": pages.iter().map(|p| page(p)).collect::<Vec<_>>(),
        }))
    }

    /// Chess ▸ Replace diagram: plan it against the page as it is now, and (unless `dry_run`)
    /// apply it as one undoable step.
    pub(crate) fn chess_replace_diagram(&mut self, a: &Args) -> Result<Value> {
        let page = self.page(a)?;
        let index = a.int("index")?;
        let index = usize::try_from(index).ok().and_then(|i| i.checked_sub(1)).ok_or_else(|| Args::wrong("index", "1 or more"))?;
        let generation = u64::try_from(a.int("generation")?).map_err(|_| Args::wrong("generation", "0 or more"))?;
        let placement = match (a.opt_str("fen")?, a.opt_str("placement")?) {
            (Some(fen), None) => fen::parse(fen).map_err(|e| ToolError::InvalidArgs(format!("fen: {e}")))?.placement,
            (None, Some(p)) => fen::parse_placement(p).map_err(|e| ToolError::InvalidArgs(format!("placement: {e}")))?,
            _ => return Err(ToolError::InvalidArgs("give either fen or placement (the first field of a FEN), not both".into())),
        };
        let style = board_style(a)?;
        let (pieces, source, error) = match a.opt_str("pieces")? {
            None | Some("installed") => pieces::find(),
            Some("geometric") => (PieceSet::geometric(), PieceSource::Geometric, None),
            Some(id) => pieces::from_bundled(pieces::bundled::by_id(id).ok_or_else(|| Args::wrong("pieces", &pieces_expected()))?),
        };
        let mut settings = self.chess_settings(a)?;
        settings.models_dir = None;
        let replacement = DiagramReplacement { page, index, generation, placement, style, pieces: Arc::new(pieces), scan: settings };
        let id = self.doc(a)?.id;
        let plan = self.session.chess_replacement(id, &replacement).map_err(failed)?;
        let geometry = self.session.chess_scan(id, &[page], &replacement.scan, false).ok().and_then(|p| p.first().and_then(|p| p.geometry));
        let dry_run = a.opt_bool("dry_run")?.unwrap_or(false);
        let mut out = if dry_run { json!({}) } else { self.apply(a, Edit::ReplaceDiagram(Box::new(replacement)))? };
        let user = |r: [f64; 4]| page_rect(r);
        let removed = match (plan.deleted_image, plan.removed_lines.is_empty()) {
            (Some(_), _) => "image_deleted",
            (None, false) => "text_removed_and_covered",
            (None, true) => "covered",
        };
        let fields = json!({
            "dry_run": dry_run,
            "page": page + 1,
            "index": index + 1,
            "source": source_name(plan.source),
            "sources": plan.sources.iter().map(|s| source_name(*s)).collect::<Vec<_>>(),
            "bbox_user": user_rect(plan.bbox_user),
            "rect": view_rect(geometry.as_ref(), plan.bbox_user),
            "old_diagram": removed,
            "deleted_image": plan.deleted_image.map(|i| i + 1),
            "removed_text_lines": plan.removed_lines.len(),
            "cover_user": plan.cover.map(|c| user_rect(user(c))),
            "board_user": user_rect(user(plan.board_user)),
            "board_rect": view_rect(geometry.as_ref(), user(plan.board_user)),
            "fen": fen::format_placement(&placement),
            "pieces": match source {
                PieceSource::Font { family, .. } => json!({ "source": "font", "family": family, "error": Value::Null }),
                PieceSource::Images { dir } => json!({ "source": "images", "family": dir.to_string_lossy(), "error": Value::Null }),
                PieceSource::Bundled { id } => json!({ "source": "bundled", "family": id, "error": Value::Null }),
                PieceSource::Geometric => json!({ "source": "geometric", "family": Value::Null, "error": error.map(|e| e.to_string()) }),
            },
            "notes": plan.notes,
        });
        if let (Some(o), Value::Object(f)) = (out.as_object_mut(), fields) {
            o.extend(f);
        }
        Ok(out)
    }

    /// Chess ▸ Recognised document: the pages read into a `ChessDoc`. A summary always; the
    /// document as JSON and the games as PGN inline (`include`) or written to `out` / `pgn_out`.
    pub(crate) fn chess_document(&mut self, a: &Args) -> Result<Value> {
        let settings = self.chess_settings(a)?;
        let pages = if a.opt_ints("pages")?.is_some() { self.pages(a, "pages")? } else { Vec::new() };
        let lang = match a.opt_str("lang")? {
            None => None,
            Some(tag) => Some(Lang::from_tag(tag).ok_or_else(|| Args::wrong("lang", "a piece-letter language such as en, de, pt or fr"))?),
        };
        let mut inline_json = false;
        let mut inline_pgn = false;
        let include = if a.get("include").is_some_and(|v| !v.is_null()) { a.strs("include")? } else { Vec::new() };
        for item in include {
            match item {
                "json" => inline_json = true,
                "pgn" => inline_pgn = true,
                _ => return Err(Args::wrong("include", "a list of \"json\" and \"pgn\"")),
            }
        }
        // Resolved before reading, so a path outside the root costs nothing.
        let out = a.opt_str("out")?.map(|p| self.resolve(p, true).map(|r| (p, r))).transpose()?;
        let pgn_out = a.opt_str("pgn_out")?.map(|p| self.resolve(p, true).map(|r| (p, r))).transpose()?;
        let id = self.doc(a)?.id;
        let (seed, cipher_out) = self.cipher_args(a)?;
        let ocr = OcrOptions { book: seed.as_ref(), ..ocr_options(a)? };
        let read = self.session.chess_document_with(id, &pages, &settings, lang, &ocr).map_err(failed)?;
        let cipher = Self::cipher_reply(&read.cipher, cipher_out.as_ref())?;
        let doc = &read.doc;
        let blocks = doc.pages.iter().flat_map(|p| &p.blocks);
        let count = |f: fn(&Block) -> bool| doc.pages.iter().flat_map(|p| &p.blocks).filter(|b| f(b)).count();
        let moves: Vec<&MoveInline> = blocks.flat_map(|b| b.inlines()).filter_map(|i| if let Inline::Move(m) = i { Some(m) } else { None }).collect();
        let json_text = doc.to_json().map_err(|e| failed(format!("the document could not be written as JSON: {e}")))?;
        let pgn = doc.pgn();
        let mut result = json!({
            "pages": doc.pages.len(),
            "lang": read.lang.lang.code(),
            "lang_given": read.lang_given,
            "headings": count(|b| matches!(b, Block::Heading { .. })),
            "paragraphs": count(|b| matches!(b, Block::Paragraph { .. })),
            "diagrams": count(|b| matches!(b, Block::Diagram(_))),
            "games": doc.games.len(),
            "moves": moves.len(),
            "valid_moves": moves.iter().filter(|m| m.status == MoveStatus::Valid).count(),
            "flagged_moves": moves.iter().filter(|m| matches!(m.status, MoveStatus::Illegal | MoveStatus::Ambiguous)).count(),
            "caption_moves": moves.iter().filter(|m| m.status == MoveStatus::Caption).count(),
            "needs_ocr": read.needs_ocr.iter().map(|(p, why)| json!({ "page": p + 1, "reason": why })).collect::<Vec<_>>(),
            "ocr_pages": read.ocr_pages.iter().map(|o| json!({ "page": o.page + 1, "lines": o.lines, "repaired": o.edits, "unsettled": o.flags, "general_ocr": o.general_ocr })).collect::<Vec<_>>(),
            "cipher": cipher,
            "notes": read.notes.iter().map(|(p, n)| json!({ "page": p + 1, "note": n })).collect::<Vec<_>>(),
        });
        if let Some((shown, path)) = out {
            crate::write_atomic(&path, json_text.as_bytes())?;
            result["out"] = json!(shown);
        }
        if let Some((shown, path)) = pgn_out {
            crate::write_atomic(&path, pgn.as_bytes())?;
            result["pgn_out"] = json!(shown);
        }
        if inline_json {
            result["document"] = serde_json::from_str(&json_text).map_err(|e| failed(format!("the document JSON could not be read back: {e}")))?;
        }
        if inline_pgn {
            result["pgn"] = json!(pgn);
        }
        Ok(result)
    }

    /// Chess ▸ Add lichess links: plan the labels on the pages as they are now and (unless
    /// `dry_run`) add them as one undoable step.
    pub(crate) fn chess_add_lichess_links(&mut self, a: &Args) -> Result<Value> {
        let scan = self.chess_settings(a)?;
        let pages = if a.opt_ints("pages")?.is_some() { self.pages(a, "pages")? } else { Vec::new() };
        let mut settings = LinkSettings::default();
        if let Some(text) = a.opt_str("label")? {
            settings.text = text.to_owned();
        }
        if let Some(size) = a.opt_num("size")? {
            settings.size = size;
        }
        settings.allow_unaccepted = a.opt_bool("allow_unaccepted")?.unwrap_or(false);
        let dry_run = a.opt_bool("dry_run")?.unwrap_or(false);
        let id = self.doc(a)?.id;
        let plan = self.session.chess_link_plan(id, &pages, &scan, &settings).map_err(failed)?;
        let mut out = match (&plan.edit, dry_run) {
            (Some(edit), false) => self.apply(a, edit.clone())?,
            _ => json!({}),
        };
        let fields = json!({
            "dry_run": dry_run,
            "added": if dry_run { 0 } else { plan.links.len() },
            "links": plan.links.iter().map(|l| json!({
                "page": l.page + 1,
                "index": l.index + 1,
                "fen": l.fen,
                "url": l.url,
                "side": match l.side { Side::Below => "below", Side::Right => "right", Side::Above => "above" },
                "rect": ([l.label_view.x0, l.label_view.y0, l.label_view.x1, l.label_view.y1].map(round)),
                "link_user": (l.link_user.map(|v| round(v as f32))),
            })).collect::<Vec<_>>(),
            "skipped": plan.skipped.iter().map(|s| json!({ "page": s.page + 1, "index": s.index + 1, "reason": s.reason })).collect::<Vec<_>>(),
        });
        if let (Some(o), Value::Object(f)) = (out.as_object_mut(), fields) {
            o.extend(f);
        }
        Ok(out)
    }

    /// Chess ▸ Export ▸ HTML: the recognised document as one static HTML file at `out`.
    pub(crate) fn chess_export_html(&mut self, a: &Args) -> Result<Value> {
        self.chess_export(a, ChessFormat::Html)
    }

    /// Chess ▸ Export ▸ EPUB: the same document as an EPUB 3 book at `out`.
    pub(crate) fn chess_export_epub(&mut self, a: &Args) -> Result<Value> {
        self.chess_export(a, ChessFormat::Epub)
    }

    /// Chess ▸ Export ▸ Word: the same document as a .docx file at `out`, diagrams as images.
    pub(crate) fn chess_export_docx(&mut self, a: &Args) -> Result<Value> {
        self.chess_export(a, ChessFormat::Docx)
    }

    /// Chess ▸ Export ▸ Markdown: the same document as a .md file at `out`, diagrams as PNG files
    /// in a folder beside it.
    pub(crate) fn chess_export_markdown(&mut self, a: &Args) -> Result<Value> {
        self.chess_export(a, ChessFormat::Markdown)
    }

    /// Chess ▸ Export ▸ PGN: every game, and every legal diagram as a position, at `out`.
    pub(crate) fn chess_export_pgn(&mut self, a: &Args) -> Result<Value> {
        self.chess_export(a, ChessFormat::Pgn)
    }

    /// Chess ▸ Export ▸ RTF: the same document as an .rtf file at `out`, diagrams as pictures.
    pub(crate) fn chess_export_rtf(&mut self, a: &Args) -> Result<Value> {
        self.chess_export(a, ChessFormat::Rtf)
    }

    fn chess_export(&mut self, a: &Args, format: ChessFormat) -> Result<Value> {
        use pdfcraft_engine::chess::pdfcraft_chessvision::doc::html::{DiagramMode, Figurines};
        let lang_of = |key: &str, tag: &str| Lang::from_tag(tag).ok_or_else(|| Args::wrong(key, "a piece-letter language such as en, de, pt or fr"));
        let lang = a.opt_str("lang")?.map(|t| lang_of("lang", t)).transpose()?;
        let diagrams = match a.opt_str("diagrams")? {
            None | Some("svg") => DiagramMode::Svg,
            Some("fen") => DiagramMode::Fen,
            Some(_) => return Err(Args::wrong("diagrams", "\"svg\" or \"fen\"")),
        };
        let figurines = match a.opt_str("figurines")? {
            None | Some("unicode") => Figurines::Unicode,
            Some("printed") => Figurines::AsPrinted,
            Some("letters") => Figurines::Letters(match a.opt_str("letters_lang")? {
                Some(t) => lang_of("letters_lang", t)?,
                None => lang.unwrap_or(Lang::En),
            }),
            Some(_) => return Err(Args::wrong("figurines", "\"unicode\", \"letters\" or \"printed\"")),
        };
        let (pieces, source, error) = match a.opt_str("pieces")? {
            None | Some("installed") => pieces::find(),
            Some("geometric") => (PieceSet::geometric(), PieceSource::Geometric, None),
            Some(id) => pieces::from_bundled(pieces::bundled::by_id(id).ok_or_else(|| Args::wrong("pieces", &pieces_expected()))?),
        };
        let link_text = a.opt_str("link_text")?.unwrap_or(chess::links::DEFAULT_TEXT).to_owned();
        if link_text.trim().is_empty() || link_text.chars().count() > 80 {
            return Err(Args::wrong("link_text", "1 to 80 characters"));
        }
        let diagram_px = match a.opt_int("diagram_px")? {
            None => chess::office::DEFAULT_DIAGRAM_PX,
            Some(n) => u32::try_from(n)
                .ok()
                .filter(|n| (32..=chess::office::MAX_DIAGRAM_PX).contains(n))
                .ok_or_else(|| Args::wrong("diagram_px", &format!("from 32 to {}", chess::office::MAX_DIAGRAM_PX)))?,
        };
        let ocr = ocr_options(a)?.mode;
        let (book, cipher_out) = self.cipher_args(a)?;
        let scan = self.chess_settings(a)?;
        let pages = if a.opt_ints("pages")?.is_some() { self.pages(a, "pages")? } else { Vec::new() };
        let shown = a.str("out")?.to_owned();
        // Resolved before reading, so a path outside the root costs nothing.
        let path = self.resolve(&shown, true)?;
        // Markdown's images go in a folder beside the file: `book.md` → `book_diagrams/`.
        let out = std::path::Path::new(&shown);
        let stem = out.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "document".into());
        let image_dir = format!("{stem}_diagrams");
        let folder = if format == ChessFormat::Markdown && diagrams == DiagramMode::Svg {
            Some(self.resolve(&out.with_file_name(&image_dir).to_string_lossy(), true)?)
        } else {
            None
        };
        let id = self.doc(a)?.id;
        let title = match a.opt_str("title")? {
            Some(t) => t.to_owned(),
            None => self.doc(a)?.name.trim_end_matches(".pdf").to_owned(),
        };
        // HTML and EPUB show figurines with the user's font when there is one.
        let fonts = pdfcraft_engine::chess::pdfcraft_chessvision::text_font::find_fonts();
        let options = ChessExportOptions {
            pages,
            scan,
            lang,
            ocr,
            diagrams,
            figurines,
            links: a.opt_bool("links")?.unwrap_or(true),
            link_text,
            pieces: std::sync::Arc::new(pieces),
            text_font: fonts.figurines.clone().map(std::sync::Arc::new),
            sign_font: fonts.signs.clone().map(std::sync::Arc::new),
            title,
            diagram_px,
            image_dir: image_dir.clone(),
            book,
        };
        let made = self.session.chess_export(id, format, &options).map_err(failed)?;
        if let Some(dir) = &folder
            && !made.images.is_empty()
        {
            std::fs::create_dir_all(dir).map_err(|e| failed(format!("{image_dir}: {e}")))?;
            for (rel, png) in &made.images {
                // `rel` is `<image_dir>/<name>`, a name made here: only its file name is used.
                let name = std::path::Path::new(rel).file_name().ok_or_else(|| failed("an image has no file name"))?;
                crate::write_atomic(&dir.join(name), png)?;
            }
        }
        crate::write_atomic(&path, &made.bytes)?;
        let (all, positioned) = made.diagram_counts();
        let read = &made.read;
        let mut notes: Vec<Value> = read.notes.iter().map(|(p, n)| json!({ "page": p + 1, "note": n })).collect();
        notes.extend(made.notes.iter().map(|n| json!({ "note": n })));
        Ok(json!({
            "out": shown,
            "bytes": made.bytes.len(),
            "pages": read.doc.pages.len(),
            "lang": read.lang.lang.code(),
            "diagrams": all,
            "diagrams_drawn": made.drawn,
            "diagrams_unread": all - positioned,
            "games": read.doc.games.len(),
            "text_font": font_json(fonts.figurines.as_ref(), matches!(format, ChessFormat::Html | ChessFormat::Epub), &fonts.errors),
            "sign_font": font_json(fonts.signs.as_ref(), matches!(format, ChessFormat::Html | ChessFormat::Epub), &[]),
            "pieces": match source {
                PieceSource::Font { family, .. } => json!({ "source": "font", "family": family, "error": Value::Null }),
                PieceSource::Images { dir } => json!({ "source": "images", "family": dir.to_string_lossy(), "error": Value::Null }),
                PieceSource::Bundled { id } => json!({ "source": "bundled", "family": id, "error": Value::Null }),
                PieceSource::Geometric => json!({ "source": "geometric", "family": Value::Null, "error": error.map(|e| e.to_string()) }),
            },
            "needs_ocr": read.needs_ocr.iter().map(|(p, why)| json!({ "page": p + 1, "reason": why })).collect::<Vec<_>>(),
            "ocr_pages": read.ocr_pages.iter().map(|o| json!({ "page": o.page + 1, "lines": o.lines, "repaired": o.edits, "unsettled": o.flags, "general_ocr": o.general_ocr })).collect::<Vec<_>>(),
            "notes": notes,
            "position_games": made.position_games,
            "cipher": Self::cipher_reply(&made.read.cipher, cipher_out.as_ref())?,
            "image_dir": folder.as_ref().filter(|_| !made.images.is_empty()).map(|_| image_dir),
        }))
    }

    /// `cipher_in`: a book cipher saved by an earlier run (JSON), to start from; `cipher_out`:
    /// where to save the cipher these pages taught. Both confined to the root.
    fn cipher_args(&self, a: &Args) -> Result<(Option<BookCipher>, Option<CipherOut>)> {
        let seed = match a.opt_str("cipher_in")? {
            None => None,
            Some(p) => {
                let path = self.resolve(p, false)?;
                let text = std::fs::read_to_string(&path).map_err(|e| failed(format!("{p}: {e}")))?;
                Some(BookCipher::from_json(&text).map_err(|e| failed(format!("{p}: {e}")))?)
            }
        };
        let out = a.opt_str("cipher_out")?.map(|p| self.resolve(p, true).map(|r| (p.to_owned(), r))).transpose()?;
        Ok((seed, out))
    }

    /// Save `cipher` to `out` (when asked) and say what it holds.
    fn cipher_reply(cipher: &BookCipher, out: Option<&CipherOut>) -> Result<Value> {
        if let Some((_, path)) = out {
            let text = cipher.to_json().map_err(|e| failed(format!("the cipher could not be written: {e}")))?;
            crate::write_atomic(path, text.as_bytes())?;
        }
        Ok(json!({ "rows": cipher.entries().len(), "settled": cipher.proven().len(), "saved": out.map(|(shown, _)| shown.clone()) }))
    }

    pub(crate) fn chess_game(&self, a: &Args) -> Result<Value> {
        use pdfcraft_engine::chess::pdfcraft_chess::game::{Game, Outcome};
        let (fen, pgn) = (a.opt_str("fen")?, a.opt_str("pgn")?);
        let (mut game, mut stopped) = match (fen, pgn) {
            (Some(_), Some(_)) => return Err(ToolError::InvalidArgs("give fen or pgn, not both".into())),
            (Some(fen), None) => (Game::from_fen(fen).map_err(|e| ToolError::InvalidArgs(format!("fen: {e}")))?, None),
            (None, Some(pgn)) => Game::from_pgn(pgn).map_err(|e| ToolError::InvalidArgs(format!("pgn: {e}")))?,
            (None, None) => (Game::standard(), None),
        };
        let moves = if a.get("moves").is_some() { a.strs("moves")? } else { Vec::new() };
        if moves.len() > 2000 {
            return Err(Args::wrong("moves", "at most 2000 moves"));
        }
        game.go_to(usize::MAX);
        for mv in moves {
            if stopped.is_some() {
                break;
            }
            // UCI first: `e2e4` is not SAN, and no SAN reads as UCI.
            if game.play_uci(mv).is_err() && game.play_san(mv).is_err() {
                stopped = Some(mv.to_owned());
            }
        }
        // The main line's length, then the move asked for along it.
        game.go_to_node(0);
        let plies = game.len();
        let ply = match a.opt_int("ply")? {
            Some(p) => usize::try_from(p).map_err(|_| Args::wrong("ply", "0 or more"))?,
            None => plies,
        };
        game.go_to(ply);
        let outcome = match game.outcome() {
            Outcome::Ongoing => "ongoing",
            Outcome::Checkmate => "checkmate",
            Outcome::Stalemate => "stalemate",
            Outcome::FiftyMoves => "fifty_moves",
        };
        Ok(json!({
            "fen": game.fen(),
            "start_fen": game.start_fen(),
            "side_to_move": if game.side_to_move() == pdfcraft_engine::chess::pdfcraft_chess::placement::Color::White { "white" } else { "black" },
            "in_check": game.in_check(),
            "outcome": outcome,
            "legal_moves": game.legal_moves(),
            "ply": game.ply(),
            "plies": plies,
            "movetext": game.movetext(),
            "pgn": game.pgn(),
            "stopped": stopped,
        }))
    }

    /// The piece sets that can be chosen (Preferences ▸ Chess pieces, plan HC2).
    pub(crate) fn chess_pieces(&self) -> Result<Value> {
        use pieces::choice::{self, CandidateKind, PieceChoice};
        let sets: Vec<Value> = choice::candidates()
            .into_iter()
            .map(|c| {
                let kind = match c.kind {
                    CandidateKind::Auto => "auto",
                    CandidateKind::Shipped => "shipped",
                    CandidateKind::Font => "font",
                    CandidateKind::Folder => "folder",
                    CandidateKind::Geometric => "geometric",
                };
                // What the chess tools' `pieces` takes for this set, when they take it.
                let argument = match &c.choice {
                    PieceChoice::Auto => Some("installed".to_owned()),
                    PieceChoice::Geometric => Some("geometric".to_owned()),
                    PieceChoice::Bundled(id) => Some(id.clone()),
                    PieceChoice::Font(_) | PieceChoice::Folder(_) => None,
                };
                json!({
                    "token": c.choice.token(),
                    "kind": kind,
                    "name": c.name,
                    "author": c.author,
                    "licence": c.licence,
                    "path": c.path.map(|p| p.to_string_lossy().into_owned()),
                    "missing": c.missing,
                    "pieces": argument,
                })
            })
            .collect();
        Ok(json!({ "sets": sets, "environment": choice::environment_overrides() }))
    }

    /// Import piece sets from a folder or a saved web page into a pieces folder (plan HC3).
    pub(crate) fn chess_pieces_import(&self, a: &Args) -> Result<Value> {
        use pieces::import::{self, ImportError};
        let from = self.resolve(a.str("from")?, false)?;
        let into = match a.opt_str("into")? {
            Some(p) => self.resolve(p, true)?,
            None if self.root.is_some() => return Err(failed("into is needed when --root is set")),
            None => pieces::pieces_folder().ok_or_else(|| failed("no pieces folder beside the executable"))?,
        };
        let replace = a.opt_bool("replace")?.unwrap_or(false);
        match import::import(&from, &into, replace) {
            Ok(sets) => {
                let sets: Vec<Value> = sets
                    .into_iter()
                    .map(
                        |s| json!({ "name": s.name, "folder": s.dir.to_string_lossy(), "token": pieces::choice::PieceChoice::Folder(s.dir).token() }),
                    )
                    .collect();
                Ok(json!({ "imported": sets, "licence": "unknown: for personal use only" }))
            }
            Err(ImportError::Exists(dirs)) => Ok(json!({
                "imported": [],
                "exists": dirs.iter().map(|d| d.to_string_lossy().into_owned()).collect::<Vec<_>>(),
                "refused": "a set of that name is already there; pass replace to replace it",
            })),
            Err(e) => Err(failed(e.to_string())),
        }
    }

    pub(crate) fn chess_status(&self, a: &Args) -> Result<Value> {
        let dir = self.models_dir(a)?;
        let models = chess::models(dir.as_deref());
        let (_, source, error) = pieces::find();
        let mut piece_set = match source {
            PieceSource::Font { path, family } => {
                json!({ "source": "font", "family": family, "path": path.map(|p| p.to_string_lossy().into_owned()) })
            }
            PieceSource::Images { dir } => json!({ "source": "images", "family": Value::Null, "path": dir.to_string_lossy() }),
            PieceSource::Bundled { id } => json!({ "source": "bundled", "family": id, "path": Value::Null }),
            PieceSource::Geometric => json!({ "source": "geometric", "family": Value::Null, "path": Value::Null }),
        };
        if let Some(o) = piece_set.as_object_mut() {
            o.insert("error".into(), error.map(|e| e.to_string()).into());
        }
        Ok(json!({
            "pieces": model_json(&models.pieces),
            "glyphs": model_json(&models.glyphs),
            "folder": models.folder.as_ref().map(|f| f.to_string_lossy().into_owned()),
            "looked_in": if dir.is_some() { "models_dir" } else { "PDFCRAFT_MODELS, then beside the executable" },
            "reads_fonts_without_model": true,
            "piece_set": piece_set,
        }))
    }
}

/// What an export says about one of the user's fonts (`chessvision::text_font`).
fn font_json(
    font: Option<&pdfcraft_engine::chess::pdfcraft_chessvision::text_font::TextFont>,
    exported: bool,
    errors: &[pdfcraft_engine::chess::pdfcraft_chessvision::text_font::TextFontError],
) -> Value {
    json!({
        "path": font.and_then(|f| f.path.as_ref()).map(|p| p.to_string_lossy().into_owned()),
        "family": font.map(|f| f.family.clone()),
        "role": font.map(|f| format!("{:?}", f.role).to_lowercase()),
        "format": font.map(|f| f.format.extension()),
        "used": font.is_some() && exported,
        "error": errors.first().map(|e| e.to_string()),
    })
}
