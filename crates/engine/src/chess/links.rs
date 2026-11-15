//! Chess ▸ Add lichess links (blueprint S11): under each diagram whose position is known, a
//! short label that opens the position on lichess.org.
//!
//! [`Session::chess_link_plan`] scans the pages (with readings), keeps the diagrams that may be
//! linked and places a label for each, and returns one undoable [`Edit::Batch`]: per diagram an
//! added text (`Edit::AddKeyedText`, so it stays upright on a turned page and can be moved or
//! edited later like any added text) and a link over it (`Edit::AddMarkedLinks`, a URI action;
//! nothing is fetched — PdfCraft makes no network request, the reader's viewer opens it when
//! clicked), the two keyed alike.
//!
//! **Which diagrams.** Only a position that is legal with the side to move it was given and that
//! the decoder did not repair gets a link, from the side the diagram is printed from (the one rule
//! of every lichess link, `pdfcraft_chess::lichess::link`, plan A9-G9); by default it must also
//! pass the reading gate (every square read with confidence), so a doubtful reading is not
//! published as the book's position. Every skipped diagram is reported with the reason.
//!
//! **Run again** (plan A9-G9, A9-G9b; finding A4-09). A diagram's own label and link are found by
//! their mark, never by where they are: the link carries `/PCMark /Added` and a key (`/PCKey`,
//! [`KEY_PREFIX`], the diagram's number on its page and the address), and the label, an added
//! text, the same key. A pair to the same address is kept, and the diagram skipped, so running
//! twice adds nothing (another pair of the diagram's, stale, goes); a pair to another address (a
//! correction changed the position) is taken away in the same undoable step that adds the new one,
//! so the diagram never has two, wherever its label was moved. Only a pair of ours is ever taken
//! away, and only its diagram's run takes it: a pair whose label's text or link's address someone
//! changed stays, and the diagram is skipped; a lichess link of anyone else by the diagram (the
//! book's own, or one drawn over our label) stays, and the diagram is skipped. A pair from an older
//! build, which keyed neither, is taken for ours only in the shape it wrote: a lichess link exactly
//! over a label saying the label's text, by one diagram alone; anything else is left. The page's
//! links and added items are kept clear of, like its diagrams.
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

/// The key a label and its link carry (`/PCKey`; plan A9-G9b): this, the diagram's number in its
/// page's reading order (1-based) and the address. The two find each other and their diagram by
/// it, never by where they are.
pub const KEY_PREFIX: &str = "pdfcraft-lichess ";

/// What the plan says of a confirmed position whose diagram's reading does not say which side it
/// is printed from ([`PlannedLink::note`]).
pub const SIDE_ASSUMED: &str = "side assumed: White (nothing says which side the diagram is printed from)";

/// How far an older build's label may be from its link's box, in points, to be taken for the one it
/// wrote under it; its bottom may be up to a tenth of its height further (the box is read back with
/// the text's own line height).
const SAME: f32 = 0.5;

/// The key of the label and link of diagram `index` (0-based) to `url`.
fn pair_key(index: usize, url: &str) -> String {
    format!("{KEY_PREFIX}{} {url}", index.saturating_add(1))
}

/// The diagram (0-based) and the address key `key` names; `None` for a key that is no lichess
/// label's.
fn key_parts(key: &str) -> Option<(usize, &str)> {
    let (n, url) = key.strip_prefix(KEY_PREFIX)?.split_once(' ')?;
    let n = n.parse::<usize>().ok()?.checked_sub(1)?;
    lichess_address(url).then_some((n, url))
}

/// Whether box `label` is where an older build put a label under its link `link`.
fn same_box(label: &ViewRect, link: &ViewRect) -> bool {
    let bottom = SAME + (link.y1 - link.y0).abs() * 0.1;
    (label.x0 - link.x0).abs() <= SAME
        && (label.x1 - link.x1).abs() <= SAME
        && (label.y0 - link.y0).abs() <= SAME
        && (label.y1 - link.y1).abs() <= bottom
}

/// An added item of a page: its index among the page's added items, its box (view space), its
/// text when it is text, and its key when it is a lichess label's.
struct PageItem<'a> {
    item: usize,
    view: ViewRect,
    text: Option<&'a str>,
    key: Option<&'a str>,
}

/// A link of a page: its index among the page's annotations, its box (view space), its web
/// address when it has one, and its key when PdfCraft marked it as a lichess label's.
struct PageLink<'a> {
    annot: usize,
    view: ViewRect,
    url: Option<&'a str>,
    key: Option<&'a str>,
}

/// A label and link of ours on a page, and the diagram they belong to.
#[derive(Clone, Debug, PartialEq)]
struct Pair {
    diagram: usize,
    /// The address: its key's.
    url: String,
    /// Its links' indices among the page's annotations, and its labels' among the page's added
    /// items.
    links: Vec<usize>,
    labels: Vec<usize>,
    /// Someone changed it since it was added (its label's text, or its link's address): it stays.
    edited: bool,
}

/// The pairs of ours among a page's added `items` and `links`: each by its key, the label's and
/// the link's the same; and, from an older build that keyed neither, a lichess link exactly over
/// a label saying the label's text (`ours`), the shape it wrote, when one diagram alone has it in
/// its label areas (a label `label` large). Geometry only finds an older pair's diagram: nothing
/// else is ever taken for ours.
fn pairs_of<'a>(
    items: &[PageItem<'a>],
    links: &[PageLink<'a>],
    diagrams: &[Option<ViewRect>],
    label: (f32, f32),
    ours: impl Fn(&str) -> bool,
) -> Vec<Pair> {
    /// The pair keyed `key`, made when it is the first of its key.
    fn slot<'a, 'k>(keyed: &'k mut Vec<(&'a str, Pair)>, key: &'a str) -> Option<&'k mut Pair> {
        let at = match keyed.iter().position(|(k, _)| *k == key) {
            Some(at) => at,
            None => {
                let (diagram, url) = key_parts(key)?;
                keyed.push((key, Pair { diagram, url: url.to_owned(), links: Vec::new(), labels: Vec::new(), edited: false }));
                keyed.len().checked_sub(1)?
            }
        };
        keyed.get_mut(at).map(|(_, p)| p)
    }
    let mut keyed: Vec<(&'a str, Pair)> = Vec::new();
    for l in links {
        let Some(p) = l.key.and_then(|key| slot(&mut keyed, key)) else { continue };
        p.links.push(l.annot);
        // An address changed in Link Properties makes the link the person's.
        p.edited |= l.url != Some(p.url.as_str());
    }
    for i in items {
        let Some(p) = i.key.and_then(|key| slot(&mut keyed, key)) else { continue };
        p.labels.push(i.item);
        p.edited |= !i.text.is_some_and(&ours);
    }
    let mut out: Vec<Pair> = keyed.into_iter().map(|(_, p)| p).collect();
    for l in links.iter().filter(|l| l.key.is_none()) {
        let Some(url) = l.url.filter(|u| lichess_address(u)) else { continue };
        let Some(under) = items.iter().find(|i| i.key.is_none() && i.text.is_some_and(&ours) && same_box(&i.view, &l.view)) else { continue };
        if out.iter().any(|p| p.labels.contains(&under.item)) {
            continue;
        }
        let mut owners = diagrams
            .iter()
            .enumerate()
            .filter(|(_, d)| d.is_some_and(|d| label_areas(d, label).iter().any(|a| overlaps(a, &l.view))))
            .map(|(k, _)| k);
        let (Some(diagram), None) = (owners.next(), owners.next()) else { continue };
        out.push(Pair { diagram, url: url.to_owned(), links: vec![l.annot], labels: vec![under.item], edited: false });
    }
    out
}

/// One label and link of a diagram this run takes away: replaced by a new one, or, in
/// [`LinkPlan::removed`], a stale one beside the diagram's pair to its position.
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
    /// What was assumed, said with the link: [`SIDE_ASSUMED`] for a confirmed position whose
    /// diagram's reading does not say which side it is printed from (plan A9-G9b).
    pub note: Option<String>,
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
    /// Stale labels and links of ours taken away from a diagram that keeps its pair to its
    /// position (plan A9-G9b).
    pub removed: Vec<Replaced>,
    /// The undoable edit (a [`Edit::Batch`] labelled [`LABEL`]); `None` when there is nothing to
    /// add or take away.
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
        let mut removed = Vec::new();
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
            // The page's added items and links, in view space, with their keys.
            let items: Vec<PageItem<'_>> = added
                .iter()
                .filter(|a| a.page == page.page)
                .enumerate()
                .filter_map(|(item, a)| {
                    let text = match &a.content {
                        pdfcraft_edit::Content::Text(t) => Some(t.text.as_str()),
                        pdfcraft_edit::Content::Image(_) => None,
                    };
                    Some(PageItem {
                        item,
                        view: display_to_view(a.content.rect(), view.1)?,
                        text,
                        key: a.key.as_deref().filter(|k| key_parts(k).is_some()),
                    })
                })
                .collect();
            let page_links: Vec<PageLink<'_>> = existing
                .iter()
                .filter(|l| l.page == page.page)
                .filter_map(|l| {
                    let [x0, y0, x1, y1] = l.rect.map(|v| v as f32);
                    let view = geometry.user_to_view(pdfcraft_chessvision::coords::PageRect::new(x0, y0, x1, y1))?;
                    let url = match &l.action {
                        pdfcraft_annot::links::LinkAction::Uri(u) => Some(u.as_str()),
                        _ => None,
                    };
                    let key = l.key.as_deref().filter(|k| l.mark.as_deref() == Some(crate::ADDED_MARK) && key_parts(k).is_some());
                    Some(PageLink { annot: l.index, view, url, key })
                })
                .collect();
            let ours = |t: &str| t == text || t == DEFAULT_TEXT;
            let pairs = pairs_of(&items, &page_links, &diagrams, (w, h), ours);
            // The lichess links that are no pair of ours: never taken away.
            let paired = |annot: usize| pairs.iter().any(|p| p.links.contains(&annot));
            let foreign: Vec<&PageLink<'_>> = page_links.iter().filter(|l| !paired(l.annot)).collect();
            let mut placed: Vec<ViewRect> = Vec::new();
            // What this page's diagrams take away, so a later label may go there.
            let mut gone_here: (Vec<usize>, Vec<usize>) = (Vec::new(), Vec::new());
            for (index, _) in page.diagrams.iter().enumerate() {
                let (fen, url, note) = if let Some(t) = &settings.only {
                    if index != t.index {
                        continue;
                    }
                    // The position a person confirmed, from the side its diagram is printed from,
                    // when its reading says; else White's, and the plan says it was assumed.
                    let read = readings.get(index).and_then(|r| r.outcome.as_ref().ok());
                    let from = read.map_or(Color::White, |p| p.printed_from());
                    let note = read.is_none_or(|p| p.orientation.is_none()).then(|| SIDE_ASSUMED.to_owned());
                    match pdfcraft_chess::lichess::link(&t.fen, from, false) {
                        Ok(url) => (t.fen.clone(), url, note),
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
                    (position.fen.clone(), url, None)
                };
                let Some(diagram) = diagrams.get(index).copied().flatten() else {
                    skip(index, "the diagram's box is not finite");
                    continue;
                };
                let key = pair_key(index, &url);
                if key.len() > pdfcraft_edit::MAX_KEY_LEN {
                    skip(index, "the position's address is too long to mark its label with");
                    continue;
                }
                let mine: Vec<&Pair> = pairs.iter().filter(|p| p.diagram == index).collect();
                if mine.iter().any(|p| p.edited) {
                    skip(index, "its label or link was changed since it was added, so it is kept (delete it to link the diagram again)");
                    continue;
                }
                let areas = label_areas(diagram, (w, h));
                if foreign.iter().any(|l| l.url.is_some_and(lichess_address) && areas.iter().any(|a| overlaps(a, &l.view))) {
                    skip(index, "the diagram already links to another position (a link this did not add)");
                    continue;
                }
                if foreign.iter().any(|l| l.url == Some(url.as_str())) {
                    skip(index, "the page already links to this position");
                    continue;
                }
                // Its own pair to this position stays; any other of its pairs goes (a stale one, or
                // one missing its label or link).
                let current = mine.iter().position(|p| p.url == url && !p.links.is_empty() && !p.labels.is_empty());
                let stale: Vec<&Pair> = mine.iter().enumerate().filter(|(k, _)| Some(*k) != current).map(|(_, p)| *p).collect();
                let stale_links: Vec<usize> = stale.iter().flat_map(|p| p.links.iter().copied()).collect();
                let stale_labels: Vec<usize> = stale.iter().flat_map(|p| p.labels.iter().copied()).collect();
                if current.is_some() {
                    gone_links.extend(stale_links.iter().map(|&k| (page.page, k)));
                    gone_labels.extend(stale_labels.iter().map(|&k| (page.page, k)));
                    gone_here.0.extend(&stale_links);
                    gone_here.1.extend(&stale_labels);
                    removed.extend(stale.iter().map(|p| Replaced { page: page.page, index, url: p.url.clone() }));
                    skip(index, "the page already links to this position");
                    continue;
                }
                // Clear of every diagram, the labels placed before, and the page's links and added
                // items, but those taken away.
                let taken: Vec<ViewRect> = diagrams
                    .iter()
                    .flatten()
                    .copied()
                    .chain(placed.iter().copied())
                    .chain(page_links.iter().filter(|l| !gone_here.0.contains(&l.annot) && !stale_links.contains(&l.annot)).map(|l| l.view))
                    .chain(items.iter().filter(|i| !gone_here.1.contains(&i.item) && !stale_labels.contains(&i.item)).map(|i| i.view))
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
                gone_links.extend(stale_links.iter().map(|&k| (page.page, k)));
                gone_labels.extend(stale_labels.iter().map(|&k| (page.page, k)));
                gone_here.0.extend(&stale_links);
                gone_here.1.extend(&stale_labels);
                let replaces = stale.first().map(|p| p.url.clone());
                links.push(PlannedLink { page: page.page, index, fen, url, side, label_view, link_user, replaces, note });
            }
        }
        if let Some(t) = &settings.only
            && links.is_empty()
            && skipped.is_empty()
        {
            skipped.push(SkippedDiagram { page: t.page, index: t.index, reason: "the page has no such diagram".into() });
        }
        let edit = (!links.is_empty() || !gone_links.is_empty() || !gone_labels.is_empty()).then(|| {
            let mut edits: Vec<Edit> = Vec::with_capacity(links.len() + gone_links.len() + gone_labels.len() + 1);
            // The labels and links taken away first, each page's from the last index, so the
            // indices of the rest stay what they were.
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
                edits.push(Edit::AddKeyedText { page: l.page, text: added, key: pair_key(l.index, &l.url) });
            }
            if !links.is_empty() {
                let style = LinkStyle { underline: false, ..LinkStyle::default() };
                let marked = links.iter().map(|l| (l.page, l.link_user, l.url.clone(), pair_key(l.index, &l.url))).collect();
                edits.push(Edit::AddMarkedLinks { links: marked, style });
            }
            Edit::Batch { label: LABEL.into(), edits }
        });
        Ok(LinkPlan { links, skipped, removed, edit })
    }
}
