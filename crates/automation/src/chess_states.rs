//! Chess ▸ Diagrams ▸ the counts the panel's chips show, without the interface (plan A8, P2a2):
//! `chess_diagram_states`.
//!
//! The diagrams are found and read as `chess_read` does, the corrections `store` keeps for the
//! document are put in place (as the panel and every export do), and each diagram is counted by
//! its status, [`Status::of`]: the rule the panel's chips and icons use. With the book's PGN
//! (`reference_pgn`, plan "Carregar PGN do livro", R3), a diagram read whose position the PGN has
//! is counted as confirmed by it ([`Status::with_reference`]).

use std::collections::HashMap;

use pdfcraft_engine::chess::ChessPage;
use pdfcraft_engine::chess::corrections::{Kind, Status};
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::reference::ReferenceIndex;
use serde_json::{Map, Value, json};

use crate::{Args, Automation, Result, failed};

/// `counts` as `{ "read": 3, "review": 1, … }`, every status present.
fn counts_json(counts: &HashMap<Status, usize>) -> Value {
    let map: Map<String, Value> = Status::ALL.iter().map(|s| (s.name().to_owned(), json!(counts.get(s).copied().unwrap_or(0)))).collect();
    Value::Object(map)
}

/// How many of `page`'s diagrams have each status, given what the user did with each (`kinds`,
/// by page and index) and the book's PGN (`reference`).
fn page_counts(page: &ChessPage, kinds: &HashMap<(usize, usize), Kind>, reference: Option<&ReferenceIndex>) -> HashMap<Status, usize> {
    let mut counts = HashMap::new();
    for index in 0..page.diagrams.len() {
        let reading = page.readings.as_ref().and_then(|r| r.iter().find(|r| r.index == index));
        let status = Status::with_reference(reading, kinds.get(&(page.page, index)).copied(), reference);
        let n = counts.entry(status).or_insert(0usize);
        *n = n.saturating_add(1);
    }
    counts
}

impl Automation {
    pub(crate) fn chess_diagram_states(&mut self, a: &Args) -> Result<Value> {
        let settings = self.chess_settings(a)?;
        let pages = if a.opt_ints("pages")?.is_some() { self.pages(a, "pages")? } else { Vec::new() };
        // Resolved and indexed before the diagrams are read, so a PGN that cannot be used costs nothing.
        let reference = match a.opt_str("reference_pgn")? {
            None => None,
            Some(p) => Some((p, crate::chess::read_reference(&self.resolve(p, false)?, p)?)),
        };
        let id = self.doc(a)?.id;
        let corrections = self.use_store_corrections(a, id)?;
        let corrected = self.session.chess_scan_corrected(id, &pages, &settings).map_err(failed)?;
        let kinds: HashMap<(usize, usize), Kind> = corrected.applied.iter().map(|c| ((c.page, c.index), c.kind)).collect();
        let mut total: HashMap<Status, usize> = HashMap::new();
        let mut per_page = Vec::new();
        for page in corrected.pages.iter().filter(|p| !p.diagrams.is_empty()) {
            let counts = page_counts(page, &kinds, reference.as_ref().map(|(_, index)| index));
            for (status, n) in &counts {
                let t = total.entry(*status).or_insert(0);
                *t = t.saturating_add(*n);
            }
            per_page.push(json!({ "page": page.page.saturating_add(1), "diagrams": page.diagrams.len(), "states": counts_json(&counts) }));
        }
        let diagrams: usize = total.values().sum();
        let to_review: usize = total.iter().filter(|(s, _)| s.to_review()).map(|(_, n)| *n).sum();
        let generation = self.doc(a)?.edit_generation();
        Ok(json!({
            "generation": generation,
            "diagrams": diagrams,
            "to_review": to_review,
            "states": counts_json(&total),
            "pages": per_page,
            "corrections": corrections,
            "reference": reference.map(|(p, index)| json!({ "pgn": p, "games": index.games().len(), "moves": index.moves() })),
        }))
    }
}
