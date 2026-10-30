//! Piece sets that ship with PdfCraft (plan HC1, decision D13, which revises D4).
//!
//! Each is twelve SVG files under `assets/chess/pieces/<id>/`, compiled in and read by the same
//! reader as the user's own folders ([`super::svg`]), so a set shipped here draws exactly as the
//! same files would from a `pieces` folder. Every file is listed in `ATTRIBUTION.toml` with its
//! author, source and licence (all on the allowlist of AGENTS.md §1.2).
//!
//! The first eight are openly licensed sets by other authors: the SVGs lichess publishes, at the
//! commit `ATTRIBUTION.toml` names, with the licence texts beside the files; `source` is where the
//! author keeps the set. The rest were drawn for PdfCraft (contributor-original, under the
//! project's licence) and are described in `assets/chess/pieces/README.md`.

use pdfcraft_chess::draw::{PieceGlyph, PieceSet};

use super::PieceError;

/// A piece set compiled into PdfCraft.
#[derive(Debug)]
pub struct Bundled {
    /// The name the set is chosen by (`PDFCRAFT_CHESS_PIECES=chessnut`, the tools' `pieces`).
    pub id: &'static str,
    /// The set's name as shown to people.
    pub name: &'static str,
    pub author: &'static str,
    /// SPDX identifier.
    pub licence: &'static str,
    /// Where the author publishes the set; for a set drawn for PdfCraft, its README here.
    pub source: &'static str,
    /// White pawn, knight, bishop, rook, queen, king, then Black's, as [`PieceSet::glyphs`].
    files: [&'static [u8]; 12],
}

/// The twelve files of set `$id`, in [`PieceSet::glyphs`] order.
macro_rules! files {
    ($id:literal) => {
        [
            include_bytes!(concat!("../../../../assets/chess/pieces/", $id, "/wP.svg")),
            include_bytes!(concat!("../../../../assets/chess/pieces/", $id, "/wN.svg")),
            include_bytes!(concat!("../../../../assets/chess/pieces/", $id, "/wB.svg")),
            include_bytes!(concat!("../../../../assets/chess/pieces/", $id, "/wR.svg")),
            include_bytes!(concat!("../../../../assets/chess/pieces/", $id, "/wQ.svg")),
            include_bytes!(concat!("../../../../assets/chess/pieces/", $id, "/wK.svg")),
            include_bytes!(concat!("../../../../assets/chess/pieces/", $id, "/bP.svg")),
            include_bytes!(concat!("../../../../assets/chess/pieces/", $id, "/bN.svg")),
            include_bytes!(concat!("../../../../assets/chess/pieces/", $id, "/bB.svg")),
            include_bytes!(concat!("../../../../assets/chess/pieces/", $id, "/bR.svg")),
            include_bytes!(concat!("../../../../assets/chess/pieces/", $id, "/bQ.svg")),
            include_bytes!(concat!("../../../../assets/chess/pieces/", $id, "/bK.svg")),
        ]
    };
}

/// A set drawn for PdfCraft: its id and name.
macro_rules! original {
    ($id:literal, $name:literal) => {
        Bundled {
            id: $id,
            name: $name,
            author: "PdfCraft contributors",
            licence: "MIT OR Apache-2.0",
            source: "assets/chess/pieces/README.md",
            files: files!($id),
        }
    };
}

/// Every set PdfCraft ships, in the order they are offered.
pub const SETS: &[Bundled] = &[
    Bundled {
        id: "chessnut",
        name: "Chessnut",
        author: "Alexis Luengas",
        licence: "Apache-2.0",
        source: "https://github.com/LexLuengas/chessnut-pieces",
        files: files!("chessnut"),
    },
    Bundled {
        id: "rhosgfx",
        name: "RhosGFX",
        author: "RhosGFX",
        licence: "CC0-1.0",
        source: "https://rhosgfx.itch.io/vector-chess-pieces",
        files: files!("rhosgfx"),
    },
    Bundled {
        id: "kiwen-suwi",
        name: "kiwen suwi",
        author: "neverRare",
        licence: "CC-BY-4.0",
        source: "https://github.com/neverRare/kiwen-suwi",
        files: files!("kiwen-suwi"),
    },
    Bundled {
        id: "firi",
        name: "Firi",
        author: "James Faure",
        licence: "CC-BY-4.0",
        source: "https://github.com/jfaure/Firi-pieceset",
        files: files!("firi"),
    },
    Bundled {
        id: "fantasy",
        name: "Fantasy",
        author: "Maurizio Monge",
        licence: "MIT",
        source: "https://github.com/maurimo/chess-art",
        files: files!("fantasy"),
    },
    Bundled {
        id: "spatial",
        name: "Spatial",
        author: "Maurizio Monge",
        licence: "MIT",
        source: "https://github.com/maurimo/chess-art",
        files: files!("spatial"),
    },
    Bundled {
        id: "celtic",
        name: "Celtic",
        author: "Maurizio Monge",
        licence: "MIT",
        source: "https://github.com/maurimo/chess-art",
        files: files!("celtic"),
    },
    Bundled {
        id: "shapes",
        name: "Shapes",
        author: "flugsio",
        licence: "CC-BY-SA-4.0",
        source: "https://github.com/flugsio/chess_shapes",
        files: files!("shapes"),
    },
    original!("classic", "Classic"),
    original!("tokens", "Tokens"),
    original!("pixelart", "Pixel Art"),
    original!("bauhaus", "Bauhaus"),
    original!("facets", "Facets"),
    original!("glass", "Stained Glass"),
    original!("moves", "Moves"),
    original!("chubby", "Chubby"),
    original!("deco", "Art Deco"),
    original!("gothic", "Gothic"),
    original!("wood", "Wood"),
    original!("sketch", "Sketch"),
];

/// The shipped set named `id` (ASCII case ignored).
pub fn by_id(id: &str) -> Option<&'static Bundled> {
    SETS.iter().find(|s| s.id.eq_ignore_ascii_case(id))
}

impl Bundled {
    /// The set's pieces, read from its files.
    pub fn load(&self) -> Result<PieceSet, PieceError> {
        let mut glyphs: [PieceGlyph; 12] = Default::default();
        for ((glyph, bytes), name) in glyphs.iter_mut().zip(self.files).zip(FILE_NAMES) {
            *glyph = super::svg::glyph_from_svg(bytes).map_err(|reason| PieceError::Bundled { id: self.id, name, reason })?;
        }
        Ok(PieceSet { glyphs, name: self.name.to_owned() })
    }
}

/// The file of each glyph, in [`PieceSet::glyphs`] order (for errors).
const FILE_NAMES: [&str; 12] =
    ["wP.svg", "wN.svg", "wB.svg", "wR.svg", "wQ.svg", "wK.svg", "bP.svg", "bN.svg", "bB.svg", "bR.svg", "bQ.svg", "bK.svg"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shipped_set_reads_whole_and_fills_its_squares() {
        assert_eq!(SETS.len(), 20);
        for set in SETS {
            let pieces = set.load().unwrap_or_else(|e| panic!("{}: {e}", set.id));
            assert_eq!(pieces.name, set.name);
            for (glyph, name) in pieces.glyphs.iter().zip(FILE_NAMES) {
                assert!(!glyph.layers.is_empty(), "{} {name}: nothing drawn", set.id);
                let all: Vec<_> = glyph.layers.iter().flat_map(|l| l.subpaths.iter().cloned()).collect();
                let b = pdfcraft_chess::draw::bounds(&all).unwrap_or_else(|| panic!("{} {name}: no bounds", set.id));
                // Drawn in its square, and not a speck in a corner of it. A file may draw a little
                // past its own box (spatial's queen reaches 0.11 below it; a browser crops that).
                assert!(b[0] >= -0.15 && b[1] >= -0.15 && b[2] <= 1.15 && b[3] <= 1.15, "{} {name}: {b:?}", set.id);
                assert!(b[2] - b[0] > 0.3 && b[3] - b[1] > 0.3, "{} {name}: {b:?}", set.id);
            }
        }
    }

    /// A set whose files were mixed up shows it: two of its pieces drawn alike, or a white piece
    /// no lighter than the black one of its kind.
    #[test]
    fn every_set_draws_twelve_different_pieces_and_white_is_the_light_side() {
        for set in SETS {
            let pieces = set.load().unwrap_or_else(|e| panic!("{}: {e}", set.id));
            for (i, a) in pieces.glyphs.iter().enumerate() {
                for (j, b) in pieces.glyphs.iter().enumerate().skip(i + 1) {
                    assert_ne!(a, b, "{}: {} and {} are drawn alike", set.id, FILE_NAMES[i], FILE_NAMES[j]);
                }
            }
            for k in 0..6 {
                let (white, black) = (lightness(&pieces.glyphs[k]), lightness(&pieces.glyphs[k + 6]));
                assert!(white > black, "{}: {} ({white}) is not lighter than {} ({black})", set.id, FILE_NAMES[k], FILE_NAMES[k + 6]);
            }
        }
    }

    /// The luminance of a piece's fills, each weighted by the area of its bounding box.
    fn lightness(glyph: &PieceGlyph) -> f32 {
        let (mut sum, mut weight) = (0.0, 0.0);
        for layer in &glyph.layers {
            if let pdfcraft_chess::draw::Paint::Fill { color, .. } = layer.paint
                && let Some(b) = pdfcraft_chess::draw::bounds(&layer.subpaths)
            {
                let area = (b[2] - b[0]) * (b[3] - b[1]);
                sum += area * (0.2126 * f32::from(color.0) + 0.7152 * f32::from(color.1) + 0.0722 * f32::from(color.2));
                weight += area;
            }
        }
        sum / weight
    }

    /// Not a check: writes a board per set to `$PDFCRAFT_PIECES_PREVIEW` to look at.
    #[test]
    #[ignore]
    fn preview_boards() {
        let Some(dir) = std::env::var_os("PDFCRAFT_PIECES_PREVIEW") else { return };
        let fen = pdfcraft_chess::fen::parse_placement("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR").unwrap();
        let mut html = String::from("<html><body style='background:#888'>");
        for set in SETS {
            let pieces = set.load().unwrap();
            let style = pdfcraft_chess::draw::BoardStyle { square: 40.0, ..Default::default() };
            let svg = pdfcraft_chess::svg::to_svg(&pdfcraft_chess::draw::board(&fen, &pieces, &style), set.name);
            html.push_str(&format!("<div style='display:inline-block;margin:6px'><p>{}</p>{svg}</div>", set.id));
        }
        std::fs::write(std::path::Path::new(&dir).join("sets.html"), html).unwrap();
    }

    #[test]
    fn ids_are_unique_and_found_whatever_their_case() {
        for (i, set) in SETS.iter().enumerate() {
            assert!(SETS.iter().skip(i + 1).all(|other| other.id != set.id), "{} twice", set.id);
            assert_eq!(by_id(&set.id.to_ascii_uppercase()).map(|s| s.id), Some(set.id));
        }
        assert!(by_id("cburnett").is_none());
        assert!(by_id("").is_none());
    }
}
