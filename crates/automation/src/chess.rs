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
use pdfcraft_engine::chess::pdfcraft_chess::draw::{BoardStyle, PieceSet, Rgb};
use pdfcraft_engine::chess::pdfcraft_chess::fen;
use pdfcraft_engine::chess::pdfcraft_chess::rules::Legality;
use pdfcraft_engine::chess::pdfcraft_chessvision::board::{BoardReading, ReadPosition, ReadingSource, Unreadable};
use pdfcraft_engine::chess::pdfcraft_chessvision::coords::{PageGeometry, PageRect};
use pdfcraft_engine::chess::pdfcraft_chessvision::detect::Source;
use pdfcraft_engine::chess::pdfcraft_chessvision::merge::Merged;
use pdfcraft_engine::chess::pdfcraft_chessvision::pieces::{self, PieceSource};
use pdfcraft_engine::chess::{self, ChessPage, ChessScanSettings, DiagramReplacement, ModelState, ReadingOrder};
use serde_json::{Value, json};

use crate::{Args, Automation, Result, ToolError, comments, failed};

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
    fn models_dir(&self, a: &Args) -> Result<Option<std::path::PathBuf>> {
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
            Some(_) => return Err(Args::wrong("pieces", "\"installed\" or \"geometric\"")),
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
                PieceSource::Geometric => json!({ "source": "geometric", "family": Value::Null, "error": error.map(|e| e.to_string()) }),
            },
            "notes": plan.notes,
        });
        if let (Some(o), Value::Object(f)) = (out.as_object_mut(), fields) {
            o.extend(f);
        }
        Ok(out)
    }

    pub(crate) fn chess_status(&self, a: &Args) -> Result<Value> {
        let dir = self.models_dir(a)?;
        let models = chess::models(dir.as_deref());
        let (_, source, error) = pieces::find();
        let mut piece_set = match source {
            PieceSource::Font { path, family } => {
                json!({ "source": "font", "family": family, "path": path.map(|p| p.to_string_lossy().into_owned()) })
            }
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
