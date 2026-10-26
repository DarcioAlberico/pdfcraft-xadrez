//! Piece sets that ship with PdfCraft (plan HC1, decision D13, which revises D4).
//!
//! Each is twelve SVG files under `assets/chess/pieces/<id>/`, compiled in and read by the same
//! reader as the user's own folders ([`super::svg`]), so a set shipped here draws exactly as the
//! same files would from a `pieces` folder. Every file is listed in `ATTRIBUTION.toml` with its
//! author, source and licence (all on the allowlist of AGENTS.md §1.2); the licence texts sit
//! beside the files. The SVGs are the ones lichess publishes, at the commit `ATTRIBUTION.toml`
//! names; `source` is where the author keeps the set.

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
    /// Where the author publishes the set.
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
        assert_eq!(SETS.len(), 8);
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
