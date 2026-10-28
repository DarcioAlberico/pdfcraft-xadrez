//! What a PGN writer leaves out (plan A8 V4): the variations past a depth, the comments, the
//! annotation glyphs.
//!
//! One type for both PGN writers (the board's game and the document's export), and for the tools
//! and menus that offer the choice. The default writes everything, so a game read and written
//! again loses nothing; leaving something out is always asked for.

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

/// What a PGN writer writes of a game's moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PgnStyle {
    pub variations: Variations,
    /// Comments in braces, and the arrows and coloured squares written in them.
    pub comments: bool,
    /// Annotation glyphs (`$1`, `$220`).
    pub nags: bool,
}

impl Default for PgnStyle {
    fn default() -> PgnStyle {
        PgnStyle::FULL
    }
}

impl PgnStyle {
    /// Everything: what a game read and written again needs.
    pub const FULL: PgnStyle = PgnStyle { variations: Variations::All, comments: true, nags: true };

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
    }
}
