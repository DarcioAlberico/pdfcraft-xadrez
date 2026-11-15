//! Chess: `chess_engine_review`, the book says × the engine says (plan A9-N17a, finding A8-10),
//! without the interface. It reads the pages as `chess_document` does, links them to the book's PGN
//! when there is one, and holds the signs and the captions' goals to the evaluations the app's
//! engine kept for the document (`chess_evals`' store) and to the mate solver's word, by the
//! caller's limits ([`pdfcraft_engine::chess::review`]). No tool runs an engine: the evaluations
//! are only read, and a position with none is said, not searched.

use pdfcraft_engine::chess::evals::EvalFile;
use pdfcraft_engine::chess::pdfcraft_chess::judge::{Band, Limits, Value as Score};
use pdfcraft_engine::chess::pdfcraft_chess::pgn;
use pdfcraft_engine::chess::pdfcraft_chess::san::Lang;
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::PageLabel;
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::reference::{self, ReferenceIndex};
use pdfcraft_engine::chess::review::{self, Computed, Divergence, Found, Place, Review, Said};
use serde_json::{Value, json};

use crate::chess_store_migrate::{EVALS, StoreAt};
use crate::{Args, Automation, Result, failed};

#[cfg(not(target_arch = "wasm32"))]
type FolderStore = pdfcraft_engine::chess::evals::DirStore;
#[cfg(target_arch = "wasm32")]
type FolderStore = pdfcraft_engine::chess::evals::MemStore;

/// The evaluations kept for a document, for a review.
pub(crate) struct KeptEvals {
    pub file: EvalFile,
    /// What the store said of them (`file`, `positions`, the store's notes), or why none were read.
    pub reply: Value,
}

/// `limits`: the judge's limits (decision D29's by default), each one the caller may change.
pub(crate) fn limits_arg(a: &Args) -> Result<Limits> {
    let mut limits = Limits::default();
    let Some(given) = a.get("limits") else { return Ok(limits) };
    let Some(o) = given.as_object() else { return Err(Args::wrong("limits", "an object")) };
    let int = |name: &str, max: i64| -> Result<Option<i64>> {
        match o.get(name) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v.as_i64().filter(|n| (0..=max).contains(n)).map(Some).ok_or_else(|| Args::wrong("limits", "whole numbers from 0")),
        }
    };
    const MAX_CP: i64 = 100_000;
    if let Some(n) = int("refuted", MAX_CP)? {
        limits.refuted = i32::try_from(n).unwrap_or(i32::MAX);
    }
    if let Some(n) = int("questioned", MAX_CP)? {
        limits.questioned = i32::try_from(n).unwrap_or(i32::MAX);
    }
    if let Some(n) = int("min_depth", 999)? {
        limits.min_depth = u32::try_from(n).unwrap_or(u32::MAX);
    }
    match o.get("bands") {
        None | Some(Value::Null) => {}
        Some(Value::Array(edges)) if edges.len() == 3 => {
            for (slot, edge) in limits.bands.iter_mut().zip(edges) {
                let n =
                    edge.as_i64().filter(|n| (0..=MAX_CP).contains(n)).ok_or_else(|| Args::wrong("limits", "bands of three whole numbers from 0"))?;
                *slot = i32::try_from(n).unwrap_or(i32::MAX);
            }
        }
        Some(_) => return Err(Args::wrong("limits", "bands of three whole numbers from 0")),
    }
    limits.check().map_err(|_| Args::wrong("limits", "bands whose edges ascend"))?;
    Ok(limits)
}

/// The limits as a reply says them.
fn limits_reply(l: &Limits) -> Value {
    json!({ "refuted": l.refuted, "questioned": l.questioned, "bands": l.bands, "min_depth": l.min_depth })
}

/// A value for White as a reply says it: `cp`, or `mate` (negative: Black mates).
fn score_reply(v: Score) -> Value {
    match v {
        Score::Cp(cp) => json!({ "cp": cp }),
        Score::Mates(n) => json!({ "mate": n }),
        Score::Mated(n) => json!({ "mate": -i64::from(n) }),
    }
}

fn band_reply(b: Band) -> Value {
    json!({ "band": b.name(), "sign": b.sign() })
}

/// One divergence for a reply.
fn divergence_reply(d: &Divergence, labels: &[String]) -> Value {
    let mut out = json!({
        "page": u64::from(d.page).saturating_add(1),
        "page_label": PageLabel::of(labels, d.page).to_string(),
        "printed": d.printed,
        "finding": d.found.name(),
        "source": d.source.name(),
        "book": d.book_words(),
        "computed": d.computed_words(),
    });
    match d.at {
        Place::Move(at) => out["move"] = json!({ "block": at.block, "inline": at.inline }),
        Place::Diagram(at) => out["diagram"] = json!(at.index.saturating_add(1)),
    }
    match d.book {
        Said::Sign(nag) => out["nag"] = json!(nag),
        Said::Goal { .. } => {}
    }
    match d.found {
        Found::Refuted { loss } => out["loss"] = json!(loss),
        Found::Questioned { loss } => out["loss"] = json!(loss),
        Found::Contradicted { band } | Found::Goal { band } => out["found"] = band_reply(band),
        Found::OtherSolution | Found::NoMate | Found::ManyKeys => {}
    }
    match &d.computed {
        Computed::Engine(e) => {
            out["eval"] = score_reply(e.value);
            out["depth"] = json!(e.depth);
            out["engine"] = json!(e.engine);
            if let Some((san, v)) = &e.best {
                out["best"] = json!({ "move": san, "eval": score_reply(*v) });
            }
            if let Some((san, v)) = &e.second {
                out["second"] = json!({ "move": san, "eval": score_reply(*v) });
            }
        }
        Computed::Solver(s) => {
            out["solver"] = json!({ "n": s.n, "best_try": s.best_try, "refutation": s.refutation, "keys": s.keys });
        }
    }
    if d.side_assumed {
        out["side_assumed"] = json!(true);
    }
    out
}

/// What `chess_engine_review` writes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Csv,
    Markdown,
}

impl Automation {
    /// The evaluations kept for document `id` in `evals_store` (the app's by default; outside the
    /// root nothing is read, and the reply says so).
    pub(crate) fn kept_evals(&self, a: &Args, id: pdfcraft_engine::DocId) -> Result<KeptEvals> {
        let doc = self.session.get(id).ok_or_else(|| failed("no such document"))?;
        let empty = EvalFile::empty(pdfcraft_engine::chess::corrections::DocRecord::default());
        let dir = match self.chess_store_at(a, EVALS, false)? {
            StoreAt::Folder(dir) => dir,
            StoreAt::OutsideRoot => {
                let why =
                    "the evaluations are kept in the app's data folder, outside the folders the tools are confined to: give evals_store to read them";
                return Ok(KeptEvals { file: empty, reply: json!({ "positions": 0, "outside_root": true, "why": why }) });
            }
        };
        #[cfg(not(target_arch = "wasm32"))]
        let store = FolderStore::new(dir);
        #[cfg(target_arch = "wasm32")]
        let store = {
            let _ = dir;
            FolderStore::new(0)
        };
        let loaded = store.load(&doc.doc_keys(), doc.info.pages.len()).map_err(failed)?;
        let reply = crate::chess_corrections::with_store_notes(json!({ "file": loaded.name, "positions": loaded.file.evals.len() }), &loaded);
        Ok(KeptEvals { file: loaded.file, reply })
    }

    /// `chess_engine_review`: the divergences between the book and the computation.
    pub(crate) fn chess_engine_review(&mut self, a: &Args) -> Result<Value> {
        let settings = self.chess_settings(a)?;
        let pages = if a.opt_ints("pages")?.is_some() { self.pages(a, "pages")? } else { Vec::new() };
        let lang = match a.opt_str("lang")? {
            None => None,
            Some(tag) => Some(Lang::from_tag(tag).ok_or_else(|| Args::wrong("lang", "a piece-letter language such as en, de, pt or fr"))?),
        };
        let format = match a.opt_str("format")? {
            None | Some("csv") => Format::Csv,
            Some("markdown") => Format::Markdown,
            Some(_) => return Err(Args::wrong("format", "\"csv\" or \"markdown\"")),
        };
        let limits = limits_arg(a)?;
        // Resolved before reading, so a path outside the root costs nothing.
        let out = a.opt_str("out")?.map(|p| self.resolve(p, true).map(|r| (p.to_owned(), r))).transpose()?;
        let id = self.doc(a)?.id;
        let (reference, reference_kept) = self.book_pgn(a, id)?;
        let (seed, _) = self.cipher_args(a)?;
        if let Some((_, path)) = &out {
            let inputs = self.input_paths(a)?;
            self.check_outputs(a, &[("out", path.as_path())], &inputs.iter().map(|p| p.as_path()).collect::<Vec<_>>())?;
        }
        let title = match a.opt_str("title")? {
            Some(t) => t.to_owned(),
            None => self.doc(a)?.name.trim_end_matches(".pdf").to_owned(),
        };
        let evals = self.kept_evals(a, id)?;
        let ocr = crate::chess::ocr_options(a)?.with_book(seed.as_ref());
        let loaded = self.use_store_corrections(a, id)?;
        let mut read = self.session.chess_document_with(id, &pages, &settings, lang, &ocr).map_err(failed)?;
        let mut reference_reply = Value::Null;
        let refs: Vec<&ReferenceIndex> = reference.iter().map(|r| &*r.index).collect();
        if let Some(r) = &reference {
            let linked = reference::link(&mut read.doc, &r.index);
            reference_reply = crate::chess::reference_reply(&r.shown, &r.index, &linked);
        }
        let review = review::divergences(&read.doc, &refs, &evals.file, &limits);
        // As the book numbers its pages: its /PageLabels, else the numbers it prints.
        let labels: Vec<String> = self.doc(a)?.info.pages.iter().map(|p| p.label.clone()).collect();
        let labels = PageLabel::with_folios(&labels, &read.doc);
        let mut result = review_reply(&review, &labels, &limits);
        if let Some(o) = result.as_object_mut() {
            o.insert("evals".into(), evals.reply);
            o.insert("pages".into(), json!(read.doc.pages.len()));
            o.insert("needs_ocr".into(), json!(crate::chess::needs_ocr_reply(&read.needs_ocr)));
            o.insert("notes".into(), json!(crate::chess::notes_reply(&read.notes)));
            o.insert("corrections".into(), crate::chess_corrections::corrections_reply(loaded, &read));
            o.insert("reference".into(), reference_reply);
            o.insert("reference_kept".into(), reference_kept);
        }
        if let Some((shown, path)) = out {
            let text = match format {
                Format::Csv => review::to_csv(&review, &labels),
                Format::Markdown => review::to_markdown(&review, &labels, &title),
            };
            let bom = format == Format::Csv && a.opt_bool("bom")?.unwrap_or(false);
            let bytes = if bom { [pgn::BOM, text.as_bytes()].concat() } else { text.into_bytes() };
            crate::write_atomic(&path, &bytes)?;
            result["format"] = json!(if format == Format::Csv { "csv" } else { "markdown" });
            result["out"] = json!(shown);
            result["bytes"] = json!(bytes.len());
        }
        Ok(result)
    }
}

/// The report for a reply: the divergences, their counts by finding and by source, and what was
/// not judged.
fn review_reply(review: &Review, labels: &[String], limits: &Limits) -> Value {
    let findings: serde_json::Map<String, Value> =
        Found::NAMES.iter().map(|n| ((*n).to_owned(), json!(review.divergences.iter().filter(|d| d.found.name() == *n).count()))).collect();
    let sources: serde_json::Map<String, Value> =
        ["solver", "engine"].iter().map(|n| ((*n).to_owned(), json!(review.divergences.iter().filter(|d| d.source.name() == *n).count()))).collect();
    json!({
        "divergences": review.divergences.iter().map(|d| divergence_reply(d, labels)).collect::<Vec<_>>(),
        "count": review.divergences.len(),
        "findings": findings,
        "sources": sources,
        "judged": review.judged,
        "not_judged": { "no_eval": review.no_eval, "shallow": review.shallow },
        "over": review.over,
        "limits": limits_reply(limits),
    })
}
