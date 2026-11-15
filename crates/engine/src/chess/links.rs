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
//! the decoder did not repair gets a link, from the side the diagram is printed from (the one rule
//! of every lichess link, `pdfcraft_chess::lichess::link`, plan A9-G9); by default it must also
//! pass the reading gate (every square read with confidence), so a doubtful reading is not
//! published as the book's position. Every skipped diagram is reported with the reason.
//!
//! **Run again** (plan A9-G9, finding A4-09). A diagram's own label and link are found by their
//! mark, never taken for the book's: the label is an added text (`/PCMark /Added`) saying the
//! label's text, under a link whose address is a lichess analysis board. One that links to the same
//! address already is kept, and the diagram skipped, so running twice adds nothing; one that
//! links elsewhere (a correction changed the position) is taken away in the same undoable step
//! that adds the new one, so the diagram never has two. A lichess link near the diagram without
//! such a label (the book's own, or a label someone edited) is never removed: the diagram is
//! skipped, "it already links to another position". The page's other links and added items are
//! kept clear of, like its diagrams. Each existing link is the diagram's whose label areas it is
//! in, the nearest diagram when two could have it.
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

use pdfcraft_chess::lichess::ANALYSIS_BASE;
use pdfcraft_chess::placement::Color;

use super::ChessScanSettings;
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
/// Built outside the engine by its constructor and its fields, never by a literal (plan A9-Q1b,
/// A7-14): a field added here then breaks no caller (the cause of #185).
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub struct LinkSettings {
    pub text: String,
    pub size: f64,
    /// Also link readings that are legal but did not pass the reading gate (a square read with
    /// little confidence). Repaired readings are never linked.
    pub allow_unaccepted: bool,
    /// Link one diagram only, to a position a person confirmed (the diagram editor): its page
    /// (0-based), its index in the page's reading order and the full FEN. The reading's gate and
    /// repairs do not apply; the FEN must be a legal position.
    pub only: Option<LinkTarget>,
}

/// One diagram to link, with the position to link it to.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkTarget {
    pub page: usize,
    pub index: usize,
    pub fen: String,
}

impl Default for LinkSettings {
    fn default() -> Self {
        LinkSettings { text: DEFAULT_TEXT.into(), size: DEFAULT_SIZE, allow_unaccepted: false, only: None }
    }
}

/// Whether link `url` goes to lichess's analysis board: one of the labels' links.
fn lichess_address(url: &str) -> bool {
    url.strip_prefix(ANALYSIS_BASE).is_some_and(|rest| rest.starts_with('/'))
}

/// A label's areas around `diagram`: where [`place_label`] tries to put a label `w` × `h` large
/// (below, right, above), each grown by the label's own size, so a label of another size or one
/// moved a little is still found.
fn label_areas(diagram: ViewRect, (w, h): (f32, f32)) -> [ViewRect; 3] {
    let grow = |r: ViewRect| ViewRect::new(r.x0 - w, r.y0 - h, r.x1 + w, r.y1 + h);
    [
        grow(ViewRect::new(diagram.x0, diagram.y1, diagram.x1.max(diagram.x0 + w), diagram.y1 + GAP + h)),
        grow(ViewRect::new(diagram.x1, diagram.y0, diagram.x1 + GAP + w, diagram.y1)),
        grow(ViewRect::new(diagram.x0, diagram.y0 - GAP - h, diagram.x1.max(diagram.x0 + w), diagram.y0)),
    ]
}

/// The distance between two boxes (0 when they overlap).
fn distance(a: &ViewRect, b: &ViewRect) -> f32 {
    let dx = (b.x0 - a.x1).max(a.x0 - b.x1).max(0.0);
    let dy = (b.y0 - a.y1).max(a.y0 - b.y1).max(0.0);
    dx.hypot(dy)
}

/// A lichess link already on a page, with the label under it when it is one of ours.
#[derive(Clone, Debug)]
struct Existing {
    /// Its index among the page's annotations (`Edit::DeleteLink`).
    link: usize,
    url: String,
    view: ViewRect,
    /// Our label under it: its index among the page's added items (`Edit::DeleteContent`) and box.
    label: Option<(usize, ViewRect)>,
}

/// One label and link this run takes away, replaced by a new one.
#[derive(Clone, Debug, PartialEq)]
pub struct Replaced {
    pub page: usize,
    /// The diagram's index in the page's reading order (0-based).
    pub index: usize,
    /// The address it linked to.
    pub url: String,
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
    /// The address of the label and link it replaces (plan A9-G9), when the diagram had one of
    /// ours to another position.
    pub replaces: Option<String>,
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

/// A box in display space (an added item's) in view space, on a displayed page `page_h` high;
/// `None` when it is not finite.
fn display_to_view(r: [f64; 4], page_h: f32) -> Option<ViewRect> {
    let [x0, y0, x1, y1] = r.map(|v| v as f32);
    [x0, y0, x1, y1].iter().all(|v| v.is_finite()).then(|| ViewRect::new(x0.min(x1), page_h - y0.max(y1), x0.max(x1), page_h - y0.min(y1)))
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
        // A confirmed position must be one to link; the side it is seen from is its diagram's.
        if let Some(t) = &settings.only {
            pdfcraft_chess::lichess::link(&t.fen, Color::White, false).map_err(|e| e.to_string())?;
        }
        let pages = match &settings.only {
            Some(t) => vec![t.page],
            None => pages.to_vec(),
        };
        // A person's correction wins over the reading: its position is linked.
        let scanned = self.chess_scan_corrected(id, &pages, scan)?.pages;
        let doc = self.get(id).ok_or("no such document")?;
        let (existing, added) = (doc.links(), doc.added());
        let size = settings.size as f32;
        let (w, h) = ((pdfcraft_fonts::helvetica_width(&text, settings.size) as f32 + 2.0), size * LINE);
        let mut links = Vec::new();
        let mut skipped = Vec::new();
        // What this run takes away: (page, link index) and (page, added item index).
        let mut gone_links: Vec<(usize, usize)> = Vec::new();
        let mut gone_labels: Vec<(usize, usize)> = Vec::new();
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
            let diagrams: Vec<Option<ViewRect>> = page.diagrams.iter().map(|d| geometry.user_to_view(d.candidate.bbox_user)).collect();
            // The page's added items, by their index among them (in view space), and its lichess
            // links, each with our label under it when there is one.
            let items: Vec<(usize, ViewRect, Option<&str>)> = added
                .iter()
                .filter(|a| a.page == page.page)
                .enumerate()
                .filter_map(|(i, a)| {
                    let text = match &a.content {
                        pdfcraft_edit::Content::Text(t) => Some(t.text.as_str()),
                        pdfcraft_edit::Content::Image(_) => None,
                    };
                    Some((i, display_to_view(a.content.rect(), view.1)?, text))
                })
                .collect();
            let ours = |t: Option<&str>| t.is_some_and(|t| t == text || t == DEFAULT_TEXT);
            let lichess: Vec<Existing> = existing
                .iter()
                .filter(|l| l.page == page.page)
                .filter_map(|l| {
                    let pdfcraft_annot::links::LinkAction::Uri(url) = &l.action else { return None };
                    if !lichess_address(url) {
                        return None;
                    }
                    let [x0, y0, x1, y1] = l.rect.map(|v| v as f32);
                    let view = geometry.user_to_view(pdfcraft_chessvision::coords::PageRect::new(x0, y0, x1, y1))?;
                    let label = items.iter().find(|(_, r, t)| ours(*t) && overlaps(r, &view)).map(|(i, r, _)| (*i, *r));
                    Some(Existing { link: l.index, url: url.clone(), view, label })
                })
                .collect();
            // Each lichess link is the diagram's whose label areas it is in, the nearest one's.
            let owner: Vec<Option<usize>> = lichess
                .iter()
                .map(|e| {
                    diagrams
                        .iter()
                        .enumerate()
                        .filter_map(|(i, d)| d.map(|d| (i, d)))
                        .filter(|(_, d)| label_areas(*d, (w, h)).iter().any(|a| overlaps(a, &e.view)))
                        .min_by(|a, b| distance(&a.1, &e.view).total_cmp(&distance(&b.1, &e.view)))
                        .map(|(i, _)| i)
                })
                .collect();
            // What labels keep clear of: every diagram, the labels placed before, and the page's
            // other links and added items (by the lichess link they belong to, when they do, so a
            // replaced one leaves its place).
            let mut placed: Vec<ViewRect> = Vec::new();
            let mut occupied: Vec<(ViewRect, Option<usize>)> = lichess.iter().enumerate().map(|(k, e)| (e.view, Some(k))).collect();
            for l in existing.iter().filter(|l| l.page == page.page) {
                let [x0, y0, x1, y1] = l.rect.map(|v| v as f32);
                if let Some(v) = geometry.user_to_view(pdfcraft_chessvision::coords::PageRect::new(x0, y0, x1, y1))
                    && !lichess.iter().any(|e| e.link == l.index)
                {
                    occupied.push((v, None));
                }
            }
            for (i, r, _) in &items {
                let pair = lichess.iter().position(|e| e.label.is_some_and(|(j, _)| j == *i));
                occupied.push((*r, pair));
            }
            let mut replaced: Vec<usize> = Vec::new();
            for (index, _) in page.diagrams.iter().enumerate() {
                let (fen, url) = if let Some(t) = &settings.only {
                    if index != t.index {
                        continue;
                    }
                    // The position a person confirmed, from the side its diagram is printed from.
                    let from = readings.get(index).and_then(|r| r.outcome.as_ref().ok()).map_or(Color::White, |p| p.printed_from());
                    match pdfcraft_chess::lichess::link(&t.fen, from, false) {
                        Ok(url) => (t.fen.clone(), url),
                        Err(e) => {
                            skip(index, &e.to_string());
                            continue;
                        }
                    }
                } else {
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
                    let url = match position.lichess_link() {
                        Ok(url) => url,
                        Err(e) => {
                            skip(index, &e.to_string());
                            continue;
                        }
                    };
                    if !position.accepted && !settings.allow_unaccepted {
                        skip(index, "the reading did not pass the gate (a square read with little confidence); review it first");
                        continue;
                    }
                    (position.fen.clone(), url)
                };
                let mine: Vec<usize> = owner.iter().enumerate().filter(|(_, o)| **o == Some(index)).map(|(k, _)| k).collect();
                let same = |u: &str| u == url;
                if mine.iter().any(|k| lichess.get(*k).is_some_and(|e| same(&e.url)))
                    || existing.iter().any(|l| l.page == page.page && matches!(&l.action, pdfcraft_annot::links::LinkAction::Uri(u) if same(u)))
                {
                    skip(index, "the page already links to this position");
                    continue;
                }
                if mine.iter().any(|k| lichess.get(*k).is_some_and(|e| e.label.is_none())) {
                    skip(index, "the diagram already links to another position (a link this did not add, or a label someone edited)");
                    continue;
                }
                let Some(diagram) = diagrams.get(index).copied().flatten() else {
                    skip(index, "the diagram's box is not finite");
                    continue;
                };
                let taken: Vec<ViewRect> = diagrams
                    .iter()
                    .flatten()
                    .copied()
                    .chain(placed.iter().copied())
                    .chain(occupied.iter().filter(|(_, pair)| pair.is_none_or(|k| !replaced.contains(&k) && !mine.contains(&k))).map(|(r, _)| *r))
                    .collect();
                let Some((side, label_view)) = place_label(diagram, view, (w, h), &taken) else {
                    skip(index, "there is no room for a label beside the diagram");
                    continue;
                };
                let Some(link_user) = user_box(&geometry, label_view) else {
                    skip(index, "the label's box is not finite");
                    continue;
                };
                placed.push(label_view);
                for k in &mine {
                    if let Some(e) = lichess.get(*k) {
                        gone_links.push((page.page, e.link));
                        if let Some((label, _)) = e.label {
                            gone_labels.push((page.page, label));
                        }
                    }
                }
                let replaces = mine.first().and_then(|k| lichess.get(*k)).map(|e| e.url.clone());
                replaced.extend(mine);
                links.push(PlannedLink { page: page.page, index, fen, url, side, label_view, link_user, replaces });
            }
        }
        if let Some(t) = &settings.only
            && links.is_empty()
            && skipped.is_empty()
        {
            skipped.push(SkippedDiagram { page: t.page, index: t.index, reason: "the page has no such diagram".into() });
        }
        let edit = (!links.is_empty()).then(|| {
            let mut edits: Vec<Edit> = Vec::with_capacity(links.len() + gone_links.len() + gone_labels.len() + 1);
            // The replaced labels and links first, each page's from the last index, so the indices
            // of the rest stay what they were.
            gone_links.sort_unstable_by(|a, b| b.cmp(a));
            gone_links.dedup();
            gone_labels.sort_unstable_by(|a, b| b.cmp(a));
            gone_labels.dedup();
            edits.extend(gone_links.iter().map(|&(page, index)| Edit::DeleteLink { page, index }));
            edits.extend(gone_labels.iter().map(|&(page, index)| Edit::DeleteContent { page, index }));
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
