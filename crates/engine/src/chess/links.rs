//! Chess ▸ Add lichess links (blueprint S11): under each diagram whose position is known, a
//! short label that opens the position on lichess.org.
//!
//! [`Session::chess_link_plan`] scans the pages (with readings), keeps the diagrams that may be
//! linked and places a label for each, and returns one undoable [`Edit::Batch`]: per diagram an
//! added text (`Edit::AddText`, so it stays upright on a turned page and can be moved or edited
//! later like any added text) and a link over it (`Edit::AddLinks`, a URI action; nothing is
//! fetched — PdfCraft makes no network request, the reader's viewer opens it when clicked).
//!
//! **Which diagrams.** Only a position that is legal with the side to move it was given and that
//! the decoder did not repair gets a link ([`super::lichess_url`]); by default it must also pass
//! the reading gate (every square read with confidence), so a doubtful reading is not published
//! as the book's position. A diagram that already has a link to the same address on its page is
//! skipped, so running it twice adds nothing. Every skipped diagram is reported with the reason.
//!
//! **Where.** Below the diagram, [`GAP`] points under it, left-aligned with it. When that would
//! leave the displayed page (inside [`MARGIN`]), to the right of the diagram, at its foot (or as
//! low beside it as the page allows);
//! failing that, above it. A label that fits nowhere is skipped. Labels of one page are also kept
//! from overlapping each other and the page's other diagrams.
//!
//! The label is Helvetica (WinAnsi), so it says "Analyze on lichess" in plain letters, never an
//! arrow or a figurine the font lacks.

use pdfcraft_annot::links::LinkStyle;
use pdfcraft_chessvision::coords::{PageGeometry, ViewRect};
use pdfcraft_edit::AddedText;

use super::{ChessScanSettings, lichess_url};
use crate::{DocId, Edit, Session};

/// The undo label of a run (listed in [`crate::BATCH_LABELS`]).
pub const LABEL: &str = "Add lichess links";
/// The label's default text.
pub const DEFAULT_TEXT: &str = "Analyze on lichess";
/// The label's default size, in points.
pub const DEFAULT_SIZE: f64 = 8.0;
/// Space between the diagram and its label, in points.
pub const GAP: f32 = 3.0;
/// A label stays this far inside the displayed page's edges, in points.
pub const MARGIN: f32 = 6.0;
/// The longest label text accepted.
pub const MAX_TEXT_CHARS: usize = 80;
/// The label's line height, as a share of its size.
const LINE: f32 = 1.25;
/// The label's colour: link blue.
const BLUE: [f64; 3] = [0.0, 0.2, 0.8];

/// How the labels look and which readings get one.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkSettings {
    pub text: String,
    pub size: f64,
    /// Also link readings that are legal but did not pass the reading gate (a square read with
    /// little confidence). Repaired readings are never linked.
    pub allow_unaccepted: bool,
}

impl Default for LinkSettings {
    fn default() -> Self {
        LinkSettings { text: DEFAULT_TEXT.into(), size: DEFAULT_SIZE, allow_unaccepted: false }
    }
}

/// Where a label goes relative to its diagram.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Below,
    Right,
    Above,
}

/// One label to add.
#[derive(Clone, Debug, PartialEq)]
pub struct PlannedLink {
    /// Page (0-based) and the diagram's index in the page's reading order (0-based).
    pub page: usize,
    pub index: usize,
    pub fen: String,
    pub url: String,
    pub side: Side,
    /// The label's box in view space (points from the displayed page's top-left, y down).
    pub label_view: ViewRect,
    /// The link's box in user space.
    pub link_user: [f64; 4],
}

/// A diagram that gets no label, and why.
#[derive(Clone, Debug, PartialEq)]
pub struct SkippedDiagram {
    pub page: usize,
    pub index: usize,
    pub reason: String,
}

/// What [`Session::chess_link_plan`] would do.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkPlan {
    pub links: Vec<PlannedLink>,
    pub skipped: Vec<SkippedDiagram>,
    /// The undoable edit (a [`Edit::Batch`] labelled [`LABEL`]); `None` when there is nothing to add.
    pub edit: Option<Edit>,
}

fn overlaps(a: &ViewRect, b: &ViewRect) -> bool {
    a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1
}

/// The label's box for a diagram at `diagram` (view space) on a displayed page of `page_w` ×
/// `page_h` points, `w` × `h` points large, clear of `taken`: below, else right, else above.
pub fn place_label(diagram: ViewRect, (page_w, page_h): (f32, f32), (w, h): (f32, f32), taken: &[ViewRect]) -> Option<(Side, ViewRect)> {
    if ![diagram.x0, diagram.y0, diagram.x1, diagram.y1, page_w, page_h, w, h].iter().all(|v| v.is_finite()) || w <= 0.0 || h <= 0.0 {
        return None;
    }
    let (left, top, right, bottom) = (MARGIN, MARGIN, page_w - MARGIN, page_h - MARGIN);
    // Left-aligned with the diagram, but kept on the page when the label is wider than what is
    // left to its right.
    let x = diagram.x0.min(right - w).max(left);
    // Beside the board at its foot, or as low as the page allows while still beside it.
    let foot = diagram.y1.min(bottom);
    let tries = [
        (Side::Below, ViewRect::new(x, diagram.y1 + GAP, x + w, diagram.y1 + GAP + h)),
        (Side::Right, ViewRect { x0: diagram.x1 + GAP, y0: foot - h, x1: diagram.x1 + GAP + w, y1: foot }),
        (Side::Above, ViewRect::new(x, diagram.y0 - GAP - h, x + w, diagram.y0 - GAP)),
    ];
    tries.into_iter().find(|(side, r)| {
        let beside = *side != Side::Right || r.y0 >= diagram.y0;
        beside && r.x0 >= left && r.y0 >= top && r.x1 <= right && r.y1 <= bottom && !taken.iter().any(|t| overlaps(r, t))
    })
}

/// The label's text, checked: one line of at most [`MAX_TEXT_CHARS`] characters Helvetica can
/// draw (WinAnsi).
pub fn check_label(text: &str) -> Result<&str, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("the label is empty".into());
    }
    if text.chars().count() > MAX_TEXT_CHARS {
        return Err(format!("the label is longer than {MAX_TEXT_CHARS} characters"));
    }
    if text.chars().any(char::is_control) {
        return Err("the label must be one line".into());
    }
    if let Some(c) = text.chars().find(|c| pdfcraft_fonts::win_ansi_byte(*c).is_none()) {
        return Err(format!("the label's font (Helvetica) cannot draw {c:?}"));
    }
    Ok(text)
}

fn view_to_display(r: ViewRect, page_h: f32) -> [f64; 4] {
    [f64::from(r.x0), f64::from(page_h - r.y1), f64::from(r.x1), f64::from(page_h - r.y0)]
}

fn user_box(geometry: &PageGeometry, r: ViewRect) -> Option<[f64; 4]> {
    let u = geometry.view_to_user(r)?;
    Some([u.x0, u.y0, u.x1, u.y1].map(f64::from))
}

impl Session {
    /// Plan Chess ▸ Add lichess links on `pages` (0-based; empty = all) of document `id`: which
    /// diagrams get a label, where, and the edit that adds them. Changes nothing. Fails when
    /// the document does not exist, is not editable or may not be changed, a page is out of
    /// range, or the label cannot be drawn.
    pub fn chess_link_plan(&mut self, id: DocId, pages: &[usize], scan: &ChessScanSettings, settings: &LinkSettings) -> Result<LinkPlan, String> {
        let text = check_label(&settings.text)?.to_owned();
        if !(settings.size.is_finite() && (4.0..=48.0).contains(&settings.size)) {
            return Err("the label size must be from 4 to 48 points".into());
        }
        {
            let doc = self.get(id).ok_or("no such document")?;
            let editor = doc.editor.as_ref().ok_or_else(|| doc.read_only_reason.clone().unwrap_or_else(|| "the document can't be edited".into()))?;
            if editor.cos.permissions().is_some_and(|p| !p.modify()) {
                return Err("the document's permissions do not allow changes".into());
            }
        }
        let scanned = self.chess_scan(id, pages, scan, true)?;
        let doc = self.get(id).ok_or("no such document")?;
        let existing = doc.links();
        let size = settings.size as f32;
        let (w, h) = ((pdfcraft_fonts::helvetica_width(&text, settings.size) as f32 + 2.0), size * LINE);
        let mut links = Vec::new();
        let mut skipped = Vec::new();
        for page in &scanned {
            let mut skip = |index: usize, reason: &str| skipped.push(SkippedDiagram { page: page.page, index, reason: reason.into() });
            let Some(geometry) = page.geometry else {
                for i in 0..page.diagrams.len() {
                    skip(i, "the page has no visible area");
                }
                continue;
            };
            let view = geometry.view_size();
            let readings = page.readings.as_deref().unwrap_or_default();
            // Labels go clear of every diagram on the page, and of the labels placed before.
            let mut taken: Vec<ViewRect> = page.diagrams.iter().filter_map(|d| geometry.user_to_view(d.candidate.bbox_user)).collect();
            for (index, _) in page.diagrams.iter().enumerate() {
                let Some(reading) = readings.get(index) else {
                    skip(index, "the diagram was not read");
                    continue;
                };
                let position = match &reading.outcome {
                    Ok(p) => p,
                    Err(e) => {
                        skip(index, &format!("the diagram could not be read: {e}"));
                        continue;
                    }
                };
                let Some(url) = lichess_url(position) else {
                    skip(index, if position.repaired() { "the reading was repaired, so it needs review first" } else { "the position is not legal" });
                    continue;
                };
                if !position.accepted && !settings.allow_unaccepted {
                    skip(index, "the reading did not pass the gate (a square read with little confidence); review it first");
                    continue;
                }
                if existing.iter().any(|l| l.page == page.page && matches!(&l.action, pdfcraft_annot::links::LinkAction::Uri(u) if *u == url)) {
                    skip(index, "the page already links to this position");
                    continue;
                }
                let Some(diagram) = geometry.user_to_view(reading.bbox_user) else {
                    skip(index, "the diagram's box is not finite");
                    continue;
                };
                let Some((side, label_view)) = place_label(diagram, view, (w, h), &taken) else {
                    skip(index, "there is no room for a label beside the diagram");
                    continue;
                };
                let Some(link_user) = user_box(&geometry, label_view) else {
                    skip(index, "the label's box is not finite");
                    continue;
                };
                taken.push(label_view);
                links.push(PlannedLink { page: page.page, index, fen: position.fen.clone(), url, side, label_view, link_user });
            }
        }
        let edit = (!links.is_empty()).then(|| {
            let mut edits: Vec<Edit> = Vec::with_capacity(links.len() + 1);
            for l in &links {
                let view_h = scanned.iter().find(|p| p.page == l.page).and_then(|p| p.geometry).map(|g| g.view_size().1).unwrap_or_default();
                let added = AddedText {
                    rect: view_to_display(l.label_view, view_h),
                    text: text.clone(),
                    size: settings.size,
                    color: BLUE,
                    ..AddedText::default()
                };
                edits.push(Edit::AddText { page: l.page, text: added });
            }
            let style = LinkStyle { underline: false, ..LinkStyle::default() };
            edits.push(Edit::AddLinks { links: links.iter().map(|l| (l.page, vec![l.link_user], l.url.clone())).collect(), style });
            Edit::Batch { label: LABEL.into(), edits }
        });
        Ok(LinkPlan { links, skipped, edit })
    }
}
