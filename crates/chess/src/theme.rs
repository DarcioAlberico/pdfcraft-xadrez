//! Board themes (plan HC4): one palette for every board PdfCraft shows — the squares, the last
//! move, the piece picked up and where it can go, the king in check, and the coordinates. The
//! colours are ours and the names generic. [`BROWN`] is the boards' colours before themes, and
//! stays the default.

use crate::draw::Rgb;
use crate::texture::Texture;

/// A colour laid over a square, premultiplied by its alpha: `[r, g, b, a]` with each of r, g and
/// b already scaled by `a`, as the boards' pictures and egui take it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tint(pub [u8; 4]);

impl Tint {
    /// The colour `base` shows under this tint.
    pub fn over(self, base: Rgb) -> Rgb {
        let [r, g, b, a] = self.0;
        let keep = 255 - u32::from(a);
        let mix = |top: u8, under: u8| {
            u8::try_from(((u32::from(top) * 255 + u32::from(under) * keep).saturating_add(127) / 255).min(255)).unwrap_or(u8::MAX)
        };
        Rgb(mix(r, base.0), mix(g, base.1), mix(b, base.2))
    }
}

/// A board's palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoardTheme {
    /// Kept in the preferences and named in the tools.
    pub id: &'static str,
    /// The English name, which the interface translates.
    pub name: &'static str,
    pub light: Rgb,
    pub dark: Rgb,
    /// The last move's two squares.
    pub last_move: Tint,
    /// The square of the piece picked up.
    pub selected: Tint,
    /// The squares it can go to (a dot, or a ring around a piece it would take).
    pub target: Tint,
    /// The ring around a king in check.
    pub check: Rgb,
    /// The drawings' arrows: their shaft's width in hundredths of a square, and their opacity.
    pub arrow_weight: u8,
    pub arrow_alpha: u8,
    /// Squares drawn with a texture over their colours (plan HC5), or flat.
    pub texture: Option<Texture>,
}

const LAST: Tint = Tint([155, 199, 0, 105]);
const PICKED: Tint = Tint([20, 85, 30, 110]);
const TARGET: Tint = Tint([20, 85, 30, 120]);
const RED: Rgb = Rgb(0xE0, 0x3E, 0x3E);
/// On green squares a green mark would not show: these themes mark in amber and blue.
const AMBER: Tint = Tint([170, 120, 0, 120]);
const BLUE_PICK: Tint = Tint([10, 50, 120, 120]);

const fn theme(id: &'static str, name: &'static str, light: Rgb, dark: Rgb) -> BoardTheme {
    BoardTheme {
        id,
        name,
        light,
        dark,
        last_move: LAST,
        selected: PICKED,
        target: TARGET,
        check: RED,
        arrow_weight: 17,
        arrow_alpha: 200,
        texture: None,
    }
}

/// The boards' colours before themes: the default.
pub const BROWN: BoardTheme = theme("brown", "Brown", Rgb(240, 217, 181), Rgb(181, 136, 99));

const fn textured(id: &'static str, name: &'static str, light: Rgb, dark: Rgb, texture: Texture) -> BoardTheme {
    BoardTheme { texture: Some(texture), ..theme(id, name, light, dark) }
}

/// Every theme, in the order to offer them; [`BROWN`] first, the textured ones last.
pub const THEMES: [BoardTheme; 18] = [
    BROWN,
    theme("light-wood", "Light wood", Rgb(246, 228, 199), Rgb(207, 168, 122)),
    theme("walnut", "Walnut", Rgb(224, 188, 140), Rgb(139, 94, 60)),
    theme("blue", "Blue", Rgb(222, 228, 232), Rgb(122, 150, 172)),
    theme("ice", "Ice blue", Rgb(236, 243, 248), Rgb(152, 184, 210)),
    BoardTheme { last_move: AMBER, selected: BLUE_PICK, target: BLUE_PICK, ..theme("green", "Green", Rgb(236, 238, 212), Rgb(118, 150, 86)) },
    BoardTheme { last_move: AMBER, selected: BLUE_PICK, target: BLUE_PICK, ..theme("olive", "Olive", Rgb(232, 230, 202), Rgb(148, 146, 96)) },
    theme("grey", "Grey", Rgb(226, 226, 226), Rgb(158, 158, 158)),
    theme("graphite", "Graphite", Rgb(196, 197, 201), Rgb(104, 107, 115)),
    theme("purple", "Purple", Rgb(234, 226, 242), Rgb(150, 124, 182)),
    theme("pink", "Pink", Rgb(247, 228, 233), Rgb(206, 142, 162)),
    theme("sand", "Sand", Rgb(242, 232, 208), Rgb(200, 176, 132)),
    theme("coral", "Coral", Rgb(250, 228, 214), Rgb(222, 140, 116)),
    BoardTheme {
        last_move: Tint([0, 70, 190, 150]),
        selected: Tint([0, 110, 40, 150]),
        target: Tint([0, 110, 40, 170]),
        check: Rgb(200, 0, 0),
        arrow_weight: 22,
        arrow_alpha: 240,
        ..theme("high-contrast", "High contrast", Rgb(255, 255, 255), Rgb(120, 120, 120))
    },
    // A deeper red: the brown's would sit close to the dark wood.
    BoardTheme { check: Rgb(190, 0, 0), ..textured("wood", "Wood", Rgb(229, 196, 146), Rgb(164, 110, 66), Texture::Wood) },
    textured("marble", "Marble", Rgb(236, 234, 229), Rgb(126, 134, 131), Texture::Marble),
    textured("paper", "Paper", Rgb(247, 242, 230), Rgb(198, 188, 168), Texture::Paper),
    // A book's diagram: white squares, the dark ones hatched in black lines.
    textured("book", "Book print", Rgb(255, 255, 255), Rgb(40, 40, 40), Texture::Hatched),
];

/// The theme `id` names, whatever its case.
pub fn by_id(id: &str) -> Option<&'static BoardTheme> {
    THEMES.iter().find(|t| t.id.eq_ignore_ascii_case(id))
}

/// Relative luminance (WCAG 2.2).
fn luminance(c: Rgb) -> f32 {
    let lin = |v: u8| {
        let s = f32::from(v) / 255.0;
        if s <= 0.040_45 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(c.0) + 0.7152 * lin(c.1) + 0.0722 * lin(c.2)
}

/// The contrast ratio of two colours (WCAG 2.2), from 1 to 21.
pub fn contrast(a: Rgb, b: Rgb) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Least contrast of the coordinates on their square: text, so 4.5:1.
pub const COORDINATE_CONTRAST: f32 = 4.5;

impl BoardTheme {
    /// The square colour (`light` square or dark).
    pub fn square(&self, light: bool) -> Rgb {
        if light { self.light } else { self.dark }
    }

    /// The colour a square reads as from afar: its own, or for hatched dark squares, the light
    /// colour with a quarter of the lines' (what marks and coordinates must stand out from).
    pub fn ground(&self, light: bool) -> Rgb {
        if light || self.texture != Some(Texture::Hatched) {
            return self.square(light);
        }
        let mix = |a: u8, b: u8| u8::try_from((u16::from(a) * 3 + u16::from(b)) / 4).unwrap_or(u8::MAX);
        Rgb(mix(self.light.0, self.dark.0), mix(self.light.1, self.dark.1), mix(self.light.2, self.dark.2))
    }

    /// The coordinates' colour on a `light` square or a dark one: the other square's colour,
    /// darkened or lightened (towards whichever reads better) until it reads at
    /// [`COORDINATE_CONTRAST`].
    pub fn coordinate_ink(&self, light: bool) -> Rgb {
        let ground = self.ground(light);
        let start = self.square(!light);
        let (black, white) = (Rgb(0, 0, 0), Rgb(255, 255, 255));
        let toward = if contrast(black, ground) >= contrast(white, ground) { black } else { white };
        let mut best = start;
        for step in 0..=20u8 {
            let t = f32::from(step) / 20.0;
            let mix = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round().clamp(0.0, 255.0) as u8;
            best = Rgb(mix(start.0, toward.0), mix(start.1, toward.1), mix(start.2, toward.2));
            if contrast(best, ground) >= COORDINATE_CONTRAST {
                break;
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn distance(a: Rgb, b: Rgb) -> f32 {
        let d = |x: u8, y: u8| f32::from(x) - f32::from(y);
        (d(a.0, b.0).powi(2) + d(a.1, b.1).powi(2) + d(a.2, b.2).powi(2)).sqrt()
    }

    #[test]
    fn ids_are_unique_and_brown_comes_first() {
        assert_eq!(THEMES[0], BROWN);
        assert_eq!((BROWN.light, BROWN.dark), (Rgb(240, 217, 181), Rgb(181, 136, 99)), "the boards' colours before themes");
        for (i, t) in THEMES.iter().enumerate() {
            assert!(THEMES.iter().skip(i + 1).all(|o| o.id != t.id && o.name != t.name), "{}", t.id);
            assert_eq!(by_id(&t.id.to_uppercase()), Some(t));
        }
        assert_eq!(by_id("cburnett"), None);
    }

    /// Every theme reads: coordinates at 4.5:1 on both squares; the last move, the piece picked
    /// up and its targets plainly off both squares; the check ring off both; the two squares
    /// apart.
    #[test]
    fn every_theme_keeps_its_marks_visible() {
        for t in &THEMES {
            for light in [true, false] {
                let square = t.ground(light);
                let ink = t.coordinate_ink(light);
                assert!(contrast(ink, square) >= COORDINATE_CONTRAST, "{} coordinates on {light}: {:.2}", t.id, contrast(ink, square));
                for (what, tint) in [("last move", t.last_move), ("selected", t.selected), ("target", t.target)] {
                    let shown = tint.over(square);
                    assert!(distance(shown, square) >= 40.0, "{} {what} on {light}: {shown:?} over {square:?}", t.id);
                }
                assert!(distance(t.check, square) >= 80.0, "{} check on {light}", t.id);
            }
            assert!(contrast(t.ground(true), t.ground(false)) >= 1.4, "{}: squares apart {:.2}", t.id, contrast(t.ground(true), t.ground(false)));
            assert!((10..=30).contains(&t.arrow_weight) && t.arrow_alpha >= 160, "{}: arrows", t.id);
        }
    }

    #[test]
    fn a_tint_blends_as_premultiplied() {
        assert_eq!(Tint([0, 0, 0, 0]).over(Rgb(10, 20, 30)), Rgb(10, 20, 30));
        assert_eq!(Tint([255, 0, 0, 255]).over(Rgb(10, 20, 30)), Rgb(255, 0, 0));
        assert_eq!(LAST.over(Rgb(240, 217, 181)), Rgb(255, 255, 106));
    }
}
