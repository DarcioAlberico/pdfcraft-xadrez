//! The one place that converts diagram boxes between PDF user space, view space and pixels
//! (blueprint S7a, §2 "Coordenadas").
//!
//! - **User space** ([`PageRect`]): PDF points, y up, as the content stream draws. Page images
//!   (`edit::PageImage::rect`), link annotations and every `DiagramCandidate::bbox_user` live
//!   here.
//! - **View space** ([`ViewRect`]): points, y down, origin at the top-left corner of the page as
//!   displayed, that is the crop box after `/Rotate` (clockwise) and scaled by `/UserUnit`.
//!   `render::TextGlyph::rect` and rendered page pixels live here. The formulas are the ones
//!   `render::PageInfo::{user_to_view, view_to_user}` use, so the two crates agree; this crate
//!   does not depend on `render` for them.
//! - **Pixels** ([`PixelRect`]): a raster of view space at an explicit DPI, or a raster of one
//!   region of it.
//!
//! `edit`'s display space (what `Edit::AddText` takes) is view space with y flipped up from the
//! bottom edge; its conversion belongs here too when a step first needs it (blueprint S10).
//!
//! The geometry primitives come from `pdfcraft-geom` (`PageRect`, `Rotation`); what is added
//! here is the transform, which geom does not have.

pub use pdfcraft_geom::{PageRect, Rotation};

/// Points per inch: PDF user space units at `/UserUnit 1`.
pub const POINTS_PER_INCH: f32 = 72.0;

/// A rectangle in view space: points, y down, origin at the displayed page's top-left corner.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct ViewRect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl ViewRect {
    /// A rectangle from two corners in any order.
    pub fn new(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        Self { x0: x0.min(x1), y0: y0.min(y1), x1: x0.max(x1), y1: y0.max(y1) }
    }

    pub fn width(&self) -> f32 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> f32 {
        self.y1 - self.y0
    }
}

/// A box of pixels, half-open: columns `x0..x1`, rows `y0..y1`, top row first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct PixelRect {
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
}

impl PixelRect {
    pub fn width(&self) -> u32 {
        self.x1.saturating_sub(self.x0)
    }

    pub fn height(&self) -> u32 {
        self.y1.saturating_sub(self.y0)
    }
}

/// Where a page's visible area sits in user space and how it is turned for display: what every
/// conversion between user space and view space needs.
///
/// Built only through [`PageGeometry::new`], so the crop box is always finite with a positive
/// area and the conversions never divide by zero.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageGeometry {
    crop_box: PageRect,
    rotation: Rotation,
    user_unit: f32,
}

impl PageGeometry {
    /// A page whose visible region is `crop_box` (user space: the effective crop box, already
    /// intersected with the media box by whoever read the page) turned clockwise by `rotation`.
    /// `None` when the box is not finite or has no area.
    pub fn new(crop_box: PageRect, rotation: Rotation) -> Option<Self> {
        let crop_box = finite_page_rect(crop_box)?;
        (crop_box.width() > 0.0 && crop_box.height() > 0.0).then_some(Self { crop_box, rotation, user_unit: 1.0 })
    }

    /// The same page with `/UserUnit` set (the size of one user unit, in 1/72 inch). `None`
    /// when `unit` is not a finite positive number.
    pub fn with_user_unit(self, unit: f32) -> Option<Self> {
        (unit.is_finite() && unit > 0.0).then_some(Self { user_unit: unit, ..self })
    }

    pub fn crop_box(&self) -> PageRect {
        self.crop_box
    }

    pub fn rotation(&self) -> Rotation {
        self.rotation
    }

    pub fn user_unit(&self) -> f32 {
        self.user_unit
    }

    /// The displayed page size in view-space points `(width, height)`.
    pub fn view_size(&self) -> (f32, f32) {
        let (w, h) = (self.crop_box.width() * self.user_unit, self.crop_box.height() * self.user_unit);
        if self.rotation.swaps_axes() { (h, w) } else { (w, h) }
    }

    /// The crop box's width and height in user units, as f64.
    fn crop_size(&self) -> (f64, f64) {
        (f64::from(self.crop_box.width()), f64::from(self.crop_box.height()))
    }

    /// A user-space point in view space. `None` for a non-finite input or result.
    pub fn user_to_view_point(&self, x: f32, y: f32) -> Option<[f32; 2]> {
        if !(x.is_finite() && y.is_finite()) {
            return None;
        }
        let (w, h) = self.crop_size();
        // Offsets from the crop box's bottom-left corner, in user units.
        let (dx, dy) = (f64::from(x) - f64::from(self.crop_box.x0), f64::from(y) - f64::from(self.crop_box.y0));
        // Unturned, view y runs down from the crop box's top; each quarter turn is clockwise.
        let (vx, vy) = match self.rotation {
            Rotation::R0 => (dx, h - dy),
            Rotation::R90 => (dy, dx),
            Rotation::R180 => (w - dx, dy),
            Rotation::R270 => (h - dy, w - dx),
        };
        let unit = f64::from(self.user_unit);
        finite_point(vx * unit, vy * unit)
    }

    /// A view-space point in user space. `None` for a non-finite input or result.
    pub fn view_to_user_point(&self, x: f32, y: f32) -> Option<[f32; 2]> {
        if !(x.is_finite() && y.is_finite()) {
            return None;
        }
        let (w, h) = self.crop_size();
        // `user_unit` is finite and positive (checked when it was set), so this divides safely.
        let unit = f64::from(self.user_unit);
        let (vx, vy) = (f64::from(x) / unit, f64::from(y) / unit);
        // The inverse of each turn in `user_to_view_point`.
        let (dx, dy) = match self.rotation {
            Rotation::R0 => (vx, h - vy),
            Rotation::R90 => (vy, vx),
            Rotation::R180 => (w - vx, vy),
            Rotation::R270 => (w - vy, h - vx),
        };
        finite_point(f64::from(self.crop_box.x0) + dx, f64::from(self.crop_box.y0) + dy)
    }

    /// A user-space box in view space. `None` for a non-finite box.
    pub fn user_to_view(&self, rect: PageRect) -> Option<ViewRect> {
        let [ax, ay] = self.user_to_view_point(rect.x0, rect.y0)?;
        let [bx, by] = self.user_to_view_point(rect.x1, rect.y1)?;
        finite_view_rect(ViewRect::new(ax, ay, bx, by))
    }

    /// A view-space box in user space. `None` for a non-finite box.
    pub fn view_to_user(&self, rect: ViewRect) -> Option<PageRect> {
        let [ax, ay] = self.view_to_user_point(rect.x0, rect.y0)?;
        let [bx, by] = self.view_to_user_point(rect.x1, rect.y1)?;
        finite_page_rect(PageRect::new(ax, ay, bx, by))
    }
}

/// `rect` normalised, or `None` when a coordinate, its width or its height is not finite.
/// `PageRect`'s fields are public, so a box may arrive with NaN in it or its corners swapped.
pub fn finite_page_rect(rect: PageRect) -> Option<PageRect> {
    let finite = [rect.x0, rect.y0, rect.x1, rect.y1].iter().all(|v| v.is_finite());
    let rect = PageRect::new(rect.x0, rect.y0, rect.x1, rect.y1);
    // The corners are finite, but their difference can still overflow to infinity.
    (finite && rect.width().is_finite() && rect.height().is_finite()).then_some(rect)
}

/// The view-space twin of [`finite_page_rect`].
fn finite_view_rect(rect: ViewRect) -> Option<ViewRect> {
    let finite = [rect.x0, rect.y0, rect.x1, rect.y1].iter().all(|v| v.is_finite());
    let rect = ViewRect::new(rect.x0, rect.y0, rect.x1, rect.y1);
    (finite && rect.width().is_finite() && rect.height().is_finite()).then_some(rect)
}

/// An f64 point as f32, or `None` when either coordinate is not finite once narrowed.
fn finite_point(x: f64, y: f64) -> Option<[f32; 2]> {
    // `as` turns out-of-range values into infinity, which the check below refuses.
    let (x, y) = (x as f32, y as f32);
    (x.is_finite() && y.is_finite()).then_some([x, y])
}

/// Points per pixel at `dpi`, or `None` for a DPI that is not finite and positive.
fn points_per_pixel(dpi: f32) -> Option<f64> {
    (dpi.is_finite() && dpi > 0.0).then(|| f64::from(POINTS_PER_INCH) / f64::from(dpi))
}

/// A finite f64 rounded by `round` to a pixel coordinate: negatives clamp to 0 and values past
/// `u32::MAX` saturate.
fn to_pixel(value: f64, round: fn(f64) -> f64) -> Option<u32> {
    let value = round(value).max(0.0);
    // NaN fails `is_finite`; the value is then in 0..=u32::MAX, so the cast is exact or rounds.
    value.is_finite().then_some(value.min(f64::from(u32::MAX)) as u32)
}

/// `pt` points at `dpi` dots per inch, in pixels. `None` for a non-finite or non-positive DPI.
pub fn pt_to_px(pt: f32, dpi: f32) -> Option<f32> {
    let px = (f64::from(pt) / points_per_pixel(dpi)?) as f32;
    px.is_finite().then_some(px)
}

/// `px` pixels at `dpi` dots per inch, in points. `None` for a non-finite or non-positive DPI.
pub fn px_to_pt(px: f32, dpi: f32) -> Option<f32> {
    let pt = (f64::from(px) * points_per_pixel(dpi)?) as f32;
    pt.is_finite().then_some(pt)
}

/// A pixel box of a whole-page raster rendered at `dpi`, in view space.
pub fn pixels_to_view(rect: PixelRect, dpi: f32) -> Option<ViewRect> {
    let scale = points_per_pixel(dpi)?;
    let [x0, y0] = finite_point(f64::from(rect.x0) * scale, f64::from(rect.y0) * scale)?;
    let [x1, y1] = finite_point(f64::from(rect.x1) * scale, f64::from(rect.y1) * scale)?;
    Some(ViewRect::new(x0, y0, x1, y1))
}

/// The smallest pixel box of a whole-page raster at `dpi` that covers a view-space box. `None`
/// for a non-finite box or DPI; parts left of or above the page clamp to pixel 0.
pub fn view_to_pixels(rect: ViewRect, dpi: f32) -> Option<PixelRect> {
    let rect = finite_view_rect(rect)?;
    let scale = points_per_pixel(dpi)?;
    let px = |v: f32| f64::from(v) / scale;
    Some(PixelRect {
        x0: to_pixel(px(rect.x0), f64::floor)?,
        y0: to_pixel(px(rect.y0), f64::floor)?,
        x1: to_pixel(px(rect.x1), f64::ceil)?,
        y1: to_pixel(px(rect.y1), f64::ceil)?,
    })
}

/// A pixel box of a `raster_width` × `raster_height` raster that shows exactly `region` of view
/// space, in view space. Each axis has its own scale, so the raster may have been rendered at
/// any DPI, even a different one across and down. `None` for an empty raster or a non-finite
/// region.
pub fn region_pixels_to_view(region: ViewRect, raster_width: u32, raster_height: u32, rect: PixelRect) -> Option<ViewRect> {
    let region = finite_view_rect(region)?;
    if raster_width == 0 || raster_height == 0 {
        return None;
    }
    let sx = f64::from(region.width()) / f64::from(raster_width);
    let sy = f64::from(region.height()) / f64::from(raster_height);
    let (ox, oy) = (f64::from(region.x0), f64::from(region.y0));
    let [x0, y0] = finite_point(ox + f64::from(rect.x0) * sx, oy + f64::from(rect.y0) * sy)?;
    let [x1, y1] = finite_point(ox + f64::from(rect.x1) * sx, oy + f64::from(rect.y1) * sy)?;
    Some(ViewRect::new(x0, y0, x1, y1))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Rotation; 4] = [Rotation::R0, Rotation::R90, Rotation::R180, Rotation::R270];

    /// A crop box offset from a US Letter media box (0 0 612 792), so every formula that forgets
    /// the crop origin is off by (100, 50).
    fn cropped(rotation: Rotation) -> PageGeometry {
        PageGeometry::new(PageRect::new(100.0, 50.0, 500.0, 750.0), rotation).unwrap()
    }

    fn close(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
    }

    fn close_rect(a: PageRect, b: PageRect) -> bool {
        close([a.x0, a.y0], [b.x0, b.y0]) && close([a.x1, a.y1], [b.x1, b.y1])
    }

    #[test]
    fn view_size_swaps_on_quarter_turns_and_scales_by_user_unit() {
        assert_eq!(cropped(Rotation::R0).view_size(), (400.0, 700.0));
        assert_eq!(cropped(Rotation::R90).view_size(), (700.0, 400.0));
        assert_eq!(cropped(Rotation::R180).view_size(), (400.0, 700.0));
        assert_eq!(cropped(Rotation::R270).view_size(), (700.0, 400.0));
        assert_eq!(cropped(Rotation::R90).with_user_unit(2.0).unwrap().view_size(), (1400.0, 800.0));
    }

    /// The crop box's corners land on the displayed page's corners as a clockwise turn puts
    /// them: the user-space top-left corner (100, 750) is the view's top-left unturned, its
    /// top-right after 90°, its bottom-right after 180° and its bottom-left after 270°.
    #[test]
    fn corners_follow_a_clockwise_turn() {
        let top_left = [100.0, 750.0];
        let bottom_left = [100.0, 50.0];
        let expected = [
            (Rotation::R0, [0.0, 0.0], [0.0, 700.0]),
            (Rotation::R90, [700.0, 0.0], [0.0, 0.0]),
            (Rotation::R180, [400.0, 700.0], [400.0, 0.0]),
            (Rotation::R270, [0.0, 400.0], [700.0, 400.0]),
        ];
        for (rotation, tl, bl) in expected {
            let page = cropped(rotation);
            let got_tl = page.user_to_view_point(top_left[0], top_left[1]).unwrap();
            let got_bl = page.user_to_view_point(bottom_left[0], bottom_left[1]).unwrap();
            assert!(close(got_tl, tl), "{rotation:?}: top-left went to {got_tl:?}, not {tl:?}");
            assert!(close(got_bl, bl), "{rotation:?}: bottom-left went to {got_bl:?}, not {bl:?}");
        }
    }

    /// The same formulas as `render::PageInfo::user_to_view` (fractions of the crop box turned
    /// and scaled to the displayed size), written out independently here as the oracle.
    #[test]
    fn agrees_with_the_renderer_formula() {
        for rotation in ALL {
            let page = cropped(rotation);
            let (w, h) = page.view_size();
            for (x, y) in [(130.0, 600.0), (499.0, 51.0), (321.5, 333.25), (40.0, 900.0)] {
                let (u, v) = ((x - 100.0) / 400.0, (750.0 - y) / 700.0);
                let (a, b) = match rotation {
                    Rotation::R90 => (1.0 - v, u),
                    Rotation::R180 => (1.0 - u, 1.0 - v),
                    Rotation::R270 => (v, 1.0 - u),
                    Rotation::R0 => (u, v),
                };
                let got = page.user_to_view_point(x, y).unwrap();
                assert!(close(got, [a * w, b * h]), "{rotation:?} ({x}, {y}): {got:?}");
            }
        }
    }

    #[test]
    fn boxes_round_trip_through_view_space_for_every_rotation() {
        let boxes = [PageRect::new(120.0, 300.0, 220.0, 410.0), PageRect::new(100.0, 50.0, 500.0, 750.0), PageRect::new(0.0, 0.0, 612.0, 792.0)];
        for rotation in ALL {
            for unit in [1.0, 2.5] {
                let page = cropped(rotation).with_user_unit(unit).unwrap();
                for user in boxes {
                    let view = page.user_to_view(user).unwrap();
                    assert!(view.x0 <= view.x1 && view.y0 <= view.y1, "{rotation:?}: {view:?} is not normalised");
                    let back = page.view_to_user(view).unwrap();
                    assert!(close_rect(back, user), "{rotation:?} x{unit}: {user:?} came back as {back:?}");
                }
            }
        }
    }

    #[test]
    fn quarter_turns_swap_the_box_sides() {
        let user = PageRect::new(120.0, 300.0, 220.0, 410.0);
        for rotation in ALL {
            let view = cropped(rotation).user_to_view(user).unwrap();
            let (w, h) = if rotation.swaps_axes() { (110.0, 100.0) } else { (100.0, 110.0) };
            assert!((view.width() - w).abs() < 1e-3 && (view.height() - h).abs() < 1e-3, "{rotation:?}: {view:?}");
        }
        // Unturned: the box top (user y 410) is 340 pt below the crop top (750), x is offset by 100.
        let view = cropped(Rotation::R0).user_to_view(user).unwrap();
        assert!(close([view.x0, view.y0], [20.0, 340.0]), "{view:?}");
    }

    #[test]
    fn pixels_and_points_take_the_dpi_explicitly() {
        assert_eq!(pt_to_px(72.0, 300.0), Some(300.0));
        assert_eq!(px_to_pt(150.0, 150.0), Some(72.0));
        for dpi in [0.0, -72.0, f32::NAN, f32::INFINITY] {
            assert_eq!((pt_to_px(72.0, dpi), px_to_pt(72.0, dpi)), (None, None), "dpi {dpi}");
        }
        assert_eq!(pt_to_px(f32::NAN, 72.0), None);

        let px = PixelRect { x0: 30, y0: 60, x1: 330, y1: 360 };
        let view = pixels_to_view(px, 144.0).unwrap();
        assert_eq!(view, ViewRect::new(15.0, 30.0, 165.0, 180.0));
        assert_eq!(view_to_pixels(view, 144.0), Some(px));
        // Covering, not rounding: a box that ends mid-pixel takes the whole pixel.
        assert_eq!(view_to_pixels(ViewRect::new(-5.0, 0.2, 0.9, 1.0), 72.0), Some(PixelRect { x0: 0, y0: 0, x1: 1, y1: 1 }));
        assert_eq!(view_to_pixels(ViewRect { x0: 0.0, y0: 0.0, x1: f32::NAN, y1: 1.0 }, 72.0), None);
    }

    #[test]
    fn region_pixels_scale_each_axis_on_its_own() {
        // A 400 × 220 raster of a 100 × 110 pt region: 4 px/pt across, 2 px/pt down.
        let region = ViewRect::new(20.0, 340.0, 120.0, 450.0);
        let got = region_pixels_to_view(region, 400, 220, PixelRect { x0: 40, y0: 20, x1: 360, y1: 200 }).unwrap();
        assert_eq!(got, ViewRect::new(30.0, 350.0, 110.0, 440.0));
        assert_eq!(region_pixels_to_view(region, 0, 220, PixelRect::default()), None);
        assert_eq!(region_pixels_to_view(ViewRect { x0: f32::NAN, y0: 0.0, x1: 1.0, y1: 1.0 }, 4, 4, PixelRect::default()), None);
    }

    #[test]
    fn refuses_degenerate_pages_and_points() {
        assert!(PageGeometry::new(PageRect { x0: 0.0, y0: 0.0, x1: 0.0, y1: 792.0 }, Rotation::R0).is_none());
        assert!(PageGeometry::new(PageRect { x0: f32::NAN, y0: 0.0, x1: 612.0, y1: 792.0 }, Rotation::R0).is_none());
        assert!(PageGeometry::new(PageRect { x0: 0.0, y0: 0.0, x1: f32::INFINITY, y1: 792.0 }, Rotation::R0).is_none());
        // Fields set by hand in the wrong order are a valid box, not an empty one.
        assert!(PageGeometry::new(PageRect { x0: 612.0, y0: 792.0, x1: 0.0, y1: 0.0 }, Rotation::R0).is_some());
        for unit in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(cropped(Rotation::R0).with_user_unit(unit).is_none(), "unit {unit}");
        }
        let page = cropped(Rotation::R90);
        assert_eq!(page.user_to_view_point(f32::NAN, 1.0), None);
        assert_eq!(page.view_to_user_point(1.0, f32::INFINITY), None);
        assert_eq!(page.user_to_view(PageRect { x0: 0.0, y0: f32::NAN, x1: 1.0, y1: 1.0 }), None);
        assert_eq!(page.view_to_user(ViewRect { x0: 0.0, y0: 0.0, x1: f32::NEG_INFINITY, y1: 1.0 }), None);
        // f32::MAX is finite, but scaling it by the user unit overflows: still no box rather
        // than an infinite one.
        let doubled = page.with_user_unit(2.0).unwrap();
        assert_eq!(doubled.user_to_view(PageRect::new(-f32::MAX, 0.0, f32::MAX, 1.0)), None);
        assert_eq!(doubled.user_to_view_point(f32::MAX, 1.0), None);
    }
}
