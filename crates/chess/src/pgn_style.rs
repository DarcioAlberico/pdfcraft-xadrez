//! What a PGN writer leaves out (plan A8 V4): the variations past a depth, the comments, the
//! annotation glyphs; and the dialect it writes for (plan A9-F7).
//!
//! One type for both PGN writers (the board's game and the document's export), and for the tools
//! and menus that offer the choice. The default writes everything, in standard PGN, so a game
//! read and written again loses nothing; leaving something out is always asked for.

/// Which variations a PGN keeps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Variations {
    /// Every variation, however deep.
    #[default]
    All,
    /// The variations nested at most this deep: 0 is the main line alone, 1 the main line's
    /// variations, 2 theirs too.
    Depth(usize),
}

/// Which program's reading of PGN the writer writes for (plan A9-F7, R6-06, decision D18).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PgnDialect {
    /// PGN as its standard writes it: every glyph as read, Black's own codes too (`$37`), and the
    /// engine's word as a comment (`{Engine: …}`).
    #[default]
    Standard,
    /// PGN for lichess, which knows fewer glyphs ([`crate::nag::LICHESS`]): Black's code of a pair
    /// is written as White's (`$37` as `$36`; the glyph is on Black's move, which says whose it
    /// is), a glyph lichess does not know goes into the move's comment as its sign or name, and
    /// the engine's value as lichess's `[%eval …]` command.
    Lichess,
    /// PGN for ChessBase, as the D18 session saw it read and write (2026-11-19): Black's code of a
    /// pair as White's, as for lichess (ChessBase drops `$37`, `$23`, `$133` when it saves), and a
    /// diagram mark as ChessBase's own comment `{[#]}` (it drops `$201`, `$220` and `$221`).
    ChessBase,
}

impl PgnDialect {
    /// All of them, in the order a choice offers them.
    pub const ALL: [PgnDialect; 3] = [PgnDialect::Standard, PgnDialect::Lichess, PgnDialect::ChessBase];

    /// The name the tools and the settings use: `standard`, `lichess` or `chessbase`.
    pub fn name(self) -> &'static str {
        match self {
            PgnDialect::Standard => "standard",
            PgnDialect::Lichess => "lichess",
            PgnDialect::ChessBase => "chessbase",
        }
    }

    /// The dialect named `name` ([`PgnDialect::name`]).
    pub fn from_name(name: &str) -> Option<PgnDialect> {
        PgnDialect::ALL.into_iter().find(|d| d.name() == name)
    }
}

/// What a PGN writer writes of a game's moves, and for which program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PgnStyle {
    pub variations: Variations,
    /// Comments in braces, and the arrows and coloured squares written in them.
    pub comments: bool,
    /// Annotation glyphs (`$1`, `$220`).
    pub nags: bool,
    /// The program the PGN is for (standard PGN by default).
    pub dialect: PgnDialect,
}

impl Default for PgnStyle {
    fn default() -> PgnStyle {
        PgnStyle::FULL
    }
}

impl PgnStyle {
    /// Everything: what a game read and written again needs.
    pub const FULL: PgnStyle = PgnStyle { variations: Variations::All, comments: true, nags: true, dialect: PgnDialect::Standard };

    /// Whether a variation nested `depth` deep (1: a variation of the main line) is written.
    pub fn keeps(self, depth: usize) -> bool {
        match self.variations {
            Variations::All => true,
            Variations::Depth(most) => depth <= most,
        }
    }

    /// The `variations` option as the tools write it: `all`, `main` or `depth:N`.
    pub fn variations_from_name(name: &str) -> Option<Variations> {
        match name {
            "all" => Some(Variations::All),
            "main" => Some(Variations::Depth(0)),
            _ => name.strip_prefix("depth:").and_then(|n| n.parse::<usize>().ok()).map(Variations::Depth),
        }
    }

    /// A `comments` or `nags` option as the tools write it: `all` or `none`.
    pub fn switch_from_name(name: &str) -> Option<bool> {
        match name {
            "all" => Some(true),
            "none" => Some(false),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_options_read_as_the_tools_write_them() {
        assert_eq!(PgnStyle::variations_from_name("all"), Some(Variations::All));
        assert_eq!(PgnStyle::variations_from_name("main"), Some(Variations::Depth(0)));
        assert_eq!(PgnStyle::variations_from_name("depth:2"), Some(Variations::Depth(2)));
        for bad in ["", "depth:", "depth:-1", "depth:x", "Main"] {
            assert_eq!(PgnStyle::variations_from_name(bad), None, "{bad}");
        }
        assert_eq!((PgnStyle::switch_from_name("all"), PgnStyle::switch_from_name("none")), (Some(true), Some(false)));
        assert_eq!(PgnStyle::switch_from_name("some"), None);
        let main = PgnStyle { variations: Variations::Depth(0), ..PgnStyle::FULL };
        assert!(!main.keeps(1) && PgnStyle::default().keeps(usize::MAX));
        let two = PgnStyle { variations: Variations::Depth(2), ..PgnStyle::FULL };
        assert!(two.keeps(2) && !two.keeps(3));
        assert_eq!(PgnStyle::FULL.dialect, PgnDialect::Standard);
        for d in PgnDialect::ALL {
            assert_eq!(PgnDialect::from_name(d.name()), Some(d));
        }
        assert_eq!(PgnDialect::from_name("Lichess"), None);
    }
}
