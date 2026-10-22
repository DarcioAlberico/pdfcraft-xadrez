//! Segmentation: binarization, connected components, the text scale and the character-box rules.
//!
//! Re-derived from the S13a spike, which ported the Python trunk's `boxes.py` (`unir_pingos`,
//! with the italic correction the spike lacked), `empilhados.py`, `colados.py`, `linhas.py` and
//! `duas_linhas.py`. Every function is total: sizes are capped by the caller ([`super::read`]),
//! lookups go through `get`, and arithmetic on coordinates is done in `i64`, which no `u32`
//! image coordinate can overflow.

/// Fewest pixels of ink-to-image share (Otsu) that look like text, and the most.
const PLAUSIBLE_INK: std::ops::RangeInclusive<f64> = 0.0005..=0.35;
/// A box this short, against the median height, is a dot, an accent or a mark.
pub(crate) const SHORT_BOX: f64 = 0.65;
/// Smallest glyph box area, in squared scales.
const MIN_AREA_GLYPH: f64 = 0.005;
/// Widest aspect (width / height) of a glyph box; flatter ones are rules or bars.
const MAX_ASPECT: f64 = 6.0;
/// Widest and tallest a bar of `=` or `±` may be, in scales.
const BAR_MAX_WIDTH: f64 = 1.20;
const BAR_MAX_HEIGHT: f64 = 0.55;
/// Farthest a dot may sit from its stem, against the median height.
const DOT_MAX_GAP: f64 = 0.40;
/// Least share of a dot's width that must sit over its stem.
const DOT_MIN_OVERLAP: f64 = 0.55;
/// A box taller than this many scales, and more than [`MAX_ASPECT`] times taller than wide, is a
/// vertical rule (a diagram's frame left on a scan, a column rule), never a character: `l`, `1`
/// and `|` are about one scale tall.
const RULE_MIN_HEIGHT: f64 = 2.0;
/// A stem this many times taller than wide is measured for an italic slant.
const NARROW_STEM: f64 = 2.0;

/// An 8-bit gray view, 0 = black, row-major.
#[derive(Clone, Copy)]
pub(crate) struct Gray<'a> {
    pub w: usize,
    pub h: usize,
    pub px: &'a [u8],
}

impl Gray<'_> {
    /// The pixel at `(x, y)`, paper (255) outside the image.
    pub fn at(&self, x: usize, y: usize) -> u8 {
        if x >= self.w || y >= self.h {
            return 255;
        }
        y.checked_mul(self.w).and_then(|r| r.checked_add(x)).and_then(|i| self.px.get(i)).copied().unwrap_or(255)
    }
}

/// An ink mask: `true` is ink.
pub(crate) struct Bin {
    pub w: usize,
    pub h: usize,
    pub ink: Vec<bool>,
}

impl Bin {
    /// Ink at `(x, y)`; `false` outside the image.
    pub fn at(&self, x: i64, y: i64) -> bool {
        let (Ok(x), Ok(y)) = (usize::try_from(x), usize::try_from(y)) else { return false };
        if x >= self.w || y >= self.h {
            return false;
        }
        y.checked_mul(self.w).and_then(|r| r.checked_add(x)).and_then(|i| self.ink.get(i)).copied().unwrap_or(false)
    }
}

/// A box in pixels, end-exclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Bx {
    pub x0: i64,
    pub y0: i64,
    pub x1: i64,
    pub y1: i64,
}

impl Bx {
    pub fn w(&self) -> i64 {
        self.x1 - self.x0
    }
    pub fn h(&self) -> i64 {
        self.y1 - self.y0
    }
    pub fn union(&self, o: &Bx) -> Bx {
        Bx { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
    }
    /// Width of the columns both boxes cover (negative when apart).
    pub fn common_x(&self, o: &Bx) -> i64 {
        self.x1.min(o.x1) - self.x0.max(o.x0)
    }
}

/// Otsu's threshold over a 256-bin histogram (as OpenCV's `THRESH_OTSU`): ink is `gray <= t`.
fn otsu(px: &[u8]) -> u8 {
    let mut hist = [0u64; 256];
    for &p in px {
        if let Some(h) = hist.get_mut(usize::from(p)) {
            *h += 1;
        }
    }
    let total = (px.len() as f64).max(1.0);
    let mu: f64 = hist.iter().enumerate().map(|(i, &c)| i as f64 * c as f64).sum::<f64>() / total;
    let (mut q1, mut mu1, mut best, mut best_t) = (0.0f64, 0.0f64, -1.0f64, 0u8);
    for (t, &c) in hist.iter().enumerate() {
        let p = c as f64 / total;
        let q1n = q1 + p;
        if q1n > 0.0 {
            mu1 = (q1 * mu1 + t as f64 * p) / q1n;
        }
        q1 = q1n;
        let q2 = 1.0 - q1;
        if q1 < 1e-12 || q2 < 1e-12 {
            continue;
        }
        let mu2 = (mu - q1 * mu1) / q2;
        let var = q1 * q2 * (mu1 - mu2) * (mu1 - mu2);
        if var > best {
            best = var;
            best_t = u8::try_from(t).unwrap_or(u8::MAX);
        }
    }
    best_t
}

/// The trunk's `binarize(auto)`: Otsu when the ink share is plausible for text, else a local
/// threshold (pixels 10 levels darker than the mean of their window), for uneven scans.
pub(crate) fn binarize(g: &Gray) -> Bin {
    let t = otsu(g.px);
    let ink: Vec<bool> = g.px.iter().map(|&p| p <= t).collect();
    let frac = ink.iter().filter(|&&b| b).count() as f64 / ink.len().max(1) as f64;
    if PLAUSIBLE_INK.contains(&frac) {
        return Bin { w: g.w, h: g.h, ink };
    }
    Bin { w: g.w, h: g.h, ink: adaptive(g) }
}

/// Box-mean local threshold with running column sums: memory linear in the width.
fn adaptive(g: &Gray) -> Vec<bool> {
    let side = 15usize.max((g.w.min(g.h) / 20) | 1);
    let r = side / 2;
    let mut col = vec![0u64; g.w];
    let add_row = |col: &mut [u64], y: usize, sign: bool| {
        for (x, c) in col.iter_mut().enumerate() {
            let v = u64::from(g.at(x, y));
            *c = if sign { c.saturating_add(v) } else { c.saturating_sub(v) };
        }
    };
    for y in 0..(r + 1).min(g.h) {
        add_row(&mut col, y, true);
    }
    let mut out = Vec::with_capacity(g.w.saturating_mul(g.h));
    let mut prefix = vec![0u64; g.w + 1];
    for y in 0..g.h {
        if y > 0 {
            if y > r {
                add_row(&mut col, y - r - 1, false);
            }
            if y + r < g.h {
                add_row(&mut col, y + r, true);
            }
        }
        let (y0, y1) = (y.saturating_sub(r), (y + r + 1).min(g.h));
        let mut acc = 0u64;
        for (x, c) in col.iter().enumerate() {
            acc = acc.saturating_add(*c);
            if let Some(p) = prefix.get_mut(x + 1) {
                *p = acc;
            }
        }
        for x in 0..g.w {
            let (x0, x1) = (x.saturating_sub(r), (x + r + 1).min(g.w));
            let s = prefix.get(x1).copied().unwrap_or(0).saturating_sub(prefix.get(x0).copied().unwrap_or(0));
            let n = ((x1 - x0) * (y1 - y0)).max(1) as f64;
            out.push(f64::from(g.at(x, y)) <= s as f64 / n - 10.0);
        }
    }
    out
}

/// A connected component: its box and its ink pixel count.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Comp {
    pub b: Bx,
    pub area: i64,
}

/// The 8-connected components of the ink, or `None` past `cap` components. Iterative flood
/// fill; the work and the memory are linear in the pixels.
pub(crate) fn components(bin: &Bin, cap: usize) -> Option<Vec<Comp>> {
    let mut left = bin.ink.clone();
    let mut out = Vec::new();
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let idx = |x: usize, y: usize| y.checked_mul(bin.w).and_then(|r| r.checked_add(x));
    for y in 0..bin.h {
        for x in 0..bin.w {
            let Some(i) = idx(x, y) else { continue };
            match left.get_mut(i) {
                Some(v) if *v => *v = false,
                _ => continue,
            }
            if out.len() >= cap {
                return None;
            }
            stack.push((x, y));
            let (xi, yi) = (x as i64, y as i64);
            let mut b = Bx { x0: xi, y0: yi, x1: xi + 1, y1: yi + 1 };
            let mut area = 0i64;
            while let Some((cx, cy)) = stack.pop() {
                area += 1;
                let (cxi, cyi) = (cx as i64, cy as i64);
                b = b.union(&Bx { x0: cxi, y0: cyi, x1: cxi + 1, y1: cyi + 1 });
                for ny in cy.saturating_sub(1)..=(cy + 1).min(bin.h.saturating_sub(1)) {
                    for nx in cx.saturating_sub(1)..=(cx + 1).min(bin.w.saturating_sub(1)) {
                        if let Some(v) = idx(nx, ny).and_then(|j| left.get_mut(j))
                            && *v
                        {
                            *v = false;
                            stack.push((nx, ny));
                        }
                    }
                }
            }
            out.push(Comp { b, area });
        }
    }
    Some(out)
}

/// The trunk's `escala_de_texto`: the box height at the median of the ink mass, over components
/// whose box is at most 1% of the image (on a page, that leaves figures out). Zero when there
/// is no ink.
///
/// On a short crop of one line every letter is more than 1% of the image, and the trunk's rule
/// measured the dots and bars that were left: a scale of 3 px on 30 px letters, which switched
/// off every rule sized by it. So when the small components hold less than half the ink, the
/// big ones are the text, and all components count.
pub(crate) fn scale(comps: &[Comp], image_px: usize) -> i64 {
    let lim = image_px as f64 * 0.01;
    let mut sel: Vec<&Comp> = comps.iter().filter(|c| (c.b.w() as f64 * c.b.h() as f64) <= lim).collect();
    let mass = |v: &[&Comp]| v.iter().map(|c| c.area).sum::<i64>();
    let all: Vec<&Comp> = comps.iter().collect();
    if mass(&sel).saturating_mul(2) < mass(&all) {
        sel = all;
    }
    sel.sort_by_key(|c| c.b.h());
    let total: i64 = sel.iter().map(|c| c.area).sum();
    if total <= 0 {
        return 0;
    }
    let half = total as f64 / 2.0;
    let mut acc = 0i64;
    for c in &sel {
        acc += c.area;
        if acc as f64 >= half {
            return c.b.h();
        }
    }
    sel.last().map_or(0, |c| c.b.h())
}

/// Character boxes (`caixas_de_caractere`) and bar candidates (`empilhados.barras`). A component
/// whose box sits inside a bigger component's box, with less ink, is inner detail (the inside
/// of an outlined figurine) and dropped, and so are rules, flat or upright. Quadratic in the components: the caller caps them.
pub(crate) fn char_boxes(comps: &[Comp], scale: i64) -> (Vec<Bx>, Vec<Bx>) {
    let s = scale as f64;
    let floor = MIN_AREA_GLYPH * s * s;
    let mut boxes = Vec::new();
    let mut bars = Vec::new();
    for (i, c) in comps.iter().enumerate() {
        let c_area = c.b.w() * c.b.h();
        let inside = comps.iter().enumerate().any(|(j, o)| {
            j != i && o.area > c.area && o.b.x0 <= c.b.x0 && o.b.y0 <= c.b.y0 && o.b.x1 >= c.b.x1 && o.b.y1 >= c.b.y1 && o.b.w() * o.b.h() > c_area
        });
        if inside {
            continue;
        }
        let (w, h) = (c.b.w() as f64, c.b.h() as f64);
        if w <= 0.0 || h <= 0.0 || w * h < floor {
            continue;
        }
        if w / h > MAX_ASPECT {
            if w <= BAR_MAX_WIDTH * s && h <= BAR_MAX_HEIGHT * s {
                bars.push(c.b);
            }
            continue;
        }
        // A box that tall would also join every line beside it into one.
        if h / w > MAX_ASPECT && h > RULE_MIN_HEIGHT * s {
            continue;
        }
        boxes.push(c.b);
    }
    (boxes, bars)
}

/// The middle element of the box heights (upper median), at least 1.
fn median_height(boxes: &[Bx]) -> f64 {
    let mut v: Vec<i64> = boxes.iter().map(Bx::h).collect();
    v.sort_unstable();
    let m = v.get(v.len() / 2).copied().unwrap_or(0) as f64;
    if m <= 0.0 { 1.0 } else { m }
}

/// The slant `dx/dy` of a narrow stem (the trunk's `inclinacao_do_box`): the shift between the
/// ink centroids of its top and bottom halves over half its height. Zero for a box that is not
/// a narrow stem or cannot be measured.
fn stem_slant(bin: &Bin, b: &Bx) -> f64 {
    if b.h() < 4 || b.w() <= 0 || (b.h() as f64) < NARROW_STEM * b.w() as f64 {
        return 0.0;
    }
    let mid = b.y0 + b.h() / 2;
    let centre = |y0: i64, y1: i64| -> Option<f64> {
        let (mut sum, mut n) = (0i64, 0i64);
        for y in y0..y1 {
            for x in b.x0..b.x1 {
                if bin.at(x, y) {
                    sum += x;
                    n += 1;
                }
            }
        }
        (n > 0).then(|| sum as f64 / n as f64)
    };
    match (centre(b.y0, mid), centre(mid, b.y1)) {
        (Some(top), Some(bottom)) => 2.0 * (top - bottom) / b.h() as f64,
        _ => 0.0,
    }
}

/// `boxes.unir_pingos`: dots and accents rejoin their stem, with the trunk's italic correction.
/// An italic `i` sets its dot to the right of the stem; the dot's x is projected back by the
/// stem's slant times the gap, so it overlaps the stem again. The shift grows with the gap, so a
/// full stop beside a letter (gap about zero) never moves. Order is preserved.
pub(crate) fn join_dots(boxes: &[Bx], bin: &Bin) -> Vec<Bx> {
    let med = median_height(boxes);
    let short = SHORT_BOX * med;
    let max_gap = DOT_MAX_GAP * med;
    let mut slants: Vec<Option<f64>> = vec![None; boxes.len()];
    let mut absorbed: Vec<Option<usize>> = vec![None; boxes.len()];
    for (i, c) in boxes.iter().enumerate() {
        if c.h() as f64 >= short {
            continue;
        }
        let mut best: Option<usize> = None;
        let mut best_gap = max_gap;
        for (j, b) in boxes.iter().enumerate() {
            if (b.h() as f64) < short {
                continue;
            }
            let above = c.y1 <= b.y0;
            let gap = if above {
                (b.y0 - c.y1) as f64
            } else if b.y1 <= c.y0 {
                (c.y0 - b.y1) as f64
            } else {
                continue;
            };
            if !(0.0..=best_gap).contains(&gap) {
                continue;
            }
            if b.x0 as f64 > c.x1 as f64 + max_gap || (b.x1 as f64) < c.x0 as f64 - max_gap {
                continue;
            }
            let shift = if above {
                let slant = match slants.get(j).copied().flatten() {
                    Some(s) => s,
                    None => {
                        let s = stem_slant(bin, b);
                        if let Some(slot) = slants.get_mut(j) {
                            *slot = Some(s);
                        }
                        s
                    }
                };
                -slant * gap
            } else {
                0.0
            };
            let common = (c.x1 as f64 + shift).min(b.x1 as f64) - (c.x0 as f64 + shift).max(b.x0 as f64);
            let ov = if c.w() > 0 { common / c.w() as f64 } else { 0.0 };
            if ov < DOT_MIN_OVERLAP {
                continue;
            }
            best = Some(j);
            best_gap = gap;
        }
        if let Some(slot) = absorbed.get_mut(i) {
            *slot = best;
        }
    }
    let mut merged: Vec<Bx> = boxes.to_vec();
    for (i, a) in absorbed.iter().enumerate() {
        if let (Some(j), Some(dot)) = (a, boxes.get(i))
            && let Some(m) = merged.get_mut(*j)
        {
            *m = m.union(dot);
        }
    }
    merged.into_iter().zip(&absorbed).filter(|(_, a)| a.is_none()).map(|(b, _)| b).collect()
}

/// A box after the stacked joins, and whether it is flat (a bar, or two bars fused into `=`),
/// which [`join_signs`] may put over or under a `+`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Part {
    pub b: Bx,
    pub flat: bool,
}

/// `empilhados.unir`: fuse the two halves of `:` `;` `=`. Short boxes and bars pair when one
/// sits over the other, half their width in common, close and not too tall together. Bars only
/// enter the text when paired; the unpaired ones come back for [`join_signs`].
pub(crate) fn join_stacked(boxes: &[Bx], bars: &[Bx], scale: i64) -> (Vec<Part>, Vec<Bx>) {
    let s = scale as f64;
    let plain = |v: &[Bx]| v.iter().map(|&b| Part { b, flat: false }).collect::<Vec<_>>();
    if scale <= 0 {
        return (plain(boxes), bars.to_vec());
    }
    // (is a bar, index in its list, box)
    let mut cand: Vec<(bool, usize, Bx)> = boxes.iter().enumerate().filter(|(_, b)| b.h() as f64 <= 0.55 * s).map(|(i, b)| (false, i, *b)).collect();
    cand.extend(bars.iter().enumerate().map(|(i, b)| (true, i, *b)));
    cand.sort_by_key(|c| (c.2.y0, c.2.x0));
    let matches = |a: &Bx, b: &Bx| -> bool {
        if a.y1 > b.y0 {
            return false;
        }
        let w = a.w().max(b.w());
        if w <= 0 || (a.common_x(b) as f64) / (w as f64) < 0.50 {
            return false;
        }
        (b.y0 - a.y1) as f64 <= 0.60 * s && (b.y1 - a.y0) as f64 <= 1.30 * s
    };
    let mut used = vec![false; cand.len()];
    let mut fused: Vec<Option<Part>> = vec![None; boxes.len()];
    let mut drop = vec![false; boxes.len()];
    let mut bar_used = vec![false; bars.len()];
    let mut extra = Vec::new();
    for i in 0..cand.len() {
        let Some(&(a_bar, a_i, a)) = cand.get(i) else { continue };
        if used.get(i).copied().unwrap_or(true) {
            continue;
        }
        for j in i + 1..cand.len() {
            let Some(&(b_bar, b_i, b)) = cand.get(j) else { continue };
            if used.get(j).copied().unwrap_or(true) || !matches(&a, &b) {
                continue;
            }
            for k in [i, j] {
                if let Some(u) = used.get_mut(k) {
                    *u = true;
                }
            }
            let u = Bx { x0: a.x0.min(b.x0), y0: a.y0, x1: a.x1.max(b.x1), y1: b.y1 };
            let flat = u.w() >= u.h() && (a_bar || b_bar || a.w() > a.h());
            let part = Part { b: u, flat };
            for (is_bar, idx) in [(a_bar, a_i), (b_bar, b_i)] {
                if is_bar && let Some(x) = bar_used.get_mut(idx) {
                    *x = true;
                }
            }
            match (a_bar, b_bar) {
                (false, false) => {
                    if let Some(f) = fused.get_mut(a_i) {
                        *f = Some(part);
                    }
                    if let Some(d) = drop.get_mut(b_i) {
                        *d = true;
                    }
                }
                (false, true) => {
                    if let Some(f) = fused.get_mut(a_i) {
                        *f = Some(part);
                    }
                }
                (true, false) => {
                    if let Some(d) = drop.get_mut(b_i) {
                        *d = true;
                    }
                    extra.push(part);
                }
                (true, true) => extra.push(part),
            }
            break;
        }
    }
    let mut out: Vec<Part> = Vec::with_capacity(boxes.len() + extra.len());
    for (i, b) in boxes.iter().enumerate() {
        if drop.get(i).copied().unwrap_or(false) {
            continue;
        }
        out.push(fused.get(i).copied().flatten().unwrap_or(Part { b: *b, flat: false }));
    }
    out.extend(extra);
    let left: Vec<Bx> = bars.iter().zip(&bar_used).filter(|(_, u)| !**u).map(|(b, _)| *b).collect();
    (out, left)
}

/// The two-part Informator signs `±` `∓` `⩲` `⩱`: a `+` over or under a bar or an `=`. The `+`
/// is too tall to be a half of a stacked glyph, so [`join_stacked`] leaves the bar unpaired and
/// the trunk dropped it (`∓` read `+`). Here a flat part pairs with the nearest box over or
/// under it of about its width, close, squarish and not taller than a scale.
pub(crate) fn join_signs(parts: Vec<Part>, bars: &[Bx], scale: i64) -> Vec<Bx> {
    let s = scale as f64;
    let mut parts = parts;
    if scale > 0 {
        parts.extend(bars.iter().map(|&b| Part { b, flat: true }));
    }
    let mut gone = vec![false; parts.len()];
    let mut out_of_bars = vec![false; parts.len()];
    if scale > 0 {
        for i in 0..parts.len() {
            let Some(f) = parts.get(i).copied() else { continue };
            if !f.flat || gone.get(i).copied().unwrap_or(true) {
                continue;
            }
            let mut best: Option<(usize, f64)> = None;
            for (j, p) in parts.iter().enumerate() {
                if j == i || gone.get(j).copied().unwrap_or(true) || p.flat {
                    continue;
                }
                let (pw, ph) = (p.b.w() as f64, p.b.h() as f64);
                if ph <= 0.0 || ph > s || !(0.6..=2.5).contains(&(pw / ph)) {
                    continue;
                }
                let wide = f.b.w().max(p.b.w()) as f64;
                if wide <= 0.0 || (f.b.common_x(&p.b) as f64) < 0.6 * wide {
                    continue;
                }
                let gap = if f.b.y1 <= p.b.y0 {
                    (p.b.y0 - f.b.y1) as f64
                } else if p.b.y1 <= f.b.y0 {
                    (f.b.y0 - p.b.y1) as f64
                } else {
                    continue;
                };
                if gap > 0.35 * s || (f.b.union(&p.b).h() as f64) > 1.4 * s {
                    continue;
                }
                if best.is_none_or(|(_, g)| gap < g) {
                    best = Some((j, gap));
                }
            }
            if let Some((j, _)) = best {
                if let Some(p) = parts.get_mut(j) {
                    p.b = p.b.union(&f.b);
                }
                if let Some(g) = gone.get_mut(i) {
                    *g = true;
                }
            } else if let Some(o) = out_of_bars.get_mut(i) {
                // An unpaired bar stays out of the text, as the trunk wants.
                *o = i >= parts.len() - bars.len();
            }
        }
    }
    parts.iter().zip(gone.iter().zip(&out_of_bars)).filter(|(_, (g, o))| !**g && !**o).map(|(p, _)| p.b).collect()
}

/// `linhas.bandas` + `ordem_em_faixa`: band by vertical overlap, then left to right.
pub(crate) fn order_in_strip(boxes: &[Bx]) -> Vec<Bx> {
    let short = median_height(boxes) * SHORT_BOX;
    let mut sorted = boxes.to_vec();
    sorted.sort_by_key(|b| b.y0);
    let mut groups: Vec<Vec<Bx>> = Vec::new();
    let mut cur: Vec<Bx> = Vec::new();
    let mut tall: Vec<i64> = Vec::new();
    for b in sorted {
        if !cur.is_empty() && !tall.is_empty() {
            let bottom = tall.iter().map(|&v| v as f64).sum::<f64>() / tall.len() as f64;
            if (b.y0 + b.y1) as f64 / 2.0 > bottom + b.h() as f64 * 0.2 {
                groups.push(std::mem::take(&mut cur));
                tall.clear();
            }
        }
        cur.push(b);
        if b.h() as f64 >= short {
            tall.push(b.y1);
        }
    }
    if !cur.is_empty() {
        groups.push(cur);
    }
    let mut out = Vec::with_capacity(boxes.len());
    for mut g in groups {
        g.sort_by_key(|b| b.x0);
        out.extend(g);
    }
    out
}

/// `linhas.quebrar_em_linhas` + `duas_linhas.descartar_fragmentos`: a new line when a box drops
/// below the baseline, goes back left or rises above the line; lines made mostly of fragments
/// (under 0.35 scale tall) are dropped.
pub(crate) fn break_lines(boxes: &[Bx], scale: i64) -> Vec<Vec<Bx>> {
    let med = median_height(boxes);
    let short = med * SHORT_BOX;
    let slack = med * 0.25;
    let mut lines: Vec<Vec<Bx>> = Vec::new();
    let mut cur: Vec<Bx> = Vec::new();
    let mut base: Option<i64> = None;
    for b in boxes {
        if let Some(prev) = cur.last() {
            let down = base.is_some_and(|bs| (b.y0 + b.y1) as f64 / 2.0 > bs as f64 + slack);
            let back = b.x0 < prev.x0 - prev.h();
            let top = cur.iter().map(|c| c.y0).min().unwrap_or(0);
            let up = (b.y1 as f64) < top as f64 - med;
            if down || back || up {
                lines.push(std::mem::take(&mut cur));
                base = None;
            }
        }
        cur.push(*b);
        if b.h() as f64 >= short {
            base = Some(base.map_or(b.y1, |v| v.max(b.y1)));
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    let floor = 0.35 * scale as f64;
    lines
        .into_iter()
        .filter(|l| {
            if scale <= 0 || l.is_empty() {
                return true;
            }
            let frag = l.iter().filter(|c| (c.h() as f64) < floor).count();
            (frag as f64) < l.len() as f64 * 0.7
        })
        .collect()
}

/// `b` shrunk to the rows and columns that hold ink (a half of a split box keeps the whole
/// box's height). `b` itself when it holds none.
pub(crate) fn tighten(bin: &Bin, b: &Bx) -> Bx {
    let mut t: Option<Bx> = None;
    for y in b.y0..b.y1 {
        for x in b.x0..b.x1 {
            if bin.at(x, y) {
                let p = Bx { x0: x, y0: y, x1: x + 1, y1: y + 1 };
                t = Some(t.map_or(p, |t| t.union(&p)));
            }
        }
    }
    t.unwrap_or(*b)
}

/// `colados.vale`: the column of least ink inside a box, away from its ends, when it is lighter
/// than the box's mean column.
pub(crate) fn valley(bin: &Bin, b: &Bx) -> Option<i64> {
    let w = usize::try_from(b.w()).ok()?;
    let h = b.h().max(1) as f64;
    if w < 3 {
        return None;
    }
    let prof: Vec<f64> = (0..b.w()).map(|dx| (b.y0..b.y1).filter(|&y| bin.at(b.x0 + dx, y)).count() as f64 / h).collect();
    let m = 1usize.max((w as f64 * 0.25) as usize);
    if m >= w.saturating_sub(m) {
        return None;
    }
    let mid = prof.get(m..w - m)?;
    let mean = prof.iter().sum::<f64>() / w as f64;
    let (arg, min) = mid.iter().enumerate().fold((0usize, f64::INFINITY), |acc, (i, &v)| if v < acc.1 { (i, v) } else { acc });
    if min >= mean {
        return None;
    }
    Some(b.x0 + (m + arg) as i64)
}
