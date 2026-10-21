//! Where a separate app window opens (wave 10, U5): its saved place on the desktop, the monitors
//! there are now, and a place beside the main window when the saved one can't be used.
//!
//! Everything here is in **physical pixels of the desktop**, the one coordinate system in which
//! monitors with different scaling sit side by side (egui's points are per window: a point is
//! more pixels on a 150% monitor than on a 100% one). A window's place is saved as its outer
//! rectangle in pixels, the scale it had (its monitor's pixels per point, without the app's
//! zoom), and the monitor it was on; its size is saved separately, as the inner size in points,
//! which is what the window's contents get and does not change with the monitor.
//!
//! egui doesn't list monitors; the desktop app reads them from winit through eframe's main
//! window ([`read_monitors`]). Without them (the web, tests) only the main window is known, and a
//! window opens inside it, on its right, where it is surely visible.
//!
//! The rules ([`target`]):
//! 1. The saved place is used on the monitor it was on, when that monitor is still there: the
//!    same one in the same place, or one of the same size and scale that moved (the window moves
//!    with it). Failing that, the monitor most of the saved rectangle is on, if at least 48 × 48
//!    points of it are.
//! 2. On that monitor the window is moved and shrunk until it is wholly on it: a window is never
//!    opened partly or fully off every monitor.
//! 3. Otherwise it opens beside the main window, on the main window's monitor: to its right if
//!    there is room, else to its left, else over its right part; wholly on that monitor.
//!
//! Saved values are untrusted: every number must be finite and within bounds, or the whole place
//! is ignored ([`Place::from_json`]).

use egui::{Pos2, Rect, Vec2, pos2, vec2};
use serde_json::{Value, json};

/// Coordinates and sizes beyond this many pixels are not believed (eight 8K monitors side by side
/// are 61,440 pixels wide).
const MAX_PX: f32 = 65_536.0;
/// Scales outside these bounds are not believed (Windows goes from 100% to 500%).
const MIN_SCALE: f32 = 0.25;
const MAX_SCALE: f32 = 8.0;
/// At most this many monitors are read.
#[cfg(not(target_arch = "wasm32"))]
const MAX_MONITORS: usize = 16;
/// Part of a saved window that must be on a monitor for its place to be used, in points.
const ON_SCREEN: Vec2 = vec2(48.0, 48.0);
/// What a window adds around its inner size (frame and title bar), in points: an estimate,
/// generous so that a window placed with it is wholly on its monitor.
pub const CHROME: Vec2 = vec2(16.0, 40.0);
/// The gap between the main window and a window placed beside it, in points.
const GAP: f32 = 8.0;
/// A window opened inside the main window, without monitors to go by, is this far below its top
/// (its title bar and toolbar stay in view), in points.
const INSIDE_TOP: f32 = 96.0;

/// A monitor: its rectangle on the desktop, in physical pixels, and its scale (pixels per point,
/// without the app's zoom).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Monitor {
    pub rect: Rect,
    pub scale: f32,
}

impl Monitor {
    /// A monitor at `[x, y]` of `[w, h]` pixels: `None` unless every number is usable.
    pub fn new(x: f32, y: f32, w: f32, h: f32, scale: f32) -> Option<Monitor> {
        let rect = px_rect([x, y, w, h])?;
        Some(Monitor { rect, scale: usable_scale(scale)? })
    }

    /// `{rect: [x, y, w, h], scale}`.
    pub fn to_json(self) -> Value {
        json!({ "rect": rect_json(self.rect), "scale": self.scale })
    }

    fn from_json(v: &Value) -> Option<Monitor> {
        let [x, y, w, h] = numbers::<4>(v.get("rect"))?;
        Monitor::new(x, y, w, h, number(v.get("scale"))?)
    }

    /// The same size and scale: the same monitor, maybe moved.
    fn same_kind(&self, other: &Monitor) -> bool {
        (self.rect.size() - other.rect.size()).abs().max_elem() < 0.5 && (self.scale - other.scale).abs() < 1e-3
    }
}

/// Where a window was: its outer rectangle on the desktop, in physical pixels, its scale then, and
/// the monitor it was on (when monitors were known).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub outer: Rect,
    pub scale: f32,
    pub monitor: Option<Monitor>,
}

impl Place {
    /// The place of a window whose outer rectangle is `outer` points at `pixels_per_point`, with
    /// `scale` its monitor's (pixels per point without the zoom), among `monitors`. `None` unless
    /// the numbers are usable.
    pub fn of(outer: Rect, pixels_per_point: f32, scale: f32, monitors: &[Monitor]) -> Option<Place> {
        if !(pixels_per_point.is_finite() && pixels_per_point > 0.0) {
            return None;
        }
        let px = Rect::from_min_size(outer.min * pixels_per_point, outer.size() * pixels_per_point);
        let outer = px_rect([px.min.x, px.min.y, px.width(), px.height()])?;
        let monitor = most_of(outer, monitors).map(|(m, _)| *m);
        Some(Place { outer, scale: usable_scale(scale)?, monitor })
    }

    /// `{outer: [x, y, w, h], scale, monitor: {rect: [x, y, w, h], scale} | null}`.
    pub fn to_json(self) -> Value {
        json!({ "outer": rect_json(self.outer), "scale": self.scale, "monitor": self.monitor.map(Monitor::to_json) })
    }

    /// Saved settings, as written by [`Place::to_json`]: `None` if the rectangle or the scale isn't
    /// usable. A monitor that isn't usable is left out (the place is then matched by where it is).
    pub fn from_json(v: &Value) -> Option<Place> {
        let outer = px_rect(numbers::<4>(v.get("outer"))?)?;
        let scale = usable_scale(number(v.get("scale"))?)?;
        Some(Place { outer, scale, monitor: v.get("monitor").and_then(Monitor::from_json) })
    }
}

/// The main window: its outer rectangle on the desktop, in physical pixels, and its scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MainWindow {
    pub outer: Rect,
    pub scale: f32,
}

impl MainWindow {
    /// The main window whose outer rectangle is `outer` points at `pixels_per_point`.
    pub fn of(outer: Rect, pixels_per_point: f32, scale: f32) -> Option<MainWindow> {
        Place::of(outer, pixels_per_point, scale, &[]).map(|p| MainWindow { outer: p.outer, scale: p.scale })
    }
}

/// Where to open a window: its outer top-left corner on the desktop in physical pixels, the scale
/// of the monitor that is on, and its inner size in points (shrunk to fit that monitor).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Target {
    pub pos: Pos2,
    pub scale: f32,
    pub inner: Vec2,
}

impl Target {
    /// The position to give egui (`ViewportBuilder::with_position`, `ViewportCommand::OuterPosition`),
    /// in points of a window at `pixels_per_point`.
    pub fn pos_in_points(&self, pixels_per_point: f32) -> Option<Pos2> {
        let p = self.pos / pixels_per_point;
        (pixels_per_point.is_finite() && pixels_per_point > 0.0 && p.is_finite()).then_some(p)
    }

    pub fn to_json(self) -> Value {
        json!({ "pos": [self.pos.x, self.pos.y], "scale": self.scale, "inner": [self.inner.x, self.inner.y] })
    }
}

/// Where a window of inner size `inner` (points, at least `min_inner`) opens, at the app's `zoom`:
/// at its `saved` place if that is on a monitor there is now, else beside the main window (see the
/// module's rules). `None` when neither monitors nor the main window are known: the system places
/// it.
pub fn target(saved: Option<&Place>, inner: Vec2, min_inner: Vec2, zoom: f32, monitors: &[Monitor], main: Option<&MainWindow>) -> Option<Target> {
    let zoom = if zoom.is_finite() && zoom > 0.0 { zoom } else { 1.0 };
    let inner = if inner.is_finite() { inner.max(min_inner) } else { min_inner };
    if monitors.is_empty() {
        // Only the main window is known, and it is on screen: inside it, on its right.
        let main = main?;
        return Some(inside(main.outer, main.scale, inner, min_inner, zoom));
    }
    if let Some((monitor, outer)) = saved.and_then(|s| saved_monitor(s, monitors, zoom)) {
        let fitted = fit_inner(inner, min_inner, monitor, zoom);
        let size = outer_px(fitted, monitor.scale, zoom).min(monitor.rect.size());
        return Some(Target { pos: clamp_into(outer.min, size, monitor.rect), scale: monitor.scale, inner: fitted });
    }
    // Beside the main window, on its monitor (the first one when it isn't known).
    let monitor = main.and_then(|m| most_of(m.outer, monitors)).map(|(m, _)| m).or_else(|| monitors.first())?;
    let fitted = fit_inner(inner, min_inner, monitor, zoom);
    let size = outer_px(fitted, monitor.scale, zoom).min(monitor.rect.size());
    let area = monitor.rect;
    let Some(main) = main else {
        return Some(Target { pos: clamp_into(area.center() - size / 2.0, size, area), scale: monitor.scale, inner: fitted });
    };
    let gap = GAP * monitor.scale * zoom;
    let x = if main.outer.right() + gap + size.x <= area.right() {
        main.outer.right() + gap
    } else if main.outer.left() - gap - size.x >= area.left() {
        main.outer.left() - gap - size.x
    } else {
        area.right() - size.x
    };
    Some(Target { pos: clamp_into(pos2(x, main.outer.top()), size, area), scale: monitor.scale, inner: fitted })
}

/// The monitor a saved place goes on, and the place's outer rectangle there (moved with its
/// monitor): rule 1.
fn saved_monitor<'a>(saved: &Place, monitors: &'a [Monitor], zoom: f32) -> Option<(&'a Monitor, Rect)> {
    if let Some(was) = &saved.monitor {
        if let Some(m) = monitors.iter().find(|m| *m == was) {
            return Some((m, saved.outer));
        }
        if let Some(m) = monitors.iter().find(|m| m.same_kind(was)) {
            return Some((m, saved.outer.translate(m.rect.min - was.rect.min)));
        }
    }
    let (m, on) = most_of(saved.outer, monitors)?;
    let needed = ON_SCREEN * m.scale * zoom;
    (on.x >= needed.x && on.y >= needed.y).then_some((m, saved.outer))
}

/// The monitor most of `rect` is on, and how much of it is there (width and height of the
/// overlap), if any is.
fn most_of(rect: Rect, monitors: &[Monitor]) -> Option<(&Monitor, Vec2)> {
    monitors
        .iter()
        .filter_map(|m| {
            let on = m.rect.intersect(rect);
            (on.is_positive()).then(|| (m, on.size()))
        })
        .max_by(|(_, a), (_, b)| (a.x * a.y).total_cmp(&(b.x * b.y)))
}

/// A window inside the main window `area` (pixels, at `scale`), on its right below its top.
fn inside(area: Rect, scale: f32, inner: Vec2, min_inner: Vec2, zoom: f32) -> Target {
    let ppp = scale * zoom;
    let room = (area.size() / ppp - CHROME).max(min_inner);
    let fitted = inner.min(room).max(min_inner);
    let size = outer_px(fitted, scale, zoom).min(area.size());
    let pos = pos2(area.right() - size.x - GAP * ppp, area.top() + INSIDE_TOP * ppp);
    Target { pos: clamp_into(pos, size, area), scale, inner: fitted }
}

/// `inner` shrunk so the window fits `monitor`, never below `min_inner`.
fn fit_inner(inner: Vec2, min_inner: Vec2, monitor: &Monitor, zoom: f32) -> Vec2 {
    let room = monitor.rect.size() / (monitor.scale * zoom) - CHROME;
    inner.min(room.max(min_inner)).max(min_inner)
}

/// A window's outer size in pixels for an inner size in points.
fn outer_px(inner: Vec2, scale: f32, zoom: f32) -> Vec2 {
    (inner + CHROME) * scale * zoom
}

/// The top-left corner that puts a `size` rectangle at `pos` wholly in `area`, as near `pos` as
/// it can (`size` is at most `area`'s; if not, the top-left corner of `area`).
fn clamp_into(pos: Pos2, size: Vec2, area: Rect) -> Pos2 {
    // Not `clamp`, which panics when the bounds cross (a size larger than the area).
    let x = pos.x.min(area.right() - size.x).max(area.left());
    let y = pos.y.min(area.bottom() - size.y).max(area.top());
    pos2(x, y)
}

fn usable_scale(scale: f32) -> Option<f32> {
    (scale.is_finite() && (MIN_SCALE..=MAX_SCALE).contains(&scale)).then_some(scale)
}

/// A rectangle `[x, y, w, h]` in pixels: finite, within ±65,536, at least one pixel wide and high.
fn px_rect([x, y, w, h]: [f32; 4]) -> Option<Rect> {
    let ok = [x, y, w, h].iter().all(|v| v.is_finite() && v.abs() <= MAX_PX) && w >= 1.0 && h >= 1.0;
    ok.then(|| Rect::from_min_size(pos2(x, y), vec2(w, h)))
}

fn rect_json(r: Rect) -> Value {
    json!([r.min.x, r.min.y, r.width(), r.height()])
}

fn number(v: Option<&Value>) -> Option<f32> {
    v?.as_f64().map(|n| n as f32).filter(|n| n.is_finite())
}

/// `N` finite numbers.
fn numbers<const N: usize>(v: Option<&Value>) -> Option<[f32; N]> {
    let list = v?.as_array()?;
    if list.len() != N {
        return None;
    }
    let mut out = [0.0; N];
    for (o, v) in out.iter_mut().zip(list) {
        *o = number(Some(v))?;
    }
    Some(out)
}

/// The monitors of the desktop, as winit reports them through eframe's main window: `None`
/// without one (tests, a host without winit). At most [`MAX_MONITORS`]; unusable ones are left out.
#[cfg(not(target_arch = "wasm32"))]
pub fn read_monitors(frame: &eframe::Frame) -> Option<Vec<Monitor>> {
    let window = frame.winit_window()?;
    let monitors = window
        .available_monitors()
        .take(MAX_MONITORS)
        .filter_map(|m| {
            let (p, s) = (m.position(), m.size());
            Monitor::new(p.x as f32, p.y as f32, s.width as f32, s.height as f32, m.scale_factor() as f32)
        })
        .collect();
    Some(monitors)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: Vec2 = vec2(320.0, 360.0);

    /// This machine's two monitors (the spec's §12b U5): 1920 × 1080 at 100% on the left, the
    /// primary, and 3440 × 1440 at 150% to its right.
    fn two() -> [Monitor; 2] {
        [Monitor::new(0.0, 0.0, 1920.0, 1080.0, 1.0).unwrap(), Monitor::new(1920.0, -200.0, 3440.0, 1440.0, 1.5).unwrap()]
    }

    fn main_on_first() -> MainWindow {
        MainWindow { outer: Rect::from_min_size(pos2(100.0, 50.0), vec2(1200.0, 900.0)), scale: 1.0 }
    }

    fn fully_on(t: &Target, monitor: &Monitor) -> bool {
        let r = Rect::from_min_size(t.pos, outer_px(t.inner, t.scale, 1.0));
        monitor.rect.contains_rect(r)
    }

    #[test]
    fn a_saved_place_comes_back_on_its_monitor_and_moves_with_it() {
        let [left, right] = two();
        let monitors = [left, right];
        let saved = Place { outer: Rect::from_min_size(pos2(2400.0, 0.0), vec2(1464.0, 1200.0)), scale: 1.5, monitor: Some(right) };
        let t = target(Some(&saved), vec2(960.0, 760.0), MIN, 1.0, &monitors, Some(&main_on_first())).unwrap();
        assert_eq!((t.pos, t.scale, t.inner), (pos2(2400.0, 0.0), 1.5, vec2(960.0, 760.0)));
        // The big monitor now on the left of the small one: the window goes with it.
        let moved = Monitor::new(-3440.0, 0.0, 3440.0, 1440.0, 1.5).unwrap();
        let t = target(Some(&saved), vec2(960.0, 760.0), MIN, 1.0, &[left, moved], Some(&main_on_first())).unwrap();
        assert_eq!(t.pos, pos2(2400.0 - 1920.0 - 3440.0, 200.0));
        assert!(fully_on(&t, &moved));
        // A window that was partly off its monitor comes back wholly on it.
        let partly = Place { outer: Rect::from_min_size(pos2(5000.0, 900.0), vec2(1500.0, 1200.0)), ..saved };
        let t = target(Some(&partly), vec2(960.0, 760.0), MIN, 1.0, &monitors, None).unwrap();
        assert!(fully_on(&t, &right), "{t:?}");
        assert_eq!(t.scale, 1.5);
    }

    #[test]
    fn a_place_off_every_monitor_opens_beside_the_main_window_fully_visible() {
        let monitors = two();
        let main = main_on_first();
        // The big monitor is gone (saved on it, now only the small one): beside the main window.
        let saved = Place { outer: Rect::from_min_size(pos2(2400.0, 100.0), vec2(1500.0, 1200.0)), scale: 1.5, monitor: Some(monitors[1]) };
        for place in
            [Some(saved), None, Some(Place { outer: Rect::from_min_size(pos2(-30000.0, 9000.0), vec2(800.0, 600.0)), scale: 1.0, monitor: None })]
        {
            let t = target(place.as_ref(), vec2(560.0, 700.0), MIN, 1.0, &monitors[..1], Some(&main)).unwrap();
            assert!(fully_on(&t, &monitors[0]), "{place:?}: {t:?}");
            assert_eq!(t.pos.x, main.outer.right() + GAP, "to the main window's right: {t:?}");
            assert_eq!(t.inner, vec2(560.0, 700.0));
        }
        // Only a sliver of the saved window on a monitor: not enough to be used.
        let sliver = Place { outer: Rect::from_min_size(pos2(1900.0, 1060.0), vec2(800.0, 600.0)), scale: 1.0, monitor: None };
        let t = target(Some(&sliver), vec2(560.0, 700.0), MIN, 1.0, &monitors[..1], Some(&main)).unwrap();
        assert_eq!(t.pos.x, main.outer.right() + GAP);
        // A maximized main window: over its right part, still wholly on the monitor.
        let full = MainWindow { outer: monitors[0].rect, scale: 1.0 };
        let t = target(None, vec2(960.0, 760.0), MIN, 1.0, &monitors[..1], Some(&full)).unwrap();
        assert!(fully_on(&t, &monitors[0]), "{t:?}");
        assert_eq!(t.pos.x + outer_px(t.inner, 1.0, 1.0).x, 1920.0);
        // Too big for the monitor: shrunk to fit it.
        let t = target(None, vec2(4000.0, 4000.0), MIN, 1.0, &monitors[..1], Some(&full)).unwrap();
        assert!(fully_on(&t, &monitors[0]), "{t:?}");
        assert!(t.inner.x <= 1920.0 && t.inner.y <= 1080.0);
        // Nothing known: the system places it; only the main window known: inside it.
        assert_eq!(target(Some(&saved), vec2(960.0, 760.0), MIN, 1.0, &[], None), None);
        let t = target(Some(&saved), vec2(960.0, 760.0), MIN, 1.0, &[], Some(&main)).unwrap();
        assert!(main.outer.contains_rect(Rect::from_min_size(t.pos, outer_px(t.inner, 1.0, 1.0))), "{t:?}");
    }

    #[test]
    fn bad_saved_values_are_ignored_and_nothing_panics() {
        let place = Place { outer: Rect::from_min_size(pos2(-1700.0, 20.0), vec2(1500.0, 1200.0)), scale: 1.25, monitor: two().first().copied() };
        assert_eq!(Place::from_json(&place.to_json()), Some(place));
        for bad in [
            json!({"outer": [f64::NAN, 0.0, 10.0, 10.0], "scale": 1.0}),
            json!({"outer": [0.0, 0.0, 10.0, 10.0], "scale": 0.0}),
            json!({"outer": [0.0, 0.0, 10.0, 10.0], "scale": 1e9}),
            json!({"outer": [1e12, 0.0, 10.0, 10.0], "scale": 1.0}),
            json!({"outer": [0.0, 0.0, 0.0, 10.0], "scale": 1.0}),
            json!({"outer": [0.0, 0.0, 10.0], "scale": 1.0}),
            json!({"outer": "here", "scale": 1.0}),
            json!({"scale": 1.0}),
            json!(null),
            json!([1, 2, 3, 4]),
        ] {
            assert_eq!(Place::from_json(&bad), None, "{bad}");
        }
        // A bad monitor is left out; the place is kept.
        let v = json!({"outer": [10.0, 10.0, 800.0, 600.0], "scale": 1.0, "monitor": {"rect": [0, 0, -5, 5], "scale": 1.0}});
        assert_eq!(Place::from_json(&v).and_then(|p| p.monitor), None);
        assert!(Place::from_json(&v).is_some());
        // Unusable sizes, zooms and windows never panic and always give a usable target.
        let monitors = two();
        for (inner, zoom) in [(vec2(f32::NAN, 1.0), 1.0), (vec2(1e30, 1e30), 1.0), (vec2(0.0, -5.0), f32::INFINITY), (vec2(500.0, 500.0), 0.0)] {
            let t = target(None, inner, MIN, zoom, &monitors, Some(&main_on_first())).unwrap();
            assert!(t.pos.is_finite() && t.inner.is_finite() && t.inner.x >= MIN.x && t.inner.y >= MIN.y, "{inner:?} {zoom}: {t:?}");
        }
        // A monitor smaller than the smallest window: on it, at its corner.
        let tiny = Monitor::new(0.0, 0.0, 200.0, 100.0, 1.0).unwrap();
        let t = target(None, vec2(960.0, 760.0), MIN, 1.0, &[tiny], None).unwrap();
        assert_eq!(t.pos, pos2(0.0, 0.0));
        assert_eq!(Monitor::new(0.0, 0.0, 10.0, 10.0, f32::NAN), None);
        assert_eq!(MainWindow::of(Rect::from_min_size(Pos2::ZERO, vec2(10.0, 10.0)), 0.0, 1.0), None);
        assert_eq!(Target { pos: pos2(300.0, 150.0), scale: 1.5, inner: MIN }.pos_in_points(1.5), Some(pos2(200.0, 100.0)));
        assert_eq!(Target { pos: pos2(300.0, 150.0), scale: 1.5, inner: MIN }.pos_in_points(0.0), None);
    }

    #[test]
    fn a_window_place_is_measured_in_pixels_of_the_desktop() {
        let monitors = two();
        // On the 150% monitor, at 1.5 pixels per point: points × 1.5.
        let p = Place::of(Rect::from_min_size(pos2(1600.0, -100.0), vec2(976.0, 800.0)), 1.5, 1.5, &monitors).unwrap();
        assert_eq!(p.outer, Rect::from_min_size(pos2(2400.0, -150.0), vec2(1464.0, 1200.0)));
        assert_eq!(p.monitor, Some(monitors[1]));
        // And back where it was, through the settings.
        let back = Place::from_json(&p.to_json()).unwrap();
        let t = target(Some(&back), vec2(960.0, 760.0), MIN, 1.0, &monitors, None).unwrap();
        assert_eq!(t.pos, pos2(2400.0, -150.0));
        assert_eq!(t.pos_in_points(1.5), Some(pos2(1600.0, -100.0)));
    }
}
